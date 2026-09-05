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
