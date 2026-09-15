//! Shared test harness: every test manages its own temporary data dir.

use std::path::PathBuf;

use serde_json::Value;
use tempfile::TempDir;
use kxtodo_core::cli::{run_cli, Routing};

pub struct TestEnv {
    pub dir: TempDir,
}

impl TestEnv {
    /// Fresh, empty data directory (v9 files created lazily by the first command).
    pub fn fresh() -> Self {
        Self {
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    pub fn write_file(&self, name: &str, value: &Value) {
        let path = self.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    pub fn read_file(&self, name: &str) -> Value {
        let path = self.path().join(name);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("无法读取 {}：{error}", path.display()));
        serde_json::from_str(&raw).unwrap_or_else(|error| panic!("{} 不是有效 JSON：{error}", name))
    }

    pub fn file_exists(&self, name: &str) -> bool {
        self.path().join(name).exists()
    }

    /// Run the CLI in-process against this data dir (standalone routing).
    pub fn run(&self, args: &[&str]) -> CliResult {
        let mut full: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        full.push("--data-dir".to_string());
        full.push(self.path().to_string_lossy().to_string());
        let output = run_cli(&full, &self.path(), Routing::Local);
        CliResult {
            code: output.code,
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }

    /// Run and require success; return the `data` payload.
    pub fn ok(&self, args: &[&str]) -> Value {
        let result = self.run(args);
        assert_eq!(
            result.code, 0,
            "命令 {:?} 失败：code={} stderr={}",
            args, result.code, result.stderr
        );
        result.envelope()["data"].clone()
    }

    /// Run and require a specific exit code; return the error object.
    pub fn err(&self, args: &[&str], code: i32) -> Value {
        let result = self.run(args);
        assert_eq!(
            result.code, code,
            "命令 {:?} 期望退出码 {}，实际 {}：stderr={}",
            args, code, result.code, result.stderr
        );
        result.stderr_envelope()["error"].clone()
    }
}

pub struct CliResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliResult {
    pub fn envelope(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("stdout 不是 JSON：{error}\n原始输出：{}", self.stdout))
    }

    pub fn stderr_envelope(&self) -> Value {
        serde_json::from_str(&self.stderr)
            .unwrap_or_else(|error| panic!("stderr 不是 JSON：{error}\n原始输出：{}", self.stderr))
    }
}

