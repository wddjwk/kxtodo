//! Shared test harness: every test manages its own temporary data dir.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
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

    /// Data dir seeded with legacy v8 files (migration exercised on first use).
    pub fn with_v8_data(data: Value, settings: Value, tasks: Value) -> Self {
        let env = Self::fresh();
        env.write_file("data.json", &data);
        env.write_file("settings.json", &settings);
        env.write_file("tasks.json", &tasks);
        env
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

/// Minimal v8-shaped data.json content for migration tests.
pub fn v8_data() -> Value {
    json!({
        "schemaVersion": 4,
        "selectedNodeId": "entry-aaa",
        "nodes": [
            { "id": "my-day", "kind": "system", "name": "我的一天", "icon": "sun", "parentId": null, "createdAt": "2025-01-01T00:00:00.000Z" },
            { "id": "planned", "kind": "system", "name": "计划内", "icon": "calendar", "parentId": null, "createdAt": "2025-01-01T00:00:00.000Z" },
            { "id": "important", "kind": "system", "name": "收藏", "icon": "star", "parentId": null, "createdAt": "2025-01-01T00:00:00.000Z" },
            { "id": "scheduled", "kind": "system", "name": "定时任务", "icon": "clock", "parentId": null, "createdAt": "2025-01-01T00:00:00.000Z" },
            { "id": "category-aaa", "kind": "category", "name": "2026", "icon": "rocket", "parentId": null, "collapsed": false, "createdAt": "2025-01-02T00:00:00.000Z", "customField": "keep-me" },
            { "id": "entry-aaa", "kind": "entry", "name": "收集箱", "icon": "inbox", "parentId": null, "collapsed": false, "createdAt": "2025-01-03T00:00:00.000Z" },
            { "id": "entry-bbb", "kind": "entry", "name": "0727-0731", "icon": "notebook", "parentId": "category-aaa", "createdAt": "2025-01-04T00:00:00.000Z" }
        ],
        "tasks": [
            {
                "id": "task-aaa", "nodeId": "entry-bbb", "markdown": "完成 XXX 需求",
                "completed": false, "important": true, "myDay": false,
                "dueDate": "2026-07-31",
                "tags": [{ "id": "tag-aaa", "color": "blue", "text": "需求" }],
                "emojis": ["🔥"], "expanded": false, "editing": false,
                "createdAt": "2025-01-05T00:00:00.000Z", "updatedAt": "2025-01-06T00:00:00.000Z"
            }
        ],
        "backgrounds": {
            "entry-aaa": { "color": "#f4f1ea", "imageOpacity": 0.28 },
            "entry-bbb": { "color": "#dfe8df", "image": "img:bg-1.png", "imageOpacity": 0.2 }
        }
    })
}

pub fn v8_settings() -> Value {
    json!({
        "profile": { "name": "旧版用户", "email": "old@example.com", "avatar": "" },
        "appearance": {
            "uiScale": 0.72,
            "uiFontSize": 18,
            "themePresets": [{ "name": "雾瓷", "color": "#f4f1ea" }],
            "uiColors": { "entry-bbb": "#dfe8df" }
        },
        "display": { "closeToTray": false, "launchAtStartup": false, "notificationDurationMs": 4200 },
        "globalShortcut": "Ctrl+Shift+K"
    })
}

/// Legacy scheduled task shapes taken from real v8 data.
pub fn v8_tasks() -> Value {
    json!({
        "runtimes": {
            "python": "C:\\Python\\python.exe",
            "node": "",
            "pwsh": "",
            "bash": "",
            "make": ""
        },
        "tasks": [
            {
                "id": "schedule-0001",
                "name": "定时任务 1",
                "enabled": false,
                "expanded": true,
                "editing": false,
                "createdAt": "2026-07-06T14:17:26.743Z",
                "updatedAt": "2026-07-06T17:11:00.270Z",
                "runCount": 5,
                "lastRunAt": "2026-07-06T17:11:00.270Z",
                "nextRunAt": "2026-07-06T17:11:05.270Z",
                "lastStatus": "stopped",
                "lastExitCode": 0,
                "lastStdout": "hello",
                "lastStderr": "",
                "trigger": {
                    "type": "interval",
                    "runAt": "",
                    "everySeconds": 3600,
                    "repeatCount": 10,
                    "cron": "0 9 * * *",
                    "stopCondition": { "enabled": true, "mode": "contains", "pattern": "DOWNLOAD_DONE" },
                    "probeAction": {},
                    "probeCondition": { "enabled": false, "mode": "contains", "pattern": "" }
                },
                "action": {
                    "type": "script",
                    "scriptMode": "path",
                    "language": "python",
                    "interpreter": "",
                    "filePath": "D:\\scripts\\download.py",
                    "code": "print(\"残留 code 不得迁移\")",
                    "executablePath": "",
                    "arguments": "--fast \"quoted arg\"",
                    "workingDirectory": "",
                    "notification": { "title": "KXToDo", "message": "定时任务已触发", "durationMs": 1231, "tone": "info" },
                    "notifyOnComplete": true,
                    "completionNotification": { "title": "KXToDo", "message": "完成：{stdout}", "durationMs": 5200, "tone": "success" },
                    "stdoutNotification": {
                        "enabled": true,
                        "condition": { "enabled": true, "mode": "contains", "pattern": "READY" },
                        "notification": { "title": "KXToDo", "message": "匹配：{stdout}", "durationMs": 5200, "tone": "info" }
                    }
                }
            },
            {
                "id": "schedule-0002",
                "name": "自定义解释器任务",
                "enabled": true,
                "createdAt": "2026-07-06T14:17:26.743Z",
                "updatedAt": "2026-07-06T17:11:00.270Z",
                "runCount": 0,
                "lastStatus": "running",
                "trigger": {
                    "type": "once",
                    "runAt": "2026-07-31T17:30",
                    "everySeconds": 300,
                    "repeatCount": 1,
                    "cron": "",
                    "stopCondition": { "enabled": false, "mode": "contains", "pattern": "" },
                    "probeAction": {},
                    "probeCondition": { "enabled": false, "mode": "contains", "pattern": "" }
                },
                "action": {
                    "type": "script",
                    "scriptMode": "inline",
                    "language": "custom",
                    "interpreter": "D:\\tools\\rscript.exe",
                    "filePath": "",
                    "code": "print(1)",
                    "executablePath": "",
                    "arguments": "-v",
                    "workingDirectory": "",
                    "notification": { "title": "KXToDo", "message": "x", "durationMs": 3000, "tone": "info" },
                    "notifyOnComplete": false,
                    "completionNotification": {},
                    "stdoutNotification": { "enabled": false, "condition": {}, "notification": {} }
                }
            },
            {
                "id": "schedule-0003",
                "name": "条件探测任务",
                "enabled": true,
                "createdAt": "2026-07-06T14:17:26.743Z",
                "updatedAt": "2026-07-06T14:17:26.743Z",
                "runCount": 0,
                "lastStatus": "idle",
                "trigger": {
                    "type": "condition",
                    "runAt": "",
                    "everySeconds": 60,
                    "repeatCount": 0,
                    "cron": "",
                    "stopCondition": { "enabled": false, "mode": "contains", "pattern": "" },
                    "probeAction": {
                        "type": "script",
                        "scriptMode": "inline",
                        "language": "python",
                        "interpreter": "",
                        "filePath": "",
                        "code": "print('READY')",
                        "executablePath": "",
                        "arguments": "",
                        "workingDirectory": "",
                        "notifyOnComplete": true,
                        "completionNotification": { "title": "不该迁移", "message": "probe 通知应被移除", "durationMs": 3000, "tone": "info" }
                    },
                    "probeCondition": { "enabled": true, "mode": "contains", "pattern": "READY" }
                },
                "action": {
                    "type": "notification",
                    "scriptMode": "inline",
                    "language": "python",
                    "interpreter": "",
                    "filePath": "",
                    "code": "",
                    "executablePath": "",
                    "arguments": "",
                    "workingDirectory": "",
                    "notification": { "title": "KXToDo", "message": "资源就绪", "durationMs": 5200, "tone": "success", "position": "top-right" },
                    "notifyOnComplete": false,
                    "completionNotification": {},
                    "stdoutNotification": { "enabled": false, "condition": {}, "notification": {} }
                }
            }
        ]
    })
}
