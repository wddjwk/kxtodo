//! 任务提醒（v0.8.3）：规格解析、写入事务、执行引擎的端到端行为。
//!
//! 提醒不是定时任务：它不建 ScheduleEntry，只在本机 runtime 记发送台账。
//! 这里钉住的是三件「错了用户立刻有感」的事：
//! 1. 规格语法（`due-60` / `+1h` / RFC3339）与「截止前必须有分钟时刻」；
//! 2. 日期 / 时刻 / 提醒是一个事务——任何一样不合法，任务一点都不能动；
//! 3. 引擎：到点只响一次、重启不补发、时钟跳变不追债。

mod common;

use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use common::TestEnv;
use serde_json::{json, Value};
use kxtodo_core::error::CoreResult;
use kxtodo_core::host::{HostBackend, HostCore};
use kxtodo_core::model::SettingsFile;
use kxtodo_core::reminders;
use kxtodo_core::repo::Repository;

#[derive(Default)]
struct RecordingBackend {
    notifications: Mutex<Vec<Value>>,
}

impl HostBackend for RecordingBackend {
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
    fn emit(&self, _event: &str, _payload: Value) {}
    fn apply_native_effect(&self, _name: &str, _settings: &SettingsFile) -> CoreResult<()> {
        Ok(())
    }
    fn request_exit(&self) {}
    fn has_gui(&self) -> bool {
        false
    }
    fn show_main_window(&self) -> CoreResult<()> {
        Ok(())
    }
}

struct Headless {
    core: Arc<HostCore>,
    backend: Arc<RecordingBackend>,
}

fn headless(env: &TestEnv) -> Headless {
    let repo = Repository::open(env.path()).unwrap();
    repo.load_all().unwrap();
    let backend = Arc::new(RecordingBackend::default());
    let core = HostCore::new(repo, env.path(), "hidden", true);
    core.set_backend(Box::new(BackendRef(backend.clone())));
    Headless { core, backend }
}

struct BackendRef(Arc<RecordingBackend>);

impl HostBackend for BackendRef {
    fn show_notification(
        &self,
        payload: &Value,
        wait_rx: Option<std::sync::mpsc::Receiver<()>>,
    ) -> CoreResult<String> {
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
        self.0.show_main_window()
    }
}

fn local_date(offset_days: i64) -> String {
    let date = chrono::Local::now().date_naive() + Duration::days(offset_days);
    date.format("%Y-%m-%d").to_string()
}

fn entry_id(env: &TestEnv) -> String {
    // env.ok 返回的就是信封里的 data
    let added = env.ok(&["task", "add", "--type", "entry", "--name", "提醒条目"]);
    added["id"].as_str().unwrap().to_string()
}

fn tasks(env: &TestEnv) -> Vec<Value> {
    env.read_file("data.json")["tasks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn receipts(env: &TestEnv) -> Value {
    // 台账住在 runtime/ 下：它是本机状态，不进同步载荷
    if env.file_exists("runtime/reminders.json") {
        env.read_file("runtime/reminders.json")
    } else {
        json!({})
    }
}

// ---------------------------------------------------------------------------
// 规格与事务
// ---------------------------------------------------------------------------

#[test]
fn reminder_specs_parse_and_store() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    let due = local_date(1);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "带提醒",
        "--due-date", &due, "--due-time", "18:00",
        "--reminder", "due-60", "--reminder", "+1h",
    ]);
    let task = &tasks(&env)[0];
    let rules = task["reminders"].as_array().unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0]["kind"], "beforeDue");
    assert_eq!(rules[0]["minutes"], 60);
    assert_eq!(rules[1]["kind"], "absolute");
    // +1h 落盘时已经规范化成带时区的 RFC3339
    assert!(rules[1]["at"].as_str().unwrap().contains('T'));
}

#[test]
fn before_due_requires_minute_time() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    let due = local_date(1);
    let error = env.err(
        &[
            "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "只有日期",
            "--due-date", &due, "--reminder", "due-60",
        ],
        2,
    );
    assert_eq!(error["code"], "REMINDER_DUE_REQUIRED");
    // 事务性：任务没建出来
    assert!(tasks(&env).is_empty());
}

#[test]
fn past_reminders_are_rejected_on_create_but_unchanged_ones_survive() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    let yesterday = local_date(-1);
    let error = env.err(
        &[
            "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "过去",
            "--due-date", &yesterday, "--due-time", "09:00", "--reminder", "due-60",
        ],
        2,
    );
    assert_eq!(error["code"], "REMINDER_IN_PAST");
    assert!(tasks(&env).is_empty());

    // 已经存在的过时提醒（比如到点响过了、或同步推来的）不该挡住后续修改。
    // 这种状态只能从「外部」来——命令层创建/修改时本来就会拒，所以直接改数据文件。
    let today = local_date(0);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "今天",
        "--due-date", &today, "--due-time", "23:59",
    ]);
    let id = tasks(&env)[0]["id"].as_str().unwrap().to_string();
    let mut data = env.read_file("data.json");
    data["tasks"][0]["reminders"] = json!([{ "kind": "absolute", "at": "2020-01-01T00:00:00Z" }]);
    env.write_file("data.json", &data);
    env.ok(&["task", "modify", "--type", "item", "--id", &id, "--markdown", "今天改过"]);
    let after = tasks(&env);
    assert_eq!(after[0]["markdown"], "今天改过");
    assert_eq!(after[0]["reminders"].as_array().unwrap().len(), 1);
}

#[test]
fn clearing_the_date_drops_dependent_reminders() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    let due = local_date(1);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "带提醒",
        "--due-date", &due, "--due-time", "18:00",
        "--reminder", "due-60", "--reminder", "+2h",
    ]);
    let id = tasks(&env)[0]["id"].as_str().unwrap().to_string();
    env.ok(&["task", "modify", "--type", "item", "--id", &id, "--clear-due-date"]);
    let task = &tasks(&env)[0];
    assert!(task["dueDate"].is_null());
    // dueTime 是 skip_serializing_if 空的：清掉之后这个键干脆不出现
    assert_eq!(task["dueTime"].as_str().unwrap_or_default(), "");
    // 「截止前」失去依附被清掉，绝对时刻留着
    let rules = task["reminders"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["kind"], "absolute");
}

#[test]
fn replace_semantics_and_bad_specs() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    let due = local_date(1);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "带提醒",
        "--due-date", &due, "--due-time", "18:00", "--reminder", "due-60",
    ]);
    let id = tasks(&env)[0]["id"].as_str().unwrap().to_string();
    // 整体替换：给一条就只剩一条
    env.ok(&["task", "modify", "--type", "item", "--id", &id, "--reminder", "due-5"]);
    let replaced = tasks(&env);
    let rules = replaced[0]["reminders"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["minutes"], 5);
    // 只写 --reminder 不带值 = 清空（空数组在 JSON 里被 skip_serializing_if 省掉）
    env.ok(&["task", "modify", "--type", "item", "--id", &id, "--reminder"]);
    let cleared = tasks(&env);
    assert_eq!(cleared[0]["reminders"].as_array().map(Vec::len).unwrap_or(0), 0);
    // 坏规格报错且不动数据
    let error = env.err(
        &["task", "modify", "--type", "item", "--id", &id, "--reminder", "due-x"],
        2,
    );
    assert_eq!(error["code"], "INVALID_REMINDERS");
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

#[test]
fn engine_fires_once_and_never_replays() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    // 绝对提醒 +3s：dueTime 只精确到分钟，做不了「几秒后到点」的实验
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "马上到点",
        "--reminder", "+3s",
    ]);
    let host = headless(&env);
    let mut engine = reminders::Engine::default();

    engine.poll(&host.core, Utc::now(), true).unwrap();
    assert!(host.backend.notifications.lock().unwrap().is_empty());
    assert!(engine.has_pending(), "第一轮应该记住这条还在未来");

    std::thread::sleep(std::time::Duration::from_millis(3_300));
    engine.poll(&host.core, Utc::now(), false).unwrap();
    let notifications = host.backend.notifications.lock().unwrap().clone();
    assert_eq!(notifications.len(), 1, "到点响一次");
    assert_eq!(notifications[0]["title"], "任务提醒");
    assert!(notifications[0]["message"]
        .as_str()
        .unwrap()
        .contains("马上到点"));

    // 再轮几轮也只响一次（台账里有 receipt 了）
    for _ in 0..3 {
        engine.poll(&host.core, Utc::now(), false).unwrap();
    }
    assert_eq!(host.backend.notifications.lock().unwrap().len(), 1);

    // 台账落盘：sent
    let ledger = receipts(&env);
    let statuses: Vec<&str> = ledger
        .as_object()
        .unwrap()
        .values()
        .map(|value| value["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, vec!["sent"]);
}

#[test]
fn engine_skips_reminders_missed_while_inactive() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "错过的提醒",
        "--due-date", &local_date(1), "--due-time", "09:00",
    ]);
    // 一条早就过期的提醒只能从「外部」来（命令层创建时会拒）：直接改数据文件
    let mut data = env.read_file("data.json");
    data["tasks"][0]["reminders"] = json!([{ "kind": "absolute", "at": "2020-01-01T00:00:00Z" }]);
    env.write_file("data.json", &data);
    let host = headless(&env);
    let mut engine = reminders::Engine::default();
    // 进程刚起来（reset=true）看见它，只能标记跳过，不补发
    engine.poll(&host.core, Utc::now(), true).unwrap();
    assert!(host.backend.notifications.lock().unwrap().is_empty());
    assert!(!engine.has_pending());

    // 之后即使 reset=false 也不补发（台账里已经是 skipped）
    engine.poll(&host.core, Utc::now(), false).unwrap();
    assert!(host.backend.notifications.lock().unwrap().is_empty());
    let ledger = receipts(&env);
    let statuses: Vec<&str> = ledger
        .as_object()
        .unwrap()
        .values()
        .map(|value| value["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, vec!["skipped"]);
}

#[test]
fn engine_stays_silent_for_completed_tasks() {
    let env = TestEnv::fresh();
    let entry = entry_id(&env);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry, "--markdown", "到点前被勾掉",
        "--reminder", "+3s",
    ]);
    let id = tasks(&env)[0]["id"].as_str().unwrap().to_string();
    let host = headless(&env);
    let mut engine = reminders::Engine::default();
    engine.poll(&host.core, Utc::now(), true).unwrap();

    env.ok(&["task", "modify", "--type", "item", "--id", &id, "--completed", "true"]);
    std::thread::sleep(std::time::Duration::from_millis(3_300));
    engine.poll(&host.core, Utc::now(), false).unwrap();
    assert!(
        host.backend.notifications.lock().unwrap().is_empty(),
        "到点前已完成的任务不该再提醒"
    );
    // 已完成的任务连台账都不进（plan 直接跳过），重启后也不会补响
    engine.poll(&host.core, Utc::now(), false).unwrap();
    assert!(host.backend.notifications.lock().unwrap().is_empty());
}

#[test]
fn clock_discontinuity_detects_suspend_and_jumps() {
    use std::time::Duration as StdDuration;
    let base = Utc::now();
    // 正常轮询间隔：不跳
    assert!(!reminders::clock_discontinuous(
        base,
        base + Duration::milliseconds(600),
        StdDuration::from_millis(600)
    ));
    // 休眠三小时：墙上与单调都跳了
    assert!(reminders::clock_discontinuous(
        base,
        base + Duration::hours(3),
        StdDuration::from_secs(3 * 3600)
    ));
    // 用户往回调时间：墙上变小
    assert!(reminders::clock_discontinuous(
        base,
        base - Duration::minutes(10),
        StdDuration::from_millis(600)
    ));
    // 单调时钟在休眠时停走的平台：墙上跳、单调没跳
    assert!(reminders::clock_discontinuous(
        base,
        base + Duration::hours(1),
        StdDuration::from_millis(600)
    ));
}
