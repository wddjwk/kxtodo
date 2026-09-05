//! 进程内运行指标（不落盘）：给管理台展示「本次运行」的实时活动。
//!
//! 只统计不持久化——重启即归零，管理台会明确标注「本次运行」。
//! 真正的历史痕迹在持久化日志里（[`crate::logging`]）。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

/// 每分钟请求数的保留窗口（60 分钟，够画一条活动曲线）
const BUCKET_CAP: usize = 60;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivity {
    pub user_id: String,
    pub username: String,
    pub last_at: String,
    pub last_ip: String,
    pub requests: u64,
    pub logins: u64,
    pub pulls: u64,
    pub pushes: u64,
    pub image_pulls: u64,
    pub image_pushes: u64,
}

#[derive(Debug, Default)]
pub struct Metrics {
    pub started_at: String,
    requests: AtomicU64,
    writes: AtomicU64,
    failures: AtomicU64,
    /// (unix 分钟, 请求数)
    buckets: Mutex<VecDeque<(u64, u64)>>,
    routes: Mutex<HashMap<String, u64>>,
    users: Mutex<HashMap<String, UserActivity>>,
}

/// 管理台关心的用户级事件类型。
#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    Login,
    Pull,
    Push,
    ImagePull,
    ImagePush,
}

impl Metrics {
    pub fn new(started_at: String) -> Self {
        Self {
            started_at,
            ..Default::default()
        }
    }

    /// 每个 HTTP 请求都会走一次（middleware）。
    pub fn hit(&self, method: &str, path: &str, ok: bool) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if !ok {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
        let minute = unix_minute();
        if let Ok(mut buckets) = self.buckets.lock() {
            match buckets.back_mut() {
                Some((bucket_minute, count)) if *bucket_minute == minute => *count += 1,
                _ => {
                    buckets.push_back((minute, 1));
                    while buckets.len() > BUCKET_CAP {
                        buckets.pop_front();
                    }
                }
            }
        }
        // 路由归一化：把 /api/v1/entities/<id> 这类带 ID 的路径折叠成一条，
        // 否则计数表会被成千上万个一次性 key 撑爆。
        let route = format!("{method} {}", normalize_route(path));
        if let Ok(mut routes) = self.routes.lock() {
            *routes.entry(route).or_insert(0) += 1;
        }
    }

    /// 一次真实写入（实体或图片密文落库），幂等重放不计。
    pub fn wrote(&self) {
        self.writes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn user_event(&self, user_id: &str, username: &str, event: UserEvent, ip: &str) {
        let Ok(mut users) = self.users.lock() else {
            return;
        };
        let entry = users.entry(user_id.to_string()).or_insert_with(|| UserActivity {
            user_id: user_id.to_string(),
            username: username.to_string(),
            ..Default::default()
        });
        entry.username = username.to_string();
        entry.last_at = crate::util::now_iso();
        if !ip.is_empty() {
            entry.last_ip = ip.to_string();
        }
        entry.requests += 1;
        match event {
            UserEvent::Login => entry.logins += 1,
            UserEvent::Pull => entry.pulls += 1,
            UserEvent::Push => entry.pushes += 1,
            UserEvent::ImagePull => entry.image_pulls += 1,
            UserEvent::ImagePush => entry.image_pushes += 1,
        }
    }

    pub fn forget_user(&self, user_id: &str) {
        if let Ok(mut users) = self.users.lock() {
            users.remove(user_id);
        }
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let buckets: Vec<serde_json::Value> = self
            .buckets
            .lock()
            .map(|buckets| {
                buckets
                    .iter()
                    .map(|(minute, count)| {
                        serde_json::json!({
                            "minute": minute,
                            "label": minute_label(*minute),
                            "count": count,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut routes: Vec<(String, u64)> = self
            .routes
            .lock()
            .map(|routes| routes.iter().map(|(k, v)| (k.clone(), *v)).collect())
            .unwrap_or_default();
        routes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut users: Vec<UserActivity> = self
            .users
            .lock()
            .map(|users| users.values().cloned().collect())
            .unwrap_or_default();
        users.sort_by(|a, b| b.last_at.cmp(&a.last_at));
        serde_json::json!({
            "startedAt": self.started_at,
            "uptimeSeconds": uptime_seconds(&self.started_at),
            "requests": self.requests.load(Ordering::Relaxed),
            "writes": self.writes.load(Ordering::Relaxed),
            "failures": self.failures.load(Ordering::Relaxed),
            "buckets": buckets,
            "routes": routes.iter().take(24).map(|(route, count)| {
                serde_json::json!({ "route": route, "count": count })
            }).collect::<Vec<_>>(),
            "users": users,
        })
    }
}

fn unix_minute() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() / 60)
        .unwrap_or(0)
}

fn minute_label(minute: u64) -> String {
    chrono::DateTime::from_timestamp((minute * 60) as i64, 0)
        .map(|time| time.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

fn uptime_seconds(started_at: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(started_at)
        .map(|start| {
            (chrono::Utc::now() - start.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0) as u64
        })
        .unwrap_or(0)
}

/// `/api/v1/entities/abc123` → `/api/v1/entities/{id}`：路径里的 ID 段折叠掉。
fn normalize_route(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let mut out = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let is_id = index > 0
            && !segment.is_empty()
            && !matches!(*segment, "v1" | "admin" | "api" | "changes" | "check" | "login"
                | "login-challenge" | "logout" | "register" | "me" | "healthz" | "users"
                | "entities" | "images" | "tokens" | "overview" | "db" | "ops" | "metrics"
                | "activity");
        out.push(if is_id { "{id}" } else { segment });
    }
    out.join("/")
}
