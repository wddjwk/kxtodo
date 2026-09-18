//! 任务提醒：规则解析 + 本机执行引擎（v0.8.3）。
//!
//! 提醒**不是定时任务**：不建 `ScheduleEntry`、不写任务时间戳，只是「到点弹一个通知」。
//! 两条边界必须钉死：
//!
//! 1. **规则跟着任务同步，台账不同步**。「哪一条已经响过」是本机状态，落在
//!    `runtime/reminders.json`——它要是进了同步载荷，一台设备响过另一台就永远不响了。
//! 2. **过时的提醒一律不补发**。设备离线/休眠期间到点的提醒，上线后直接标记跳过：
//!    用户要的是「提醒我」，不是「把我离开这三小时欠的账一次性倒出来」。
//!
//! 第 2 条靠两层实现：`Engine::pending` 只记住「上一轮亲眼看见还在未来」的那些键
//! （没见过就直接跳过），加上调用方在时钟跳变（休眠唤醒、手动改时间）时传 `reset`
//! 清空这份记忆。

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CoreError, CoreResult};
use crate::host::HostCore;
use crate::model::{DataFile, Item, Reminder};
use crate::repo::{write_json_atomic, RepoLock, Repository};
use crate::time::{format_instant, parse_instant, parse_stored_instant};

/// 解析命令层的 `reminders` 参数：**字符串规格数组**。
///
/// 与 `tags` 的 `"color:text"` 同一套口径——CLI 与 GUI 都发这个形状，core 一处解析，
/// 免得两边各写一份规格语法（两份一定会漂）。
///
/// 规格两种：
/// - `due-<分钟>`：截止前 N 分钟（`due-0` = 到点时），需要任务同时有截止日期与分钟时刻
/// - 其余一律按绝对时刻解析：RFC3339（`2026-09-20T09:00:00+08:00`）或相对写法（`+30m`/`+1h`/`+2d`）
pub fn parse_rules(value: &Value) -> CoreResult<Vec<Reminder>> {
    let Some(list) = value.as_array() else {
        return Err(CoreError::validation(
            "INVALID_REMINDERS",
            "reminders 必须是提醒规格的数组",
        ));
    };
    list.iter().map(parse_spec).collect()
}

fn parse_spec(value: &Value) -> CoreResult<Reminder> {
    let raw = value.as_str().ok_or_else(|| {
        CoreError::validation(
            "INVALID_REMINDERS",
            "每条提醒必须是字符串规格（如 due-60、+1h 或 2026-09-20T09:00:00+08:00）",
        )
    })?;
    let text = raw.trim();
    if let Some(minutes) = text.strip_prefix("due-") {
        let minutes: u32 = minutes.trim().parse().map_err(|_| {
            CoreError::validation(
                "INVALID_REMINDERS",
                format!("无效提醒 `{raw}`：due- 后面要跟分钟数（如 due-60 = 截止前一小时）"),
            )
        })?;
        return Ok(Reminder::BeforeDue { minutes });
    }
    // parse_instant 已经把它规范化成 UTC 毫秒，落盘与同步的字符串因此逐字节一致
    Ok(Reminder::Absolute {
        at: parse_instant(text)?,
    })
}

/// `parse_rules` 的「参数可以没给」版本。
///
/// `None` 是 `task modify` 的语义分界：**没给 / null = 这次不动提醒**，
/// 而**空数组 = 清空全部提醒**。两者的区别只有 `Option` 能表达，
/// 所以这里不把缺省折成空数组。`task add` 拿到 `None` 再 `unwrap_or_default()`。
pub fn parse_rules_opt(value: Option<&Value>) -> CoreResult<Option<Vec<Reminder>>> {
    match value {
        Some(value) if value.is_array() => Ok(Some(parse_rules(value)?)),
        _ => Ok(None),
    }
}

/// 截止日期 + 时刻是**本地墙上时间**，按设备时区解释成瞬时。
///
/// 夏令时切换那一刻「不存在」或「出现两次」，`single()` 会返回 None——这时明确报错，
/// 而不是随便挑一个瞬时。挑错了的后果是提醒早/晚一小时且用户完全无从察觉。
pub fn due_in_zone<T: TimeZone>(date: &str, time: &str, zone: &T) -> CoreResult<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| CoreError::validation("INVALID_DATE", format!("无效截止日期 `{date}`")))?;
    let time = NaiveTime::parse_from_str(time, "%H:%M")
        .map_err(|_| CoreError::validation("INVALID_TIME", format!("无效截止时刻 `{time}`")))?;
    zone.from_local_datetime(&date.and_time(time))
        .single()
        .map(|at| at.with_timezone(&Utc))
        .ok_or_else(|| {
            CoreError::validation(
                "AMBIGUOUS_DUE_TIME",
                "截止时刻在本地时区不存在或有歧义（夏令时切换），请换一个时刻",
            )
        })
}

/// 把一条规则算成绝对瞬时。`BeforeDue` 需要任务同时有截止日期与分钟时刻。
pub fn resolve(rule: &Reminder, date: Option<&str>, time: &str) -> CoreResult<DateTime<Utc>> {
    match rule {
        Reminder::Absolute { at } => parse_stored_instant(at),
        Reminder::BeforeDue { minutes } => {
            let date = date.filter(|_| !time.is_empty()).ok_or_else(|| {
                CoreError::validation(
                    "REMINDER_DUE_REQUIRED",
                    "「截止前」提醒需要任务同时设置截止日期与精确到分钟的时刻",
                )
            })?;
            let span = chrono::Duration::minutes(i64::from(*minutes));
            due_in_zone(date, time, &Local)?
                .checked_sub_signed(span)
                .ok_or_else(|| {
                    CoreError::validation("INVALID_REMINDERS", "提前量超出可表示的时间范围")
                })
        }
    }
}

/// 两条规则「形状」是否相同（不看解析结果，用于判断某条是否只是原样保留）。
fn same_shape(left: &Reminder, right: &Reminder) -> bool {
    match (left, right) {
        (Reminder::Absolute { .. }, Reminder::Absolute { .. }) => true,
        (Reminder::BeforeDue { minutes: a }, Reminder::BeforeDue { minutes: b }) => a == b,
        _ => false,
    }
}

/// 写入前的整体验证：日期、时刻、提醒三样是一个事务，任何一样不合法都不许落盘。
///
/// `previous` 是改前的任务（新建时 None）。**原样保留的过时规则不算错**——用户改了
/// 截止日期不该被一条早就响过的旧提醒挡住；但**新增或改动**的规则必须落在未来，
/// 否则它会立刻被引擎标记跳过，用户以为设了提醒其实永远不会响（这条对 CLI/Agent
/// 同样成立，所以放在 core 而不是前端）。
///
/// `clear_dependent` = 这次改动清掉了日期或时刻，此时「截止前」规则失去依附对象，
/// 一并清掉（留着就是一条永远解析失败的死规则）。
pub fn normalize_metadata(
    date: &mut Option<String>,
    time: &mut String,
    rules: &mut Vec<Reminder>,
    previous: Option<&Item>,
    clear_dependent: bool,
    now: DateTime<Utc>,
) -> CoreResult<()> {
    if let Some(raw) = date.as_deref() {
        *date = Some(crate::time::parse_date(raw)?);
    }
    if clear_dependent && date.is_none() {
        time.clear();
    }
    if !time.is_empty() {
        *time = crate::time::parse_clock(time)?;
        if date.is_none() {
            return Err(CoreError::validation(
                "DUE_DATE_REQUIRED",
                "设置了截止时刻就必须有截止日期",
            ));
        }
    }
    if clear_dependent {
        rules.retain(|rule| !matches!(rule, Reminder::BeforeDue { .. }));
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(rules.len());
    for rule in rules.drain(..) {
        let mut rule = rule;
        // 绝对时刻一律规范化成 UTC 毫秒（CLI 可以传 +1h 这类相对写法）
        if let Reminder::Absolute { at } = &mut rule {
            *at = parse_instant(at)?;
        }
        let at = resolve(&rule, date.as_deref(), time)?;
        let unchanged = previous.is_some_and(|item| {
            item.reminders.iter().any(|old| {
                same_shape(old, &rule)
                    && resolve(old, item.due_date.as_deref(), &item.due_time).ok() == Some(at)
            })
        });
        if at <= now && !unchanged {
            return Err(CoreError::validation(
                "REMINDER_IN_PAST",
                "新增或修改的提醒必须在未来；已经错过的提醒不会补发",
            ));
        }
        // 同一瞬时只留一条：提醒的身份就是「这个任务在这一刻响一次」，
        // 两条规则算到同一刻（比如手写的绝对时刻正好等于「截止前 5 分钟」）
        // 也只该弹一次。
        if seen.insert(at.timestamp_millis()) {
            normalized.push(rule);
        }
    }
    *rules = normalized;
    Ok(())
}

/// 一条「已经算出瞬时、等着到点」的提醒。
///
/// 刻意不携带规则本身：台账与去重都只认「哪个任务在哪一刻响」，
/// 规则改了（比如把「截止前 5 分钟」改成 10 分钟）自然算出新的瞬时、成为新的一条。
#[derive(Debug, Clone)]
pub struct PlannedReminder {
    /// 台账键：任务 id + 瞬时
    pub key: String,
    pub task_id: String,
    pub at: DateTime<Utc>,
}

fn receipt_key(task_id: &str, at: DateTime<Utc>) -> String {
    // 用 JSON 元组编码而不是分隔符拼接：拼接的分隔符万一出现在字段里就有歧义。
    serde_json::to_string(&(task_id, format_instant(at)))
        .unwrap_or_else(|_| format!("{task_id}|{}", at.timestamp_millis()))
}

/// 把当前数据里所有**未完成**任务的提醒算成待触发清单（同一任务的同一瞬时只出一条）。
///
/// 解析失败的规则（比如截止时刻被清掉了却还留着 `BeforeDue`）直接跳过——
/// 轮询是后台循环，不能因为一条脏数据每 500ms 报一次错。
pub fn plan(data: &DataFile) -> Vec<PlannedReminder> {
    let mut plans = Vec::new();
    for item in data.tasks.iter().filter(|item| !item.completed) {
        let mut seen = HashSet::new();
        for rule in &item.reminders {
            let Ok(at) = resolve(rule, item.due_date.as_deref(), &item.due_time) else {
                continue;
            };
            if !seen.insert(at.timestamp_millis()) {
                continue;
            }
            plans.push(PlannedReminder {
                key: receipt_key(&item.id, at),
                task_id: item.id.clone(),
                at,
            });
        }
    }
    plans
}

/// 时钟是否发生了跳变（休眠唤醒、用户改系统时间、时区切换）。
///
/// 墙上时钟与单调时钟**都要看**：有的平台休眠时单调时钟也停（醒来后墙上跳了、
/// 单调没跳），有的则继续走（两者都跳）；用户往回调时间则墙上变小。
/// 5 秒的容差是给正常轮询抖动的，不是给积压补发用的。
pub fn clock_discontinuous(previous: DateTime<Utc>, now: DateTime<Utc>, elapsed: Duration) -> bool {
    let wall = now.signed_duration_since(previous).num_milliseconds();
    let monotonic = i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX);
    wall < 0 || wall > 5_000 || monotonic > 5_000 || wall.abs_diff(monotonic) > 2_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    /// claimed / skipped / sent / failed / cancelled
    status: String,
    at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

type Receipts = BTreeMap<String, Receipt>;

fn receipts_path(repo: &Repository) -> std::path::PathBuf {
    repo.layout.runtime_dir().join("reminders.json")
}

fn load_receipts(repo: &Repository) -> CoreResult<Receipts> {
    match std::fs::read_to_string(receipts_path(repo)) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|error| {
            // 宁可报错停下也不当成空台账：台账丢了就等于所有提醒重放一遍。
            CoreError::execution("REMINDER_STATE_CORRUPTED", format!("提醒台账损坏：{error}"))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.into()),
    }
}

fn save_receipts(repo: &Repository, receipts: &Receipts) -> CoreResult<()> {
    std::fs::create_dir_all(repo.layout.runtime_dir())?;
    write_json_atomic(&receipts_path(repo), &serde_json::to_value(receipts)?)
}

fn receipt(status: &str, now: DateTime<Utc>, error: Option<String>) -> Receipt {
    Receipt {
        status: status.to_owned(),
        at: format_instant(now),
        error,
    }
}

/// 提醒执行引擎。调度器线程持有它，每轮 tick 调一次 `poll`。
#[derive(Default)]
pub struct Engine {
    /// 上一轮亲眼看见「还在未来」的键。只有在这里出现过、这一轮到点的才允许触发。
    pending: HashSet<String>,
}

impl Engine {
    /// 还有没有等着触发的提醒（看门狗据此决定能不能自动退出）。
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// 每轮重新从磁盘对账（同步可能刚改了任务），领取与落盘都在跨进程锁内完成。
    ///
    /// 领取（claimed）先落盘再发通知：进程在「发出去了但没记下来」的窗口里崩掉，
    /// 结果是少响一次；反过来则是重启后重复轰炸。通知这种一次性动作，少一次比多一次好。
    pub fn poll(&mut self, core: &HostCore, now: DateTime<Utc>, reset: bool) -> CoreResult<()> {
        let due = {
            let _lock = RepoLock::acquire(&core.repo.layout)?;
            let data = core.repo.load_data()?;
            let mut receipts = load_receipts(&core.repo)?;
            let mut pending = HashSet::new();
            let mut due = Vec::new();
            let mut changed = false;
            for planned in plan(&data) {
                if receipts.contains_key(&planned.key) {
                    continue;
                }
                if planned.at > now {
                    pending.insert(planned.key);
                    continue;
                }
                // 到点了：只有上一轮见过它在未来才发，否则（刚启动、刚唤醒、
                // 同步推来一条早就过期的）标记跳过。
                let fire = !reset && self.pending.contains(&planned.key);
                receipts.insert(
                    planned.key.clone(),
                    receipt(if fire { "claimed" } else { "skipped" }, now, None),
                );
                changed = true;
                if fire {
                    due.push(planned);
                }
            }
            if changed {
                save_receipts(&core.repo, &receipts)?;
            }
            self.pending = pending;
            due
        };
        for planned in due {
            // 发通知可能耗时（建窗口 / 等系统通知服务），期间命令或同步可能已经
            // 改掉了这条任务，所以每一条都在发之前重新核对一次。
            let current = {
                let _lock = RepoLock::acquire(&core.repo.layout)?;
                core.repo.load_data()?.tasks.into_iter().find(|item| {
                    item.id == planned.task_id
                        && !item.completed
                        && item.reminders.iter().any(|rule| {
                            resolve(rule, item.due_date.as_deref(), &item.due_time).ok()
                                == Some(planned.at)
                        })
                })
            };
            let outcome = current.map(|item| {
                let message = item
                    .markdown
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or("有一条任务到点了");
                core.resolve_notification_payload(Some("任务提醒"), message, None, None, None, false)
                    .and_then(|payload| core.show_notification_payload(payload))
            });
            let (status, error) = match outcome {
                Some(Ok(_)) => ("sent", None),
                Some(Err(error)) => ("failed", Some(error.message)),
                None => ("cancelled", None),
            };
            let _lock = RepoLock::acquire(&core.repo.layout)?;
            let mut receipts = load_receipts(&core.repo)?;
            receipts.insert(planned.key, receipt(status, now, error));
            save_receipts(&core.repo, &receipts)?;
        }
        Ok(())
    }
}
