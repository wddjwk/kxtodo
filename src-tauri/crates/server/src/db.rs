//! SQLite 存储：用户（含 auth_key）、token、加密实体。
//! 单进程 + Mutex<Connection>：个人同步服务的并发量完全够用。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use serde_json::json;

use crate::error::{ServerError, ServerResult};

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub auth_key: String,
    pub current_seq: u64,
}

#[derive(Debug, Clone)]
pub struct EntityRow {
    pub entity_id: String,
    pub nonce: String,
    pub ciphertext: String,
    pub seq: u64,
}

/// 图片 blob 行；`ciphertext` 为 None 表示只取了元数据（增量清单不带密文）。
#[derive(Debug, Clone)]
pub struct ImageRow {
    pub image_id: String,
    pub kind: String,
    pub node_id: String,
    pub filename: String,
    pub nonce: String,
    pub ciphertext: Option<Vec<u8>>,
    pub content_hash: String,
    pub size: i64,
    pub updated_at: String,
    pub updated_by: String,
    pub deleted: bool,
    pub seq: u64,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    auth_key TEXT NOT NULL,
    current_seq INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS users_legacy (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL DEFAULT '',
    auth_key TEXT NOT NULL,
    current_seq INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    migrated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tokens (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS entities (
    user_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    nonce TEXT NOT NULL,
    ciphertext TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    seq INTEGER NOT NULL,
    PRIMARY KEY (user_id, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_entities_user_seq ON entities(user_id, seq);
CREATE TABLE IF NOT EXISTS images (
    user_id TEXT NOT NULL,
    image_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    node_id TEXT NOT NULL DEFAULT '',
    filename TEXT NOT NULL,
    nonce TEXT NOT NULL,
    ciphertext BLOB NOT NULL,
    content_hash TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0,
    seq INTEGER NOT NULL,
    PRIMARY KEY (user_id, image_id)
);
CREATE INDEX IF NOT EXISTS idx_images_user_seq ON images(user_id, seq);
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

impl Db {
    pub fn open(path: &Path) -> ServerResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 本库的身份：建库时生成一次，重启不变，库文件被重建就会变。
    ///
    /// 客户端把它和 `runtime/sync.json` 里缓存的值比对，用来判断「我连的还是不是同一台
    /// 主机 / 它的库是不是被重建过」。变了就必须把拉取水位归零、推送台账清空重新播种——
    /// 否则新库的 `current_seq` 从 1 开始而客户端水位停在几百，表现是**静默地**
    /// 什么都拉不到、推的时候一路 OCC 409。
    pub fn instance_id(&self) -> ServerResult<String> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = 'instanceId'", [], |row| {
                row.get(0)
            })
            .ok();
        if let Some(instance_id) = existing.filter(|value| !value.is_empty()) {
            return Ok(instance_id);
        }
        let instance_id = format!("srv-{}", crate::util::random_hex(8));
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('instanceId', ?1)",
            [&instance_id],
        )?;
        Ok(instance_id)
    }

    /// v0.5.0 及以前的 users 表带 email 列（账户 = 用户名 + 邮箱）。
    ///
    /// 旧账户的 auth_key 是用 `kxtodo|username|email` 派生的，新客户端只填用户名+密码，
    /// 再也算不出同一把密钥——把它们留在 users 里只会白占用户名（同名注册被 ACCOUNT_EXISTS
    /// 拒掉且永远登录不上）。所以整体归档进 users_legacy 并重建干净的 users 表；
    /// 旧账户的实体/图片/token 原样保留（token 因 JOIN 不到 users 自然失效），
    /// 管理台可查看并一键删除。返回归档的账户数。
    pub fn migrate_legacy_accounts(&self) -> ServerResult<usize> {
        let mut conn = self.conn.lock().unwrap();
        let has_email = {
            let mut stmt = conn.prepare("PRAGMA table_info(users)")?;
            let mut rows = stmt.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name == "email" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_email {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO users_legacy
                (id, username, email, auth_key, current_seq, created_at, migrated_at)
             SELECT id, username, email, auth_key, current_seq, created_at, ?1 FROM users",
            [crate::util::now_iso()],
        )?;
        let archived: usize = tx.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        tx.execute("DROP TABLE users", [])?;
        tx.execute_batch(
            "CREATE TABLE users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                auth_key TEXT NOT NULL,
                current_seq INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );",
        )?;
        tx.commit()?;
        Ok(archived)
    }

    pub fn create_user(&self, username: &str, auth_key: &str) -> ServerResult<String> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE username = ?1",
                [username],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)?;
        if exists {
            return Err(ServerError::account_exists());
        }
        let id = format!("user-{}", crate::util::random_hex(8));
        conn.execute(
            "INSERT INTO users (id, username, auth_key, current_seq, created_at)
             VALUES (?1, ?2, ?3, 0, ?4)",
            rusqlite::params![id, username, auth_key, crate::util::now_iso()],
        )?;
        Ok(id)
    }

    pub fn find_user(&self, username: &str) -> ServerResult<Option<UserRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, auth_key, current_seq FROM users WHERE username = ?1",
        )?;
        let mut rows = stmt.query([username])?;
        Ok(match rows.next()? {
            Some(row) => Some(UserRow {
                id: row.get(0)?,
                username: row.get(1)?,
                auth_key: row.get(2)?,
                current_seq: row.get::<_, i64>(3)? as u64,
            }),
            None => None,
        })
    }

    pub fn insert_token(&self, token_hash: &str, user_id: &str, expires_at: &str) -> ServerResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO tokens (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![token_hash, user_id, expires_at],
        )?;
        Ok(())
    }

    /// 通过 token hash 找有效用户；顺带清理过期 token。
    pub fn user_by_token(&self, token_hash: &str) -> ServerResult<Option<UserRow>> {
        let conn = self.conn.lock().unwrap();
        let now = crate::util::now_iso();
        conn.execute("DELETE FROM tokens WHERE expires_at < ?1", [now.as_str()])?;
        let mut stmt = conn.prepare(
            "SELECT u.id, u.username, u.auth_key, u.current_seq
             FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE t.token_hash = ?1 AND t.expires_at >= ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![token_hash, now.as_str()])?;
        Ok(match rows.next()? {
            Some(row) => Some(UserRow {
                id: row.get(0)?,
                username: row.get(1)?,
                auth_key: row.get(2)?,
                current_seq: row.get::<_, i64>(3)? as u64,
            }),
            None => None,
        })
    }

    pub fn delete_token(&self, token_hash: &str) -> ServerResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM tokens WHERE token_hash = ?1", [token_hash])?;
        Ok(())
    }

    pub fn entity_count(&self, user_id: &str) -> ServerResult<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM entities WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?)
    }

    /// OCC 写入：base 必须等于该实体当前 seq（不存在时 base 必须为 0）。
    /// 内容 hash 相同则为幂等重放：返回当前 seq 且 `changed = false`，不推进版本。
    pub fn put_entity(
        &self,
        user_id: &str,
        entity_id: &str,
        base: u64,
        nonce: &str,
        ciphertext: &str,
        content_hash: &str,
    ) -> ServerResult<(u64, bool)> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let current: Option<(i64, String)> = tx
            .query_row(
                "SELECT seq, content_hash FROM entities WHERE user_id = ?1 AND entity_id = ?2",
                rusqlite::params![user_id, entity_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        match current {
            Some((seq, existing_hash)) => {
                let seq = seq as u64;
                if base != seq {
                    return Err(ServerError::version_conflict(seq));
                }
                // 内容去重：相同密文重放不推进版本
                if existing_hash == content_hash {
                    return Ok((seq, false));
                }
                tx.execute(
                    "UPDATE entities SET nonce = ?3, ciphertext = ?4, content_hash = ?5 WHERE
                     user_id = ?1 AND entity_id = ?2",
                    rusqlite::params![user_id, entity_id, nonce, ciphertext, content_hash],
                )?;
                let new_seq = bump_seq(&tx, user_id)?;
                tx.execute(
                    "UPDATE entities SET seq = ?3 WHERE user_id = ?1 AND entity_id = ?2",
                    rusqlite::params![user_id, entity_id, new_seq as i64],
                )?;
                tx.commit()?;
                Ok((new_seq, true))
            }
            None => {
                if base != 0 {
                    // 不存在但带了旧 base：按冲突处理，返回“当前版本 0”
                    return Err(ServerError::version_conflict(0));
                }
                let new_seq = bump_seq(&tx, user_id)?;
                tx.execute(
                    "INSERT INTO entities (user_id, entity_id, nonce, ciphertext, content_hash, seq)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        user_id,
                        entity_id,
                        nonce,
                        ciphertext,
                        content_hash,
                        new_seq as i64
                    ],
                )?;
                tx.commit()?;
                Ok((new_seq, true))
            }
        }
    }

    pub fn changes_since(&self, user_id: &str, since: u64, limit: u64) -> ServerResult<(Vec<EntityRow>, u64)> {
        let conn = self.conn.lock().unwrap();
        let current_seq: u64 = conn
            .query_row(
                "SELECT current_seq FROM users WHERE id = ?1",
                [user_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|seq| seq as u64)?;
        let mut stmt = conn.prepare(
            "SELECT entity_id, nonce, ciphertext, seq FROM entities
             WHERE user_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let mut rows = stmt.query(rusqlite::params![user_id, since as i64, limit as i64])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(EntityRow {
                entity_id: row.get(0)?,
                nonce: row.get(1)?,
                ciphertext: row.get(2)?,
                seq: row.get::<_, i64>(3)? as u64,
            });
        }
        Ok((out, current_seq))
    }

    pub fn get_entity(&self, user_id: &str, entity_id: &str) -> ServerResult<Option<EntityRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT entity_id, nonce, ciphertext, seq FROM entities
             WHERE user_id = ?1 AND entity_id = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![user_id, entity_id])?;
        Ok(match rows.next()? {
            Some(row) => Some(EntityRow {
                entity_id: row.get(0)?,
                nonce: row.get(1)?,
                ciphertext: row.get(2)?,
                seq: row.get::<_, i64>(3)? as u64,
            }),
            None => None,
        })
    }

    pub fn storage_bytes(&self, user_id: &str) -> ServerResult<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(ciphertext)), 0) FROM entities WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?)
    }

    // -----------------------------------------------------------------------
    // 图片 blob（v0.5.0）：内容寻址、不可变，没有 LWW/OCC
    // -----------------------------------------------------------------------

    /// 写入图片：内容哈希相同即幂等重放（不推进版本，`changed = false`），否则 upsert 并推进 seq。
    #[allow(clippy::too_many_arguments)]
    pub fn put_image(
        &self,
        user_id: &str,
        image_id: &str,
        kind: &str,
        node_id: &str,
        filename: &str,
        nonce: &str,
        ciphertext: &[u8],
        content_hash: &str,
        updated_at: &str,
        updated_by: &str,
    ) -> ServerResult<(u64, bool)> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT seq, content_hash FROM images WHERE user_id = ?1 AND image_id = ?2",
                rusqlite::params![user_id, image_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        if let Some((seq, hash)) = existing {
            if hash == content_hash {
                return Ok((seq as u64, false));
            }
        }
        let new_seq = bump_seq(&tx, user_id)?;
        tx.execute(
            "INSERT INTO images (user_id, image_id, kind, node_id, filename, nonce, ciphertext,
                                  content_hash, size, updated_at, updated_by, deleted, seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12)
             ON CONFLICT(user_id, image_id) DO UPDATE SET
                kind = excluded.kind,
                node_id = excluded.node_id,
                filename = excluded.filename,
                nonce = excluded.nonce,
                ciphertext = excluded.ciphertext,
                content_hash = excluded.content_hash,
                size = excluded.size,
                updated_at = excluded.updated_at,
                updated_by = excluded.updated_by,
                deleted = 0,
                seq = excluded.seq",
            rusqlite::params![
                user_id,
                image_id,
                kind,
                node_id,
                filename,
                nonce,
                ciphertext,
                content_hash,
                ciphertext.len() as i64,
                updated_at,
                updated_by,
                new_seq as i64,
            ],
        )?;
        tx.commit()?;
        Ok((new_seq, true))
    }

    /// 增量图片元数据（不带密文）+ 当前总版本号。
    pub fn image_changes_since(
        &self,
        user_id: &str,
        since: u64,
        limit: u64,
    ) -> ServerResult<(Vec<ImageRow>, u64)> {
        let conn = self.conn.lock().unwrap();
        let current_seq: u64 = conn
            .query_row(
                "SELECT current_seq FROM users WHERE id = ?1",
                [user_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|seq| seq as u64)?;
        let mut stmt = conn.prepare(
            "SELECT image_id, kind, node_id, filename, content_hash, size, updated_at, updated_by,
                    deleted, seq
             FROM images WHERE user_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let mut rows = stmt.query(rusqlite::params![user_id, since as i64, limit as i64])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(ImageRow {
                image_id: row.get(0)?,
                kind: row.get(1)?,
                node_id: row.get(2)?,
                filename: row.get(3)?,
                nonce: String::new(),
                ciphertext: None,
                content_hash: row.get(4)?,
                size: row.get(5)?,
                updated_at: row.get(6)?,
                updated_by: row.get(7)?,
                deleted: row.get::<_, i64>(8)? != 0,
                seq: row.get::<_, i64>(9)? as u64,
            });
        }
        Ok((out, current_seq))
    }

    /// 取单张图片的密文与 nonce（下载用）。
    pub fn get_image(&self, user_id: &str, image_id: &str) -> ServerResult<Option<ImageRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT image_id, kind, node_id, filename, nonce, ciphertext, content_hash, size,
                    updated_at, updated_by, deleted, seq
             FROM images WHERE user_id = ?1 AND image_id = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![user_id, image_id])?;
        Ok(match rows.next()? {
            Some(row) => Some(ImageRow {
                image_id: row.get(0)?,
                kind: row.get(1)?,
                node_id: row.get(2)?,
                filename: row.get(3)?,
                nonce: row.get(4)?,
                ciphertext: Some(row.get(5)?),
                content_hash: row.get(6)?,
                size: row.get(7)?,
                updated_at: row.get(8)?,
                updated_by: row.get(9)?,
                deleted: row.get::<_, i64>(10)? != 0,
                seq: row.get::<_, i64>(11)? as u64,
            }),
            None => None,
        })
    }

    /// 客户端上报 (id, 内容哈希) 清单，返回服务端缺失或内容不一致的 id。
    pub fn images_needed(
        &self,
        user_id: &str,
        items: &[(String, String)],
    ) -> ServerResult<Vec<String>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT content_hash FROM images WHERE user_id = ?1 AND image_id = ?2 AND deleted = 0",
        )?;
        let mut needed = Vec::new();
        for (id, hash) in items {
            let existing: Option<String> = stmt
                .query_row(rusqlite::params![user_id, id], |row| row.get(0))
                .ok();
            match existing {
                Some(existing_hash) if existing_hash == *hash => {}
                _ => needed.push(id.clone()),
            }
        }
        Ok(needed)
    }

    pub fn image_count(&self, user_id: &str) -> ServerResult<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM images WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?)
    }

    pub fn image_bytes(&self, user_id: &str) -> ServerResult<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(ciphertext)), 0) FROM images WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?)
    }

    // -----------------------------------------------------------------------
    // 管理界面查询
    // -----------------------------------------------------------------------

    pub fn overview(&self) -> ServerResult<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let users: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        let legacy_users: i64 =
            conn.query_row("SELECT COUNT(*) FROM users_legacy", [], |row| row.get(0))?;
        let entities: i64 =
            conn.query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))?;
        let images: i64 = conn.query_row("SELECT COUNT(*) FROM images", [], |row| row.get(0))?;
        let entity_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(ciphertext)), 0) FROM entities",
            [],
            |row| row.get(0),
        )?;
        let image_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(ciphertext)), 0) FROM images",
            [],
            |row| row.get(0),
        )?;
        let tokens: i64 = {
            let now = crate::util::now_iso();
            conn.query_row(
                "SELECT COUNT(*) FROM tokens WHERE expires_at >= ?1",
                [now],
                |row| row.get(0),
            )?
        };
        Ok(serde_json::json!({
            "users": users,
            "legacyUsers": legacy_users,
            "entities": entities,
            "images": images,
            "storageBytes": entity_bytes + image_bytes,
            "entityBytes": entity_bytes,
            "imageBytes": image_bytes,
            "tokens": tokens,
        }))
    }

    pub fn list_users(&self) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT u.id, u.username, u.current_seq, u.created_at,
                    (SELECT COUNT(*) FROM entities e WHERE e.user_id = u.id) AS entity_count,
                    (SELECT COALESCE(SUM(LENGTH(e.ciphertext)), 0) FROM entities e WHERE e.user_id = u.id)
                      AS entity_bytes,
                    (SELECT COUNT(*) FROM images i WHERE i.user_id = u.id) AS image_count,
                    (SELECT COALESCE(SUM(LENGTH(i.ciphertext)), 0) FROM images i WHERE i.user_id = u.id)
                      AS image_bytes
             FROM users u ORDER BY u.created_at",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let entity_bytes: i64 = row.get(5)?;
            let image_bytes: i64 = row.get(7)?;
            out.push(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "username": row.get::<_, String>(1)?,
                "currentSeq": row.get::<_, i64>(2)?,
                "createdAt": row.get::<_, String>(3)?,
                "entityCount": row.get::<_, i64>(4)?,
                "entityBytes": entity_bytes,
                "imageCount": row.get::<_, i64>(6)?,
                "imageBytes": image_bytes,
                "storageBytes": entity_bytes + image_bytes,
            }));
        }
        Ok(out)
    }

    /// v0.5.0 及以前归档的账户（旧密钥派生含邮箱，已无法登录；数据仍在库里）。
    pub fn list_legacy_users(&self) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT u.id, u.username, u.email, u.current_seq, u.created_at, u.migrated_at,
                    (SELECT COUNT(*) FROM entities e WHERE e.user_id = u.id) AS entity_count,
                    (SELECT COUNT(*) FROM images i WHERE i.user_id = u.id) AS image_count,
                    (SELECT COALESCE(SUM(LENGTH(e.ciphertext)), 0) FROM entities e WHERE e.user_id = u.id)
                      + (SELECT COALESCE(SUM(LENGTH(i.ciphertext)), 0) FROM images i WHERE i.user_id = u.id)
                      AS storage
             FROM users_legacy u ORDER BY u.created_at",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "username": row.get::<_, String>(1)?,
                "email": row.get::<_, String>(2)?,
                "currentSeq": row.get::<_, i64>(3)?,
                "createdAt": row.get::<_, String>(4)?,
                "migratedAt": row.get::<_, String>(5)?,
                "entityCount": row.get::<_, i64>(6)?,
                "imageCount": row.get::<_, i64>(7)?,
                "storageBytes": row.get::<_, i64>(8)?,
            }));
        }
        Ok(out)
    }

    pub fn list_user_entities(&self, user_id: &str) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT entity_id, seq, LENGTH(ciphertext), LENGTH(nonce), content_hash FROM entities
             WHERE user_id = ?1 ORDER BY seq DESC LIMIT 1000",
        )?;
        let mut rows = stmt.query([user_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let hash: String = row.get(4)?;
            out.push(serde_json::json!({
                "entityId": row.get::<_, String>(0)?,
                "seq": row.get::<_, i64>(1)?,
                "ciphertextBytes": row.get::<_, i64>(2)?,
                "nonceBytes": row.get::<_, i64>(3)?,
                "hash": format!("{}…", &hash[..12.min(hash.len())]),
            }));
        }
        Ok(out)
    }

    /// 某用户的图片 blob 明细（密文本身不解密，只给元数据）。
    pub fn list_user_images(&self, user_id: &str) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT image_id, kind, node_id, filename, size, updated_at, updated_by, deleted, seq
             FROM images WHERE user_id = ?1 ORDER BY seq DESC LIMIT 1000",
        )?;
        let mut rows = stmt.query([user_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(serde_json::json!({
                "imageId": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "nodeId": row.get::<_, String>(2)?,
                "filename": row.get::<_, String>(3)?,
                "size": row.get::<_, i64>(4)?,
                "updatedAt": row.get::<_, String>(5)?,
                "updatedBy": row.get::<_, String>(6)?,
                "deleted": row.get::<_, i64>(7)? != 0,
                "seq": row.get::<_, i64>(8)?,
            }));
        }
        Ok(out)
    }

    pub fn list_tokens(&self) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let now = crate::util::now_iso();
        let mut stmt = conn.prepare(
            "SELECT t.token_hash, t.expires_at, u.username FROM tokens t
             JOIN users u ON u.id = t.user_id WHERE t.expires_at >= ?1 ORDER BY t.expires_at",
        )?;
        let mut rows = stmt.query([now])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let digest: String = row.get(0)?;
            out.push(serde_json::json!({
                "tokenDigest": format!("{}…", &digest[..12.min(digest.len())]),
                "expiresAt": row.get::<_, String>(1)?,
                "username": row.get::<_, String>(2)?,
            }));
        }
        Ok(out)
    }

    /// 库里到底存了些什么：每张表的行数、密文体积，以及 SQLite 页统计。
    pub fn db_stats(&self) -> ServerResult<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let count = |table: &str| -> ServerResult<i64> {
            // 表名来自下面的固定清单，不接受外部输入
            Ok(conn.query_row(
                &format!("SELECT COUNT(*) FROM {table}"),
                [],
                |row| row.get(0),
            )?)
        };
        let entity_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(ciphertext)), 0) FROM entities",
            [],
            |row| row.get(0),
        )?;
        let image_bytes: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(ciphertext)), 0) FROM images",
            [],
            |row| row.get(0),
        )?;
        let image_kinds: Vec<serde_json::Value> = {
            let mut stmt = conn.prepare(
                "SELECT kind, COUNT(*), COALESCE(SUM(LENGTH(ciphertext)), 0) FROM images
                 GROUP BY kind ORDER BY kind",
            )?;
            let mut rows = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                out.push(serde_json::json!({
                    "kind": row.get::<_, String>(0)?,
                    "count": row.get::<_, i64>(1)?,
                    "bytes": row.get::<_, i64>(2)?,
                }));
            }
            out
        };
        let pragma = |name: &str| -> ServerResult<i64> {
            // PRAGMA 名是下面的固定字面量，不接受外部输入
            Ok(conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))?)
        };
        let page_count = pragma("page_count")?;
        let page_size = pragma("page_size")?;
        let freelist = pragma("freelist_count")?;
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap_or_else(|_| "unknown".to_string());
        Ok(serde_json::json!({
            "tables": [
                { "table": "users", "rows": count("users")?, "bytes": 0 },
                { "table": "users_legacy", "rows": count("users_legacy")?, "bytes": 0 },
                { "table": "tokens", "rows": count("tokens")?, "bytes": 0 },
                { "table": "entities", "rows": count("entities")?, "bytes": entity_bytes },
                { "table": "images", "rows": count("images")?, "bytes": image_bytes },
            ],
            "imageKinds": image_kinds,
            "pageCount": page_count,
            "pageSize": page_size,
            "freelistCount": freelist,
            "allocatedBytes": page_count * page_size,
            "journalMode": journal_mode,
            "sqliteVersion": rusqlite::version(),
        }))
    }

    /// 删除用户及其全部数据（实体 + 图片 blob + token），返回清除的 (实体数, 图片数)。
    /// 归档在 users_legacy 里的旧账户也能删（同一套数据表）。
    pub fn delete_user(&self, user_id: &str) -> ServerResult<(usize, usize)> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let entities = tx.execute("DELETE FROM entities WHERE user_id = ?1", [user_id])?;
        let images = tx.execute("DELETE FROM images WHERE user_id = ?1", [user_id])?;
        tx.execute("DELETE FROM tokens WHERE user_id = ?1", [user_id])?;
        let users = tx.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
        let legacy = tx.execute("DELETE FROM users_legacy WHERE id = ?1", [user_id])?;
        tx.commit()?;
        if users + legacy == 0 {
            return Err(ServerError::not_found());
        }
        Ok((entities, images))
    }
}

fn bump_seq(tx: &rusqlite::Transaction<'_>, user_id: &str) -> ServerResult<u64> {
    tx.execute(
        "UPDATE users SET current_seq = current_seq + 1 WHERE id = ?1",
        [user_id],
    )?;
    let seq: i64 = tx.query_row(
        "SELECT current_seq FROM users WHERE id = ?1",
        [user_id],
        |row| row.get(0),
    )?;
    Ok(seq as u64)
}

pub fn entity_to_json(row: &EntityRow) -> serde_json::Value {
    json!({
        "id": row.entity_id,
        "seq": row.seq,
        "nonce": row.nonce,
        "ciphertext": row.ciphertext,
    })
}

/// 图片元数据（不含密文；密文走 `GET /api/v1/images/{id}` 的裸字节响应体）。
pub fn image_to_json(row: &ImageRow) -> serde_json::Value {
    json!({
        "id": row.image_id,
        "kind": row.kind,
        "nodeId": row.node_id,
        "filename": row.filename,
        "hash": row.content_hash,
        "size": row.size,
        "updatedAt": row.updated_at,
        "updatedBy": row.updated_by,
        "deleted": row.deleted,
        "seq": row.seq,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v0.5.0 及以前的库：users 表带 email 列、UNIQUE(username, email)。
    const LEGACY_SCHEMA: &str = "
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    auth_key TEXT NOT NULL,
    current_seq INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(username, email)
);
CREATE TABLE entities (
    user_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    nonce TEXT NOT NULL,
    ciphertext TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    seq INTEGER NOT NULL,
    PRIMARY KEY (user_id, entity_id)
);";

    #[test]
    fn legacy_accounts_are_archived_not_destroyed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.db");
        {
            // 造一个旧版库：一个账户 + 一条实体密文
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(LEGACY_SCHEMA).unwrap();
            conn.execute(
                "INSERT INTO users (id, username, email, auth_key, current_seq, created_at)
                 VALUES ('user-old', 'alice', 'a@x.y', 'aa', 7, '2026-01-01T00:00:00.000Z')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO entities (user_id, entity_id, nonce, ciphertext, content_hash, seq)
                 VALUES ('user-old', 'task-1', 'n', 'c', 'h', 7)",
                [],
            )
            .unwrap();
        }

        let db = Db::open(&path).unwrap();
        assert_eq!(db.migrate_legacy_accounts().unwrap(), 1, "旧账户应被归档");
        // 再跑一次是幂等的（新表已无 email 列）
        assert_eq!(db.migrate_legacy_accounts().unwrap(), 0);

        let legacy = db.list_legacy_users().unwrap();
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0]["username"], "alice");
        assert_eq!(legacy[0]["email"], "a@x.y");
        assert_eq!(legacy[0]["entityCount"], 1, "旧数据必须保留");

        // 用户名不再被死账户占着：同名可以重新注册
        assert!(db.find_user("alice").unwrap().is_none());
        let id = db.create_user("alice", "bb").unwrap();
        assert!(!id.is_empty());
        assert!(db.find_user("alice").unwrap().is_some());

        // 删遗留账户会连带清掉它的实体
        let (entities, images) = db.delete_user("user-old").unwrap();
        assert_eq!((entities, images), (1, 0));
        assert!(db.list_legacy_users().unwrap().is_empty());
    }

    #[test]
    fn username_uniqueness_is_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("data.db")).unwrap();
        db.create_user("bob", "k1").unwrap();
        assert!(db.create_user("bob", "k2").is_err(), "同名账户必须被拒");
        let row = db.find_user("bob").unwrap().unwrap();
        assert_eq!(row.auth_key, "k1");
    }

    #[test]
    fn put_reports_whether_content_actually_changed() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("data.db")).unwrap();
        let user = db.create_user("carol", "k").unwrap();
        let (seq, changed) = db
            .put_entity(&user, "task-1", 0, "n", "cipher", "hash-1")
            .unwrap();
        assert!(changed && seq == 1);
        // 幂等重放：同内容不推进版本、不算改动（持久化日志据此静默）
        let (seq2, changed2) = db
            .put_entity(&user, "task-1", 1, "n", "cipher", "hash-1")
            .unwrap();
        assert!(!changed2 && seq2 == 1);
        let (seq3, changed3) = db
            .put_entity(&user, "task-1", 1, "n2", "cipher2", "hash-2")
            .unwrap();
        assert!(changed3 && seq3 == 2);
        // base 不符 → 版本冲突
        assert!(db
            .put_entity(&user, "task-1", 1, "n", "c", "h")
            .is_err());
    }
}
