//! 日志：stdout + `<data-dir>/log/server-YYYYMMDD.log` 双写，按日轮转保留最近 7 天。
//!
//! 两条通道刻意分开（v0.5.1）：
//! - [`Logger::log`] 关键操作 → stdout + **持久化** + 管理台「操作日志」环形缓冲；
//! - [`Logger::console`] 高频噪音（每请求访问行、周期性空同步、UDP 发现应答）→ **只进 stdout**。
//!
//! 自动同步最短 5 秒一轮，每轮至少两三个请求；全写文件的话一天就是十几万行、
//! 几十 MB 纯噪音，真正想查的「谁在什么时候改了什么」反而被冲走。

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;
use serde::Serialize;

/// 管理台「操作日志」里保留的条数（进程内，重启即空）
const RECENT_CAP: usize = 400;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub at: String,
    pub kind: String,
    pub message: String,
}

pub struct Logger {
    dir: PathBuf,
    inner: Option<std::sync::Arc<Mutex<File>>>,
    current_date: Option<String>,
    recent: VecDeque<LogLine>,
}

impl Logger {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            inner: None,
            current_date: None,
            recent: VecDeque::new(),
        }
    }

    pub fn disabled() -> Self {
        Self {
            dir: PathBuf::new(),
            inner: None,
            current_date: None,
            recent: VecDeque::new(),
        }
    }

    fn log_file_path(dir: &PathBuf, date: &str) -> PathBuf {
        dir.join(format!("server-{date}.log"))
    }

    fn rotate_if_needed(&mut self, date: &str) {
        if self.current_date.as_deref() == Some(date) && self.inner.is_some() {
            return;
        }
        let _ = fs::create_dir_all(&self.dir);
        let path = Self::log_file_path(&self.dir, date);
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                self.inner = Some(std::sync::Arc::new(Mutex::new(file)));
                self.current_date = Some(date.to_string());
                self.prune_old_logs();
            }
            Err(_) => {
                self.inner = None;
            }
        }
    }

    fn prune_old_logs(&self) {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        let mut logs: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("server-") && n.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();
        if logs.len() <= 7 {
            return;
        }
        logs.sort();
        let overflow = logs.len() - 7;
        for path in logs.iter().take(overflow) {
            let _ = fs::remove_file(path);
        }
    }

    fn emit(&mut self, kind: &str, message: &str, persist: bool) {
        let now = Local::now();
        let line = format!(
            "{} [{}] {}",
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            kind,
            message
        );
        println!("{line}");
        if !persist {
            return;
        }
        let date = now.format("%Y%m%d").to_string();
        self.rotate_if_needed(&date);
        if let Some(file) = &self.inner {
            if let Ok(mut handle) = file.lock() {
                let _ = writeln!(handle, "{line}");
            }
        }
        self.recent.push_back(LogLine {
            at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            kind: kind.to_string(),
            message: message.to_string(),
        });
        while self.recent.len() > RECENT_CAP {
            self.recent.pop_front();
        }
    }

    /// 关键操作：stdout + 文件 + 管理台环形缓冲。
    ///
    /// 只给「增删改」这类值得事后追查的事件用（注册、登录成败、实体/图片真正写入、
    /// 管理操作、启动与配置）。
    pub fn log(&mut self, kind: &str, message: &str) {
        self.emit(kind, message, true);
    }

    /// 高频噪音：只进 stdout，不落盘（每请求访问行、周期空同步、UDP 发现应答）。
    pub fn console(&mut self, kind: &str, message: &str) {
        self.emit(kind, message, false);
    }

    /// 管理台「操作日志」页的数据源（新 → 旧）。
    pub fn recent(&self) -> Vec<LogLine> {
        self.recent.iter().rev().cloned().collect()
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::disabled()
    }
}
