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
    pub email: String,
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
    username TEXT NOT NULL,
    email TEXT NOT NULL,
    auth_key TEXT NOT NULL,
    current_seq INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(username, email)
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

    pub fn create_user(&self, username: &str, email: &str, auth_key: &str) -> ServerResult<String> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE username = ?1 AND email = ?2",
                [username, email],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)?;
        if exists {
            return Err(ServerError::account_exists());
        }
        let id = format!("user-{}", crate::util::random_hex(8));
        conn.execute(
            "INSERT INTO users (id, username, email, auth_key, current_seq, created_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            rusqlite::params![id, username, email, auth_key, crate::util::now_iso()],
        )?;
        Ok(id)
    }

    pub fn find_user(&self, username: &str, email: &str) -> ServerResult<Option<UserRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, email, auth_key, current_seq FROM users
             WHERE username = ?1 AND email = ?2",
        )?;
        let mut rows = stmt.query([username, email])?;
        Ok(match rows.next()? {
            Some(row) => Some(UserRow {
                id: row.get(0)?,
                username: row.get(1)?,
                email: row.get(2)?,
                auth_key: row.get(3)?,
                current_seq: row.get::<_, i64>(4)? as u64,
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
            "SELECT u.id, u.username, u.email, u.auth_key, u.current_seq
             FROM tokens t JOIN users u ON u.id = t.user_id
             WHERE t.token_hash = ?1 AND t.expires_at >= ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![token_hash, now.as_str()])?;
        Ok(match rows.next()? {
            Some(row) => Some(UserRow {
                id: row.get(0)?,
                username: row.get(1)?,
                email: row.get(2)?,
                auth_key: row.get(3)?,
                current_seq: row.get::<_, i64>(4)? as u64,
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
    /// 内容 hash 相同则为幂等重放：返回当前 seq，不推进版本。
    pub fn put_entity(
        &self,
        user_id: &str,
        entity_id: &str,
        base: u64,
        nonce: &str,
        ciphertext: &str,
        content_hash: &str,
    ) -> ServerResult<u64> {
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
                    return Ok(seq);
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
                Ok(new_seq)
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
                Ok(new_seq)
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

    /// 写入图片：内容哈希相同即幂等重放（不推进版本），否则 upsert 并推进 seq。
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
    ) -> ServerResult<u64> {
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
                return Ok(seq as u64);
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
        Ok(new_seq)
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
            "entities": entities,
            "images": images,
            "storageBytes": entity_bytes + image_bytes,
            "imageBytes": image_bytes,
            "tokens": tokens,
        }))
    }

    pub fn list_users(&self) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT u.id, u.username, u.email, u.current_seq, u.created_at,
                    (SELECT COUNT(*) FROM entities e WHERE e.user_id = u.id) AS entity_count,
                    (SELECT COALESCE(SUM(LENGTH(e.ciphertext)), 0) FROM entities e WHERE e.user_id = u.id)
                      + (SELECT COALESCE(SUM(LENGTH(i.ciphertext)), 0) FROM images i WHERE i.user_id = u.id) AS storage,
                    (SELECT COUNT(*) FROM images i WHERE i.user_id = u.id) AS image_count
             FROM users u ORDER BY u.created_at",
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
                "entityCount": row.get::<_, i64>(5)?,
                "storageBytes": row.get::<_, i64>(6)?,
                "imageCount": row.get::<_, i64>(7)?,
            }));
        }
        Ok(out)
    }

    pub fn list_user_entities(&self, user_id: &str) -> ServerResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT entity_id, seq, LENGTH(ciphertext) FROM entities
             WHERE user_id = ?1 ORDER BY seq DESC LIMIT 500",
        )?;
        let mut rows = stmt.query([user_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let seq: i64 = row.get(1)?;
            out.push(serde_json::json!({
                "entityId": row.get::<_, String>(0)?,
                "seq": seq,
                "ciphertextBytes": row.get::<_, i64>(2)?,
                "updatedAt": format!("seq#{seq}"),
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

    /// 删除用户及其全部数据（实体 + 图片 blob），返回清除的实体数。
    pub fn delete_user(&self, user_id: &str) -> ServerResult<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let removed = tx.execute("DELETE FROM entities WHERE user_id = ?1", [user_id])?;
        tx.execute("DELETE FROM images WHERE user_id = ?1", [user_id])?;
        tx.execute("DELETE FROM tokens WHERE user_id = ?1", [user_id])?;
        let users = tx.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
        tx.commit()?;
        if users == 0 {
            return Err(ServerError::not_found());
        }
        Ok(removed)
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
