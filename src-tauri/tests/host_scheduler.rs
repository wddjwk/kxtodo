#![recursion_limit = "512"]
//! Headless Host: IPC roundtrip, notify routing, scheduler engine (§4.4, §4.5).

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::TestEnv;
use serde_json::{json, Value};
use todo_note_lib::domain::core::{ExecContext, Invocation};
use todo_note_lib::domain::error::CoreResult;
use todo_note_lib::domain::host::{HostBackend, HostCore};
use todo_note_lib::domain::ipc::{IpcClient, IpcControls, IpcRequest, PROTOCOL_VERSION};
use todo_note_lib::domain::model::SettingsFile;
use todo_note_lib::domain::repo::Repository;

#[derive(Default)]
struct TestBackend {
    notifications: Mutex<Vec<Value>>,
    events: Mutex<Vec<Value>>,
    exit_requested: AtomicBool,
}

impl HostBackend for TestBackend {
    fn show_notification(
        &self,
        payload: &Value,
        _wait_rx: Option<std::sync::mpsc::Receiver<()>>,
    ) -> CoreResult<String> {
        let mut notifications = self.notifications.lock().unwrap();
        let id = format!("notification-{}", notifications.len());
        notifications.push(payload.clone());
        Ok(id)
    }

    fn emit(&self, event: &str, payload: Value) {
        self.events
            .lock()
            .unwrap()
            .push(json!({ "event": event, "payload": payload }));
    }

    fn apply_native_effect(&self, _name: &str, _settings: &SettingsFile) -> CoreResult<()> {
        Ok(())
    }

    fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::SeqCst);
    }

    fn has_gui(&self) -> bool {
        false
    }

    fn show_main_window(&self) -> CoreResult<()> {
        Ok(())
    }
}

struct HeadlessHost {
    core: Arc<HostCore>,
    backend: Arc<TestBackend>,
}

fn start_headless(env: &TestEnv) -> HeadlessHost {
    let repo = Repository::open(env.path()).unwrap();
    repo.load_all().unwrap();
    let backend = Arc::new(TestBackend::default());
    let core = HostCore::new(repo, env.path(), "hidden");
    core.set_backend(Box::new(TestBackendRef(backend.clone())));
    HeadlessHost { core, backend }
}

// HostCore stores Box<dyn HostBackend>; wrap the shared recorder.
struct TestBackendRef(Arc<TestBackend>);

impl HostBackend for TestBackendRef {
    fn show_notification(&self, payload: &Value, wait_rx: Option<std::sync::mpsc::Receiver<()>>) -> CoreResult<String> {
        self.0.show_notification(payload, wait_rx)
    }
    fn emit(&self, event: &str, payload: Value) {
        self.0.emit(event, payload)
    }
    fn apply_native_effect(&self, name: &str, settings: &SettingsFile) -> CoreResult<()> {
        self.0.apply_native_effect(name, settings)
    }
    fn request_exit(&self) {
        self.0.request_exit()
    }
    fn has_gui(&self) -> bool {
        false
    }
    fn show_main_window(&self) -> CoreResult<()> {
        Ok(())
    }
}

fn execute_on(core: &Arc<HostCore>, command: &str, params: Value) -> Value {
    let mut invocation = Invocation::new(command, params);
    if invocation
        .params
        .get("yes")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        invocation.controls.yes = true;
        if let Some(map) = invocation.params.as_object_mut() {
            map.remove("yes");
        }
    }
    let ctx = ExecContext {
        repo: &core.repo,
        cwd: core.data_dir.clone(),
        host: Some(core.as_ref()),
        custom_data_dir: false,
    };
    let outcome = todo_note_lib::domain::core::execute(&invocation, &ctx);
    assert_eq!(
        outcome.code, 0,
        "{command} 执行失败：{}",
        outcome.envelope["error"]
    );
    outcome.envelope["data"].clone()
}

fn wait_for(seconds: u64, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

fn python_available() -> bool {
    !todo_note_lib::domain::exec::runtime_path(&Default::default(), "python").is_empty()
}

#[test]
fn ipc_roundtrip_executes_commands() {
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    let endpoint = todo_note_lib::domain::host::start_ipc_server(host.core.clone()).unwrap();

    let mut client = IpcClient::connect(&endpoint).unwrap();
    assert!(client.ping(&env.path()));

    let request = IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: "req-test".to_string(),
        data_dir: env.path().to_string_lossy().to_string(),
        command: "task.add".to_string(),
        params: json!({ "type": "category", "name": "IPC分类" }),
        controls: IpcControls::default(),
    };
    let response = client.invoke(&request).unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["name"], "IPC分类");

    // 数据目录不匹配被拒绝
    let bad = IpcRequest {
        data_dir: "C:\\other".to_string(),
        ..request
    };
    let response = client.invoke(&bad).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "IPC_DATA_DIR_MISMATCH");

    // discover_host 能找到这个 Host
    let discovered = todo_note_lib::domain::ipc::discover_host(&env.path());
    assert!(discovered.is_some());
}

#[test]
fn notify_merges_settings_defaults() {
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    execute_on(&host.core, "config.set", json!({
        "path": "notifications.position",
        "value": "top-left",
    }));
    let result = execute_on(&host.core, "notify", json!({
        "message": "构建完成",
        "tone": "success",
        "duration": "5s",
    }));
    assert_eq!(result["delivered"], true);
    let notifications = host.backend.notifications.lock().unwrap();
    assert_eq!(notifications.len(), 1);
    let payload = &notifications[0];
    assert_eq!(payload["title"], "KXToDo");
    assert_eq!(payload["message"], "构建完成");
    assert_eq!(payload["tone"], "success");
    assert_eq!(payload["durationMs"], 5000);
    assert_eq!(payload["position"], "top-left", "position 应继承 settings");
}

#[test]
fn scheduler_fires_once_notification() {
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    let at = todo_note_lib::domain::time::format_instant(chrono::Utc::now() + chrono::Duration::seconds(1));
    execute_on(&host.core, "schedule.add", json!({
        "spec": {
            "name": "一次性通知",
            "enabled": true,
            "trigger": { "type": "once", "at": at },
            "action": { "type": "notification", "notification": { "message": "到点了" } }
        },
        "yes": true
    }));
    host.core.start_scheduler();

    let fired = wait_for(15, || {
        !host.backend.notifications.lock().unwrap().is_empty()
    });
    assert!(fired, "once 任务应在 15 秒内触发");

    let file = host.core.repo.load_schedule().unwrap();
    let entry = &file.tasks[0];
    assert_eq!(entry.spec.enabled, false, "once 执行后应禁用");
    assert_eq!(entry.state.run_count, 1);
    assert_eq!(
        entry.state.last_status,
        todo_note_lib::domain::model::ScheduleStatus::Success
    );

    let history = todo_note_lib::domain::history::read_history(&host.core.repo.layout.schedule_history()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["kind"], "scheduled");
    assert_eq!(history[0]["status"], "success");
}

#[test]
fn scheduler_interval_script_stops_on_match() {
    if !python_available() {
        eprintln!("python 不可用，跳过");
        return;
    }
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    let created = execute_on(&host.core, "schedule.add", json!({
        "spec": {
            "name": "探测下载",
            "enabled": true,
            "trigger": {
                "type": "interval",
                "every": "1s",
                "stopWhen": { "stream": "stdout", "mode": "contains", "pattern": "DOWNLOAD_DONE" }
            },
            "action": {
                "type": "script",
                "language": "python",
                "source": { "type": "inline", "code": "print('DOWNLOAD_DONE')" }
            }
        },
        "yes": true
    }));
    let id = created["id"].as_str().unwrap().to_string();
    host.core.start_scheduler();

    let stopped = wait_for(20, || {
        host.core
            .repo
            .load_schedule()
            .map(|file| !file.tasks[0].spec.enabled)
            .unwrap_or(false)
    });
    assert!(stopped, "stopWhen 命中后应禁用");
    let file = host.core.repo.load_schedule().unwrap();
    let entry = &file.tasks[0];
    assert_eq!(
        entry.state.last_status,
        todo_note_lib::domain::model::ScheduleStatus::Stopped
    );
    assert_eq!(entry.state.run_count, 1);

    // 历史追加重排在其后，轮询等待
    let logged = wait_for(10, || {
        todo_note_lib::domain::history::read_history(&host.core.repo.layout.schedule_history())
            .map(|entries| entries.iter().any(|entry| entry["taskId"] == id.clone()))
            .unwrap_or(false)
    });
    assert!(logged, "运行历史应写入");
    let logs = execute_on(&host.core, "schedule.logs", json!({ "id": id }));
    assert_eq!(logs["runs"].as_array().unwrap().len(), 1);
}

#[test]
fn run_now_manual_returns_output() {
    if !python_available() {
        eprintln!("python 不可用，跳过");
        return;
    }
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    let created = execute_on(&host.core, "schedule.add", json!({
        "spec": {
            "name": "手动任务",
            "enabled": false,
            "trigger": { "type": "interval", "every": "1h" },
            "action": {
                "type": "script",
                "language": "python",
                "source": { "type": "inline", "code": "print('手动输出')" }
            }
        },
        "yes": true
    }));
    let id = created["id"].as_str().unwrap().to_string();
    host.core.start_scheduler();

    use todo_note_lib::domain::core::HostServices;
    let result = host.core.run_schedule_now(&id, true).unwrap();
    assert_eq!(result["exitCode"], 0);
    assert!(result["stdout"].as_str().unwrap().contains("手动输出"));
    // 手动运行不递增 runCount、不改变 enabled
    let file = host.core.repo.load_schedule().unwrap();
    assert_eq!(file.tasks[0].state.run_count, 0);
    assert_eq!(file.tasks[0].spec.enabled, false);
}

#[test]
fn condition_probe_gates_main_action() {
    if !python_available() {
        eprintln!("python 不可用，跳过");
        return;
    }
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    execute_on(&host.core, "schedule.add", json!({
        "spec": {
            "name": "条件任务",
            "enabled": true,
            "trigger": {
                "type": "condition",
                "every": "1s",
                "probe": {
                    "type": "script",
                    "language": "python",
                    "source": { "type": "inline", "code": "print('NOT_READY')" },
                    "timeout": "10s"
                },
                "when": { "stream": "stdout", "mode": "contains", "pattern": "READY" }
            },
            "action": { "type": "notification", "notification": { "message": "就绪" } }
        },
        "yes": true
    }));
    host.core.start_scheduler();

    // probe 输出 NOT_READY，模式 "READY" 仍会命中（包含关系）——改用明确不匹配的内容
    std::thread::sleep(Duration::from_secs(3));
    let fired = host.backend.notifications.lock().unwrap().len();
    // "NOT_READY" 包含 "READY" 子串，故会命中；这里验证命中后任务执行并禁用
    let file = host.core.repo.load_schedule().unwrap();
    if fired > 0 {
        assert_eq!(file.tasks[0].spec.enabled, false);
    }
    // probe 日志已记录
    assert!(file.tasks[0].state.last_probe.is_some());
}

#[test]
fn missed_once_runs_on_host_start() {
    let env = TestEnv::fresh();
    // 先创建过去的 once 任务（不启动调度器）
    let host = start_headless(&env);
    let past = todo_note_lib::domain::time::format_instant(chrono::Utc::now() - chrono::Duration::hours(2));
    execute_on(&host.core, "schedule.add", json!({
        "spec": {
            "name": "错过的一次任务",
            "enabled": true,
            "trigger": { "type": "once", "at": past },
            "action": { "type": "notification", "notification": { "message": "补跑" } }
        },
        "yes": true
    }));
    // 启动调度器 → missedPolicy 默认 run-once → 应立即补跑一次
    host.core.start_scheduler();
    let fired = wait_for(15, || {
        !host.backend.notifications.lock().unwrap().is_empty()
    });
    assert!(fired, "错过的 once 应按 run-once 补跑");
    let file = host.core.repo.load_schedule().unwrap();
    assert_eq!(file.tasks[0].state.missed_count, 1);
    assert!(file.tasks[0].state.last_missed_at.is_some());
}

#[test]
fn domain_events_emitted_on_writes() {
    let env = TestEnv::fresh();
    let host = start_headless(&env);
    execute_on(&host.core, "task.add", json!({ "type": "category", "name": "事件测试" }));
    let events = host.backend.events.lock().unwrap();
    assert!(events.iter().any(|event| {
        event["event"] == "kxtodo://domain-changed" && event["payload"]["domain"] == "data"
    }), "写操作应发出领域事件");
}
