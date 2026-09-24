//! Scheduler rule boundaries and pure planning. No sleeps, clocks or subprocesses.
use chrono::{DateTime, Utc};
use kxtodo_core::model::{ScheduleEntry, ScheduleStatus};
use kxtodo_core::{ops_schedule, plan};
use serde_json::{json, Value};

fn at(raw: &str) -> DateTime<Utc> { DateTime::parse_from_rfc3339(raw).unwrap().with_timezone(&Utc) }
fn entry(trigger: Value) -> ScheduleEntry {
    serde_json::from_value(json!({
        "id": "schedule-test", "spec": { "name": "test", "enabled": true, "trigger": trigger,
            "action": { "type": "notification", "notification": { "message": "test" } } },
        "state": {}, "ui": {}, "createdAt": "2030-01-01T00:00:00Z", "updatedAt": "2030-01-01T00:00:00Z"
    })).unwrap()
}
fn calendar() -> ScheduleEntry { entry(json!({ "type": "calendar", "cron": "0 9 * * *", "timezone": "Asia/Shanghai" })) }
fn probe() -> Value { json!({ "type": "script", "language": "python", "source": { "type": "inline", "code": "print('READY')" }, "timeout": "30s" }) }
fn matcher() -> Value { json!({ "stream": "stdout", "mode": "contains", "pattern": "READY" }) }

#[test]
fn schedule_rules_validate_gate_until_and_cooldown_boundaries() {
    let spec = serde_json::to_value(calendar().spec).unwrap();
    let valid_gate = json!({ "windows": [{ "start": "22:00", "end": "02:00", "weekdays": [1, 5] }], "probe": probe(), "when": matcher() });
    let mut valid = spec.clone(); valid["gate"] = valid_gate.clone(); valid["until"] = json!("2030-02-28");
    assert!(ops_schedule::validate_spec_value(&valid, &Default::default()).is_ok());
    for patch in [
        json!({ "until": "2030-02-30" }), json!({ "until": "2030-2-28" }), json!({ "until": "" }),
        json!({ "gate": { "windows": [] } }), json!({ "gate": { "windows": [], "when": matcher() } }),
        json!({ "gate": { "windows": [], "probe": probe() } }),
        json!({ "gate": { "windows": [{ "start": "24:00", "end": "01:00" }] } }),
        json!({ "gate": { "windows": [{ "start": "9:00", "end": "18:00" }] } }),
        json!({ "gate": { "windows": [{ "start": "09:00", "end": "09:00" }] } }),
        json!({ "gate": { "windows": [{ "start": "09:00", "end": "18:00", "weekdays": [] }] } }),
        json!({ "gate": { "windows": [{ "start": "09:00", "end": "18:00", "weekdays": [7] }] } }),
        json!({ "gate": { "windows": [{ "start": "09:00", "end": "18:00", "weekdays": [1, 1] }] } }),
        json!({ "gate": { "windows": [], "probe": probe(), "when": { "stream": "stdout", "mode": "regex", "pattern": "[" } } }),
        json!({ "gate": { "windows": [], "probe": { "type": "notification" }, "when": matcher() } }),
        json!({ "gate": { "windows": [], "probe": probe(), "when": matcher(), "typo": true } }),
    ] {
        let merged = ops_schedule::apply_patch(&spec, &patch).unwrap();
        assert!(ops_schedule::validate_spec_value(&merged, &Default::default()).is_err(), "accepted {patch}");
    }
    let mut condition = spec.clone();
    condition["trigger"] = json!({ "type": "condition", "every": "1s", "probe": probe(), "when": matcher(), "cooldown": "5m" });
    assert!(ops_schedule::validate_spec_value(&condition, &Default::default()).is_ok());
    for cooldown in ["0s", "-1s", "1.5s", "18446744073709551615ms", ""] {
        condition["trigger"]["cooldown"] = json!(cooldown);
        assert!(ops_schedule::validate_spec_value(&condition, &Default::default()).is_err());
    }
    condition["trigger"]["cooldown"] = json!("1s"); condition["gate"] = valid_gate;
    assert!(ops_schedule::validate_spec_value(&condition, &Default::default()).is_err());
    let mut wrong_branch = spec; wrong_branch["trigger"]["cooldown"] = json!("1s");
    assert!(ops_schedule::validate_spec_value(&wrong_branch, &Default::default()).is_err());
}

#[test]
fn schedule_rules_mobile_allows_windows_but_never_probe_gate() {
    let mut task = calendar();
    task.spec.gate = Some(serde_json::from_value(json!({ "windows": [{ "start": "09:00", "end": "18:00" }] })).unwrap());
    assert!(ops_schedule::ensure_action_supported(&task.spec, true).is_ok());
    task.spec.gate.as_mut().unwrap().probe = Some(serde_json::from_value(probe()).unwrap());
    task.spec.gate.as_mut().unwrap().when = Some(serde_json::from_value(matcher()).unwrap());
    assert_eq!(ops_schedule::ensure_action_supported(&task.spec, true).unwrap_err().code, "SCHEDULE_GATE_UNSUPPORTED");
    assert!(ops_schedule::ensure_action_supported(&task.spec, false).is_ok());
}

#[test]
fn schedule_rules_windows_are_half_open_and_overnight_uses_start_weekday() {
    let mut task = calendar();
    task.spec.gate = Some(serde_json::from_value(json!({ "windows": [{ "start": "22:00", "end": "02:00", "weekdays": [1] }] })).unwrap());
    // 2030-01-07 is Monday in the schedule timezone.
    for (instant, allowed) in [
        ("2030-01-07T21:59:59+08:00", false), ("2030-01-07T22:00:00+08:00", true),
        ("2030-01-08T01:59:59+08:00", true), ("2030-01-08T02:00:00+08:00", false),
        ("2030-01-08T22:00:00+08:00", false),
    ] { assert_eq!(plan::gate_window_allows(&task.spec, at(instant)).unwrap(), allowed, "{instant}"); }
}

#[test]
fn schedule_rules_until_is_inclusive_and_uses_calendar_timezone_across_dst() {
    let mut task = entry(json!({ "type": "calendar", "cron": "0 9 * * *", "timezone": "America/New_York" }));
    task.spec.until = Some("2030-03-10".into());
    assert!(!plan::is_expired(&task.spec, at("2030-03-11T03:59:59.999Z")).unwrap());
    assert!(plan::is_expired(&task.spec, at("2030-03-11T04:00:00Z")).unwrap());
    assert_eq!(plan::compute_next_run(&task, at("2030-03-10T12:00:00Z")).unwrap(), Some(at("2030-03-10T13:00:00Z")));
    assert_eq!(plan::compute_next_run(&task, at("2030-03-10T13:00:00Z")).unwrap(), None);
}

#[test]
fn schedule_rules_calendar_uses_supplied_clock_and_standard_weekdays() {
    let task = entry(json!({ "type": "calendar", "cron": "30 8 * * 1-5", "timezone": "UTC" }));
    assert_eq!(plan::compute_next_run(&task, at("2030-01-04T09:00:00Z")).unwrap(), Some(at("2030-01-07T08:30:00Z")));
    let monthly = entry(json!({ "type": "calendar", "cron": "0 9 31 * *", "timezone": "UTC" }));
    assert_eq!(plan::compute_next_run(&monthly, at("2030-02-01T00:00:00Z")).unwrap(), Some(at("2030-03-31T09:00:00Z")));
}

#[test]
fn schedule_rules_skip_never_changes_execution_counters_status_or_output() {
    let mut task = entry(json!({ "type": "interval", "every": "5m", "maxRuns": 3 }));
    task.state.last_status = ScheduleStatus::Success;
    task.state.last_stdout = Some("previous".into());
    let previous = serde_json::to_value(&task.state).unwrap();
    plan::advance_skipped_gate(&mut task, at("2030-01-01T00:16:00Z")).unwrap();
    assert_eq!(task.state.next_run_at.as_deref(), Some("2030-01-01T00:20:00.000Z"));
    let mut after = serde_json::to_value(&task.state).unwrap(); after.as_object_mut().unwrap().remove("nextRunAt");
    assert_eq!(previous, after);
    let mut once = entry(json!({ "type": "once", "at": "2030-01-01T00:00:00Z" }));
    plan::advance_skipped_gate(&mut once, at("2030-01-01T00:01:00Z")).unwrap();
    assert!(!once.spec.enabled); assert_eq!(once.state.run_count, 0); assert_eq!(once.state.last_status, ScheduleStatus::Idle);
}

#[test]
fn schedule_rules_condition_uses_larger_of_every_and_cooldown() {
    for (every, cooldown, next) in [("1m", "5m", "2030-01-01T00:05:00Z"), ("10m", "5m", "2030-01-01T00:10:00Z")] {
        let mut task = entry(json!({ "type": "condition", "every": every, "cooldown": cooldown, "probe": probe(), "when": matcher() }));
        task.state.last_run_at = Some("2030-01-01T00:00:00Z".into());
        task.state.last_probe = Some(serde_json::from_value(json!({ "at": "2030-01-01T00:00:00Z", "status": "success" })).unwrap());
        assert_eq!(plan::compute_next_run(&task, at("2030-01-01T00:00:00Z")).unwrap(), Some(at(next)));
    }
}

#[test]
fn schedule_rules_patch_removes_optional_rules_and_normalizes_gate_paths() {
    let mut spec = serde_json::to_value(calendar().spec).unwrap();
    spec["until"] = json!("2030-12-31");
    spec["gate"] = json!({ "windows": [], "probe": { "type": "script", "language": "python", "source": { "type": "file", "path": "probe.py" } }, "when": matcher() });
    let cwd = std::env::current_dir().unwrap();
    ops_schedule::normalize_spec_paths(&mut spec, &cwd);
    assert_eq!(spec["gate"]["probe"]["source"]["path"], cwd.join("probe.py").display().to_string());
    let cleared = ops_schedule::apply_patch(&spec, &json!({ "gate": null, "until": null })).unwrap();
    assert!(cleared.get("gate").is_none()); assert!(cleared.get("until").is_none());
}
