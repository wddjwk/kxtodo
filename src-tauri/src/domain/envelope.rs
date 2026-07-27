//! Output envelope (§3.2 输出协议).

use serde_json::{json, Value};

use crate::domain::error::CoreError;
use crate::domain::ids::request_id;
use crate::domain::repo::Domain;

#[derive(Debug, Clone)]
pub struct Meta {
    pub request_id: String,
    pub count: Option<usize>,
    pub next_cursor: Option<String>,
    pub revision_domain: Option<Domain>,
    pub revision: Option<u64>,
    pub replayed: bool,
    pub data_dir: Option<String>,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            request_id: request_id(),
            count: None,
            next_cursor: None,
            revision_domain: None,
            revision: None,
            replayed: false,
            data_dir: None,
        }
    }
}

impl Meta {
    pub fn to_json(&self) -> Value {
        let mut meta = json!({ "requestId": self.request_id });
        if let Some(count) = self.count {
            meta["count"] = json!(count);
        }
        if self.next_cursor.is_some() || self.count.is_some() {
            meta["nextCursor"] = json!(self.next_cursor);
        }
        if let Some(domain) = self.revision_domain {
            meta["revisionDomain"] = json!(domain.as_str());
        }
        if let Some(revision) = self.revision {
            meta["revision"] = json!(revision);
        }
        if self.replayed {
            meta["replayed"] = json!(true);
        }
        if let Some(dir) = &self.data_dir {
            meta["dataDir"] = json!(dir);
        }
        meta
    }
}

pub fn success(command: &str, data: Value, meta: Meta) -> Value {
    json!({
        "ok": true,
        "command": command,
        "data": data,
        "meta": meta.to_json(),
    })
}

pub fn failure(command: &str, error: &CoreError, meta: Meta) -> Value {
    json!({
        "ok": false,
        "command": command,
        "error": error.to_json(),
        "meta": meta.to_json(),
    })
}
