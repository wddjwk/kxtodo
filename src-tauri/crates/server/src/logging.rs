//! 请求/操作日志：stdout + `~/.local/share/kxtodo/server/log/server-YYYYMMDD.log` 双写。
//! 按日轮转，保留最近 7 天。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

#[derive(Clone)]
pub struct Logger {
    dir: PathBuf,
    inner: Option<std::sync::Arc<Mutex<File>>>,
    current_date: Option<String>,
}

impl Logger {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            inner: None,
            current_date: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            dir: PathBuf::new(),
            inner: None,
            current_date: None,
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

    /// 记一行日志：stdout + 文件。`kind` 用于区分事件类型（req/op/info）。
    pub fn log(&mut self, kind: &str, message: &str) {
        let now = Local::now();
        let date = now.format("%Y%m%d").to_string();
        let line = format!(
            "{} [{}] {}",
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            kind,
            message
        );
        println!("{line}");
        self.rotate_if_needed(&date);
        if let Some(file) = &self.inner {
            if let Ok(mut handle) = file.lock() {
                let _ = writeln!(handle, "{line}");
            }
        }
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::disabled()
    }
}
