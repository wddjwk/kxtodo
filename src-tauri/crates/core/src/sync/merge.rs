//! 实体模型与 LWW 合并（纯函数）：提取本地实体、应用远端实体、墓碑传播、排序归一。
//!
//! 合并规则：每个实体按 `(updatedAt, updatedBy)` 字典序取最大者；删除 = 一条
//! `deleted=true` 的墓碑实体，与普通实体走同一比较（本地更新则复活）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::model::{DataFile, ScheduleFile, SettingsFile};

pub const SETTINGS_ENTITY_ID: &str = "settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scopes {
    pub data: bool,
    pub settings: bool,
    pub schedules: bool,
}

impl Scopes {
    pub fn from_settings(settings: &SettingsFile) -> Self {
        Self {
            data: settings.sync.sync_data,
            settings: settings.sync.sync_settings,
            schedules: settings.sync.sync_schedules,
        }
    }
}

/// 解密后的同步实体（内存表示）。
#[derive(Debug, Clone)]
pub struct EntityRecord {
    /// "node" | "task" | "schedule" | "settings"
    pub kind: String,
    pub id: String,
    pub updated_at: String,
    pub updated_by: String,
    pub deleted: bool,
    pub data: Value,
    /// 服务端版本号（本地提取时为 0）
    pub seq: u64,
}

/// 加密信封明文（wire format）。密文 = XChaCha20-Poly1305(envelope JSON)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEnvelope {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(rename = "updatedBy")]
    pub updated_by: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl EntityRecord {
    pub fn from_envelope(envelope: SyncEnvelope, seq: u64) -> Self {
        Self {
            kind: envelope.kind,
            id: envelope.id,
            updated_at: envelope.updated_at,
            updated_by: envelope.updated_by,
            deleted: envelope.deleted,
            data: envelope.data,
            seq,
        }
    }

    pub fn to_envelope(&self) -> SyncEnvelope {
        SyncEnvelope {
            v: 1,
            kind: self.kind.clone(),
            id: self.id.clone(),
            updated_at: self.updated_at.clone(),
            updated_by: self.updated_by.clone(),
            deleted: self.deleted,
            data: if self.deleted {
                Value::Null
            } else {
                self.data.clone()
            },
        }
    }

    /// LWW 排序键。
    pub fn stamp(&self) -> (&str, &str) {
        (&self.updated_at, &self.updated_by)
    }
}

/// 本地实体的版本戳（数据域：node/task 活实体或墓碑）。
pub fn data_entity_stamp(data: &DataFile, id: &str) -> Option<String> {
    if let Some(node) = data.nodes.iter().find(|node| node.id == id) {
        return Some(
            node.updated_at
                .clone()
                .unwrap_or_else(|| node.created_at.clone()),
        );
    }
    if let Some(item) = data.tasks.iter().find(|item| item.id == id) {
        return Some(
            item.updated_at
                .clone()
                .unwrap_or_else(|| item.created_at.clone()),
        );
    }
    data.meta
        .tombstones
        .iter()
        .find(|tomb| tomb.id == id)
        .map(|tomb| tomb.updated_at.clone())
}

/// 本地实体的版本戳（schedule 域）。
pub fn schedule_entity_stamp(schedule: &ScheduleFile, id: &str) -> Option<String> {
    if let Some(entry) = schedule.tasks.iter().find(|entry| entry.id == id) {
        return Some(entry.updated_at.clone());
    }
    schedule
        .meta
        .tombstones
        .iter()
        .find(|tomb| tomb.id == id)
        .map(|tomb| tomb.updated_at.clone())
}

/// 远端是否胜出（本地无版本，或远端 (ts, by) 更大；相等视为同一版本）。
pub fn remote_wins(
    record: &EntityRecord,
    local_stamp: Option<(&str, &str)>,
) -> bool {
    match local_stamp {
        None => true,
        Some(local) => record.stamp() > local,
    }
}

// ---------------------------------------------------------------------------
// 提取：本地文件 → 实体集合
// ---------------------------------------------------------------------------

fn node_payload(data: &DataFile, node: &crate::model::Node) -> Value {
    let mut payload = serde_json::to_value(node).unwrap_or(Value::Null);
    if let Some(map) = payload.as_object_mut() {
        // collapsed 是本机 UI 状态，不参与同步
        map.remove("collapsed");
        if let Some(background) = data.backgrounds.get(&node.id) {
            map.insert("background".to_string(), background.clone());
        }
    }
    payload
}

fn task_payload(item: &crate::model::Item) -> Value {
    let mut payload = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Some(map) = payload.as_object_mut() {
        // expanded 是本机 UI 状态，不参与同步
        map.remove("expanded");
    }
    payload
}

fn schedule_payload(entry: &crate::model::ScheduleEntry) -> Value {
    json!({
        "spec": serde_json::to_value(&entry.spec).unwrap_or(Value::Null),
        "createdAt": entry.created_at,
    })
}

pub fn settings_payload(settings: &SettingsFile) -> Value {
    json!({
        "profile": {
            "displayName": settings.profile.display_name,
            "email": settings.profile.email,
            "avatar": settings.profile.avatar,
        },
        "appearance": {
            "linkOpenMode": settings.appearance.link_open_mode,
            "themePresets": settings.appearance.theme_presets,
            "uiColors": settings.appearance.ui_colors,
        },
        "features": {
            "showCategoryBadges": settings.features.show_category_badges,
        },
        "updates": {
            "autoCheck": settings.updates.auto_check,
        },
    })
}

/// 提取本设备的全部同步实体（含墓碑），按 scope 裁剪。
pub fn extract_entities(
    data: &DataFile,
    settings: &SettingsFile,
    schedule: &ScheduleFile,
    scopes: &Scopes,
    device_id: &str,
) -> Vec<EntityRecord> {
    let mut out = Vec::new();
    if scopes.data {
        for node in &data.nodes {
            out.push(EntityRecord {
                kind: "node".to_string(),
                id: node.id.clone(),
                updated_at: node
                    .updated_at
                    .clone()
                    .unwrap_or_else(|| node.created_at.clone()),
                updated_by: device_id.to_string(),
                deleted: false,
                data: node_payload(data, node),
                seq: 0,
            });
        }
        for item in &data.tasks {
            out.push(EntityRecord {
                kind: "task".to_string(),
                id: item.id.clone(),
                updated_at: item
                    .updated_at
                    .clone()
                    .unwrap_or_else(|| item.created_at.clone()),
                updated_by: device_id.to_string(),
                deleted: false,
                data: task_payload(item),
                seq: 0,
            });
        }
        for tomb in &data.meta.tombstones {
            if tomb.kind == "node" || tomb.kind == "task" {
                out.push(EntityRecord {
                    kind: tomb.kind.clone(),
                    id: tomb.id.clone(),
                    updated_at: tomb.updated_at.clone(),
                    updated_by: device_id.to_string(),
                    deleted: true,
                    data: Value::Null,
                    seq: 0,
                });
            }
        }
    }
    if scopes.schedules {
        for entry in &schedule.tasks {
            out.push(EntityRecord {
                kind: "schedule".to_string(),
                id: entry.id.clone(),
                updated_at: entry.updated_at.clone(),
                updated_by: device_id.to_string(),
                deleted: false,
                data: schedule_payload(entry),
                seq: 0,
            });
        }
        for tomb in &schedule.meta.tombstones {
            if tomb.kind == "schedule" {
                out.push(EntityRecord {
                    kind: "schedule".to_string(),
                    id: tomb.id.clone(),
                    updated_at: tomb.updated_at.clone(),
                    updated_by: device_id.to_string(),
                    deleted: true,
                    data: Value::Null,
                    seq: 0,
                });
            }
        }
    }
    if scopes.settings {
        if let Some(ts) = &settings.sync_updated_at {
            out.push(EntityRecord {
                kind: "settings".to_string(),
                id: SETTINGS_ENTITY_ID.to_string(),
                updated_at: ts.clone(),
                updated_by: device_id.to_string(),
                deleted: false,
                data: settings_payload(settings),
                seq: 0,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 应用：远端实体 → 本地文件（调用方已确认远端胜出）
// ---------------------------------------------------------------------------

fn apply_node_record(record: &EntityRecord, data: &mut DataFile) -> Result<(), String> {
    let mut node: crate::model::Node =
        serde_json::from_value(record.data.clone()).map_err(|e| e.to_string())?;
    // 本机 UI 状态保留
    if let Some(existing) = data.nodes.iter().find(|n| n.id == node.id) {
        node.collapsed = existing.collapsed;
    }
    let background = record.data.get("background").cloned();
    data.nodes.retain(|n| n.id != node.id);
    data.nodes.push(node);
    match background {
        Some(bg) if !bg.is_null() => {
            data.backgrounds.insert(record.id.clone(), bg);
        }
        _ => {
            data.backgrounds.remove(&record.id);
        }
    }
    Ok(())
}

fn apply_task_record(record: &EntityRecord, data: &mut DataFile) -> Result<(), String> {
    let mut item: crate::model::Item =
        serde_json::from_value(record.data.clone()).map_err(|e| e.to_string())?;
    if let Some(existing) = data.tasks.iter().find(|t| t.id == item.id) {
        item.expanded = existing.expanded;
    }
    data.tasks.retain(|t| t.id != item.id);
    data.tasks.push(item);
    Ok(())
}

pub fn apply_schedule_record(record: &EntityRecord, schedule: &mut ScheduleFile) -> Result<(), String> {
    #[derive(Deserialize)]
    struct SchedulePayload {
        spec: crate::model::ScheduleSpec,
        #[serde(default)]
        created_at: String,
    }
    let payload: SchedulePayload =
        serde_json::from_value(record.data.clone()).map_err(|e| e.to_string())?;
    let existing = schedule
        .tasks
        .iter()
        .position(|entry| entry.id == record.id);
    match existing {
        Some(index) => {
            let entry = &mut schedule.tasks[index];
            entry.spec = payload.spec;
            entry.created_at = payload.created_at;
            entry.updated_at = record.updated_at.clone();
        }
        None => {
            let mut entry = crate::model::ScheduleEntry {
                id: record.id.clone(),
                spec: payload.spec,
                state: Default::default(),
                ui: Default::default(),
                created_at: payload.created_at,
                updated_at: record.updated_at.clone(),
                extra: Map::new(),
            };
            if entry.spec.enabled {
                if let Ok(Some(next)) =
                    crate::plan::compute_next_run_iso(&entry, chrono::Utc::now())
                {
                    entry.state.next_run_at = Some(next);
                }
            }
            schedule.tasks.push(entry);
        }
    }
    Ok(())
}

fn apply_settings_record(record: &EntityRecord, settings: &mut SettingsFile) -> Result<(), String> {
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct SharedSettings {
        #[serde(default)]
        profile: Option<Value>,
        #[serde(default)]
        appearance: Option<Value>,
        #[serde(default)]
        features: Option<Value>,
        #[serde(default)]
        updates: Option<Value>,
    }
    let payload: SharedSettings =
        serde_json::from_value(record.data.clone()).map_err(|e| e.to_string())?;
    if let Some(profile) = payload.profile {
        if let Ok(parsed) = serde_json::from_value::<crate::model::ProfileSettings>(profile) {
            settings.profile = parsed;
        }
    }
    if let Some(appearance) = payload.appearance {
        if let Some(map) = appearance.as_object() {
            if let Some(mode) = map.get("linkOpenMode") {
                if let Ok(parsed) =
                    serde_json::from_value::<crate::model::LinkOpenMode>(mode.clone())
                {
                    settings.appearance.link_open_mode = parsed;
                }
            }
            if let Some(presets) = map.get("themePresets") {
                if let Ok(parsed) = serde_json::from_value::<Vec<crate::model::ThemePreset>>(
                    presets.clone(),
                ) {
                    settings.appearance.theme_presets = parsed;
                }
            }
            if let Some(colors) = map.get("uiColors") {
                if let Some(parsed) = colors.as_object() {
                    settings.appearance.ui_colors = parsed.clone();
                }
            }
        }
    }
    if let Some(features) = payload.features {
        if let Ok(parsed) = serde_json::from_value::<crate::model::FeatureSettings>(features) {
            settings.features = parsed;
        }
    }
    if let Some(updates) = payload.updates {
        if let Ok(parsed) = serde_json::from_value::<crate::model::UpdateSettings>(updates) {
            settings.updates = parsed;
        }
    }
    settings.sync_updated_at = Some(record.updated_at.clone());
    Ok(())
}

/// 应用一条远端胜出的数据域实体（node/task，含墓碑）。
/// 返回 Err(原因) 时调用方跳过该实体。
pub fn apply_data_record(
    record: &EntityRecord,
    data: &mut DataFile,
) -> Result<(), String> {
    if record.deleted {
        match record.kind.as_str() {
            "node" => {
                data.nodes.retain(|n| n.id != record.id);
                data.backgrounds.remove(&record.id);
                data.meta
                    .record_tombstone(&record.id, "node", &record.updated_at);
            }
            "task" => {
                data.tasks.retain(|t| t.id != record.id);
                data.meta
                    .record_tombstone(&record.id, "task", &record.updated_at);
            }
            other => return Err(format!("未知数据域墓碑类型 `{other}`")),
        }
        return Ok(());
    }
    match record.kind.as_str() {
        "node" => apply_node_record(record, data),
        "task" => apply_task_record(record, data),
        other => Err(format!("未知数据域实体类型 `{other}`")),
    }
}

/// 应用 settings 实体（共享子集）。
pub fn apply_record_settings(
    record: &EntityRecord,
    settings: &mut SettingsFile,
) -> Result<(), String> {
    if record.deleted {
        return Err("settings 实体没有墓碑形态".to_string());
    }
    apply_settings_record(record, settings)
}

/// 合并后归一化数组顺序，保证两端渲染一致。
fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

pub fn normalize_data_orders(data: &mut DataFile) {
    data.nodes.sort_by(|a, b| {
        let ka = (a.parent_id.clone().unwrap_or_default(), a.order, &a.id);
        let kb = (b.parent_id.clone().unwrap_or_default(), b.order, &b.id);
        ka.0
            .cmp(&kb.0)
            .then_with(|| cmp_f64(ka.1, kb.1))
            .then_with(|| ka.2.cmp(kb.2))
    });
    data.tasks.sort_by(|a, b| {
        let ka = (&a.node_id, a.order, &a.id);
        let kb = (&b.node_id, b.order, &b.id);
        ka.0
            .cmp(kb.0)
            .then_with(|| cmp_f64(ka.1, kb.1))
            .then_with(|| ka.2.cmp(kb.2))
    });
}

pub fn normalize_schedule_orders(schedule: &mut ScheduleFile) {
    schedule.tasks.sort_by(|a, b| {
        (a.created_at.clone(), a.id.clone()).cmp(&(b.created_at.clone(), b.id.clone()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Item, Node, NodeKind};

    fn node(id: &str, parent: Option<&str>, order: f64, ts: &str) -> Node {
        Node {
            id: id.to_string(),
            kind: NodeKind::Entry,
            name: id.to_string(),
            icon: String::new(),
            parent_id: parent.map(str::to_string),
            order,
            collapsed: None,
            created_at: ts.to_string(),
            updated_at: Some(ts.to_string()),
            extra: Map::new(),
        }
    }

    fn task(id: &str, node_id: &str, order: f64, ts: &str) -> Item {
        Item {
            id: id.to_string(),
            node_id: node_id.to_string(),
            order,
            markdown: id.to_string(),
            completed: false,
            important: false,
            my_day: false,
            planned_date: None,
            due_date: None,
            completed_at: None,
            tags: Vec::new(),
            emojis: Vec::new(),
            expanded: None,
            created_at: ts.to_string(),
            updated_at: Some(ts.to_string()),
            extra: Map::new(),
        }
    }

    fn empty_data() -> DataFile {
        DataFile {
            schema_version: 0,
            meta: Default::default(),
            nodes: Vec::new(),
            tasks: Vec::new(),
            selected_node_id: String::new(),
            backgrounds: Map::new(),
            extra: Map::new(),
        }
    }

    fn record(kind: &str, id: &str, ts: &str, by: &str, data: Value) -> EntityRecord {
        EntityRecord {
            kind: kind.to_string(),
            id: id.to_string(),
            updated_at: ts.to_string(),
            updated_by: by.to_string(),
            deleted: false,
            data,
            seq: 1,
        }
    }

    #[test]
    fn lww_remote_newer_wins_and_order_normalizes() {
        let mut data = DataFile {
            nodes: vec![node("entry-a", None, 0.0, "2026-01-01T00:00:00.000Z")],
            tasks: vec![task("t1", "entry-a", 0.0, "2026-01-01T00:00:00.000Z")],
            ..empty_data()
        };
        let mut settings = SettingsFile::default();
        let mut schedule = ScheduleFile::default();

        let remote = record(
            "node",
            "entry-a",
            "2026-02-01T00:00:00.000Z",
            "dev-b",
            serde_json::to_value(node("entry-a", None, 3.0, "2026-02-01T00:00:00.000Z")).unwrap(),
        );
        assert!(remote_wins(
            &remote,
            Some(("2026-01-01T00:00:00.000Z", "dev-a"))
        ));
        apply_data_record(&remote, &mut data).unwrap();
        normalize_data_orders(&mut data);
        let _ = (&mut settings, &mut schedule);
        assert_eq!(data.nodes[0].updated_at.as_deref(), Some("2026-02-01T00:00:00.000Z"));
        assert_eq!(data.nodes[0].order, 3.0);
    }

    #[test]
    fn lww_local_newer_ignores_remote() {
        let mut data = DataFile {
            nodes: vec![node("entry-a", None, 0.0, "2026-03-01T00:00:00.000Z")],
            ..empty_data()
        };
        let remote = record(
            "node",
            "entry-a",
            "2026-02-01T00:00:00.000Z",
            "dev-b",
            serde_json::to_value(node("entry-a", None, 9.0, "2026-02-01T00:00:00.000Z")).unwrap(),
        );
        assert!(!remote_wins(
            &remote,
            Some(("2026-03-01T00:00:00.000Z", "dev-a"))
        ));
    }

    #[test]
    fn tombstone_delete_and_resurrect() {
        let mut data = DataFile {
            tasks: vec![task("t1", "entry-a", 0.0, "2026-01-01T00:00:00.000Z")],
            ..empty_data()
        };
        let mut settings = SettingsFile::default();
        let mut schedule = ScheduleFile::default();
        let _ = (&mut settings, &mut schedule);

        // 远端墓碑更新 → 删除
        let tomb = EntityRecord {
            kind: "task".to_string(),
            id: "t1".to_string(),
            updated_at: "2026-02-01T00:00:00.000Z".to_string(),
            updated_by: "dev-b".to_string(),
            deleted: true,
            data: Value::Null,
            seq: 2,
        };
        apply_data_record(&tomb, &mut data).unwrap();
        assert!(data.tasks.is_empty());
        assert_eq!(data.meta.tombstones.len(), 1);

        // 本地（墓碑 ts=2月）胜过旧的远端活实体（1月）→ 不复活
        let stale_remote = record(
            "task",
            "t1",
            "2026-01-01T00:00:00.000Z",
            "dev-b",
            serde_json::to_value(task("t1", "entry-a", 0.0, "2026-01-01T00:00:00.000Z")).unwrap(),
        );
        assert!(!remote_wins(
            &stale_remote,
            Some(("2026-02-01T00:00:00.000Z", "dev-b"))
        ));

        // 远端更新（3月）→ 复活
        let fresh_remote = record(
            "task",
            "t1",
            "2026-03-01T00:00:00.000Z",
            "dev-b",
            serde_json::to_value(task("t1", "entry-a", 0.0, "2026-03-01T00:00:00.000Z")).unwrap(),
        );
        apply_data_record(&fresh_remote, &mut data).unwrap();
        assert_eq!(data.tasks.len(), 1);
    }

    #[test]
    fn extraction_respects_scopes_and_excludes_ui_state() {
        let mut data = DataFile {
            nodes: vec![node("entry-a", None, 0.0, "2026-01-01T00:00:00.000Z")],
            tasks: vec![task("t1", "entry-a", 0.0, "2026-01-01T00:00:00.000Z")],
            ..empty_data()
        };
        data.nodes[0].collapsed = Some(true);
        data.tasks[0].expanded = Some(true);
        let settings = SettingsFile::default();
        let schedule = ScheduleFile::default();

        let entities = extract_entities(
            &data,
            &settings,
            &schedule,
            &Scopes {
                data: true,
                settings: false,
                schedules: false,
            },
            "dev-a",
        );
        assert_eq!(entities.len(), 2, "仅 data scope：node + task");
        let node_entity = entities.iter().find(|e| e.kind == "node").unwrap();
        assert!(node_entity.data.get("collapsed").is_none());
        let task_entity = entities.iter().find(|e| e.kind == "task").unwrap();
        assert!(task_entity.data.get("expanded").is_none());
    }
}
