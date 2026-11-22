//! gui namespace: GUI-sourced business writes (§4.3; internal, not a CLI surface).
//!
//! **除 `clipboard-configure` 外，这些命令一律不发域事件**（没有 `notify_host`）：
//! 调用方只有 GUI 桥，而前端在 `actions.ts` 里是「先乐观更新 store、再 dispatch」，
//! 事件只会换来一次多余的全量快照往返。`clipboard-configure` 是设置类业务写：
//! 后台剪切板服务必须靠 Settings 域事件启停，所以它显式 notify_host；前端用
//! `noteEnvelopeRevision` 把随后的快照往返挡在 revision 水位之外。
//! 纯 UI 状态写（select-node / set-collapsed / set-*-ui）另外还不进审计流，见
//! `Repository::audit` 的 UI_ONLY_COMMANDS。**若将来把 gui.* 开放给 CLI 或出现第二个
//! 消费者，必须给 apply-tree-order 与 import-state 这两个实质业务写补 `notify_host`**，
//! 否则另一个进程改了树、正在跑的 GUI 永远不会回刷。

use serde_json::{json, Map, Value};

use crate::core::{ExecContext, Invocation};
use crate::envelope::Meta;
use crate::error::{CoreError, CoreResult};
use crate::model::{ClipboardSettings, DataFile, NodeKind, SYSTEM_NODE_IDS};
use crate::ops_task as task_ops;
use crate::repo::Domain;

fn required_str(params: &Value, key: &str) -> CoreResult<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CoreError::validation("MISSING_PARAM", format!("缺少必填参数 {key}")))
}

pub fn gui_dispatch(
    action: &str,
    inv: &Invocation,
    ctx: &ExecContext,
    meta: &mut Meta,
) -> CoreResult<Value> {
    match action {
        "select-node" => {
            let node_id = required_str(&inv.params, "nodeId")?;
            let data = ctx.repo.load_data()?;
            if task_ops::find_node(&data, &node_id).is_none() {
                return Err(CoreError::not_found(
                    "NODE_NOT_FOUND",
                    format!("未找到节点 {node_id}"),
                ));
            }
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                file.selected_node_id = node_id.clone();
                Ok(json!({ "id": node_id }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "selectedNodeId": node_id, "revision": outcome.revision }))
        }
        "set-collapsed" => {
            let id = required_str(&inv.params, "id")?;
            let collapsed = inv
                .params
                .get("collapsed")
                .and_then(Value::as_bool)
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 collapsed"))?;
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                let node = task_ops::find_node_mut(file, &id).ok_or_else(|| {
                    CoreError::not_found("NODE_NOT_FOUND", format!("未找到节点 {id}"))
                })?;
                node.collapsed = Some(collapsed);
                Ok(json!({ "id": id }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "id": id, "collapsed": collapsed }))
        }
        "set-all-collapsed" => {
            let collapsed = inv
                .params
                .get("collapsed")
                .and_then(Value::as_bool)
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 collapsed"))?;
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                let mut changed = 0usize;
                for node in file.nodes.iter_mut() {
                    if node.kind == NodeKind::Category && node.collapsed != Some(collapsed) {
                        node.collapsed = Some(collapsed);
                        changed += 1;
                    }
                }
                Ok(json!({ "collapsed": collapsed, "changed": changed }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "collapsed": collapsed }))
        }
        // 剪切板设置整份写入：一次保存只触发一次 Settings 域事件，
        // 否则逐字段 config.set 会让后台服务反复重启（每个字段一次）。
        "clipboard-configure" => {
            let raw = inv
                .params
                .get("settings")
                .cloned()
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 settings"))?;
            let parsed: ClipboardSettings = serde_json::from_value(raw)
                .map_err(|error| CoreError::validation("INVALID_CONFIG_VALUE", format!("剪切板设置格式不正确：{error}")))?;
            let normalized = crate::clipboard_sync::normalize_settings(&parsed)?;
            let (_file, outcome) = ctx.repo.write_settings(
                inv.controls.if_revision,
                inv.controls.idempotency_key.as_deref(),
                &inv.command,
                |file| {
                    file.clipboard = normalized.clone();
                    Ok(json!({ "clipboard": normalized }))
                },
            )?;
            crate::core::notify_host(ctx, Domain::Settings, outcome.revision, vec!["clipboard".to_string()]);
            meta.revision_domain = Some(Domain::Settings);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "revision": outcome.revision }))
        }
        "clipboard-peer" => {
            // 每台设备的「同步 / 不同步」勾选是纯本机偏好：不进 settings.json
            // （否则每点一次复选框都会重启一次后台服务），也不进审计台账。
            let device = required_str(&inv.params, "deviceId")?;
            let selected = inv
                .params
                .get("selected")
                .and_then(Value::as_bool)
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 selected"))?;
            crate::clipboard_sync::set_peer_selected(&ctx.repo.layout, &device, selected)?;
            Ok(json!({ "deviceId": device, "selected": selected }))
        }
        "set-item-ui" => {
            let id = required_str(&inv.params, "id")?;
            let expanded = inv.params.get("expanded").and_then(Value::as_bool);
            // UI 临时状态：不触碰 updatedAt。
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                let item = file
                    .tasks
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or_else(|| {
                        CoreError::not_found("TASK_NOT_FOUND", format!("未找到任务 {id}"))
                    })?;
                if let Some(expanded) = expanded {
                    item.expanded = Some(expanded);
                }
                Ok(json!({ "id": id }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "id": id, "expanded": expanded }))
        }
        "set-items-ui" => {
            let ids: Vec<String> = inv
                .params
                .get("ids")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let expanded = inv.params.get("expanded").and_then(Value::as_bool);
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                // 「展开全部」一次能带上几百个 id：用 HashSet，否则这里是 O(任务数 × id 数)。
                let ids: std::collections::HashSet<&str> =
                    ids.iter().map(String::as_str).collect();
                for item in file.tasks.iter_mut().filter(|item| ids.contains(item.id.as_str())) {
                    if let Some(expanded) = expanded {
                        item.expanded = Some(expanded);
                    }
                }
                Ok(json!({ "count": ids.len() }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "updated": ids.len() }))
        }
        "set-diary-ui" => {
            let id = required_str(&inv.params, "id")?;
            let expanded = inv.params.get("expanded").and_then(Value::as_bool);
            // UI 临时状态：不触碰 updatedAt。
            let (_file, outcome) = ctx.repo.write_diary(None, None, &inv.command, |file| {
                let entry = file
                    .entries
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| {
                        CoreError::not_found("DIARY_NOT_FOUND", format!("未找到日记 {id}"))
                    })?;
                if let Some(expanded) = expanded {
                    entry.expanded = Some(expanded);
                }
                Ok(json!({ "id": id }))
            })?;
            meta.revision_domain = Some(Domain::Diary);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "id": id, "expanded": expanded }))
        }
        "set-schedule-ui" => {
            let id = required_str(&inv.params, "id")?;
            let expanded = inv.params.get("expanded").and_then(Value::as_bool);
            let editing = inv.params.get("editing").and_then(Value::as_bool);
            let (_file, outcome) = ctx.repo.write_schedule(None, None, &inv.command, |file| {
                let entry = file
                    .tasks
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| {
                        CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
                    })?;
                if let Some(expanded) = expanded {
                    entry.ui.expanded = Some(expanded);
                }
                if let Some(editing) = editing {
                    entry.ui.editing = Some(editing);
                }
                Ok(json!({ "id": id }))
            })?;
            meta.revision_domain = Some(Domain::Schedule);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "id": id, "expanded": expanded, "editing": editing }))
        }
        "set-background" => {
            let node_id = required_str(&inv.params, "nodeId")?;
            let color = inv
                .params
                .get("color")
                .and_then(Value::as_str)
                .map(str::to_string);
            let image = inv.params.get("image").cloned();
            let image_opacity = inv.params.get("imageOpacity").and_then(Value::as_f64);
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                // 背景属于 node 同步实体的一部分：变化需要刷新 updatedAt。
                if let Some(node) = file.nodes.iter_mut().find(|n| n.id == node_id) {
                    node.updated_at = Some(crate::time::now_iso());
                }
                let entry = file
                    .backgrounds
                    .entry(node_id.clone())
                    .or_insert_with(|| json!({ "color": "#f4f1ea" }));
                let Some(map) = entry.as_object_mut() else {
                    return Err(CoreError::internal("background 结构异常"));
                };
                if let Some(color) = &color {
                    map.insert("color".to_string(), json!(color));
                }
                if let Some(image) = &image {
                    if image.is_null() {
                        map.remove("image");
                    } else {
                        map.insert("image".to_string(), image.clone());
                    }
                }
                if let Some(opacity) = image_opacity {
                    map.insert("imageOpacity".to_string(), json!(opacity));
                }
                Ok(json!({ "id": node_id }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "nodeId": node_id }))
        }
        "set-scratchpad" => {
            // 草稿纸（v0.8.4 需求 2；v0.9.0 标签页化）：tabs + activeTabId 一次整份覆盖，
            // 写一次就是一条审计 + 一次 revision（前端把防抖拉长到秒级，台账不会被
            // 逐键输入灌爆）。结构校验只做底线：id 非空且不重复、名字截断。
            let raw_tabs = inv
                .params
                .get("tabs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut tabs: Vec<crate::model::ScratchpadTab> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for raw in raw_tabs {
                let Ok(mut tab) = serde_json::from_value::<crate::model::ScratchpadTab>(raw) else {
                    continue;
                };
                if tab.id.trim().is_empty() || !seen.insert(tab.id.clone()) {
                    continue;
                }
                tab.name = tab.name.chars().take(40).collect();
                tabs.push(tab);
            }
            let active_tab_id = inv
                .params
                .get("activeTabId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let stamp = crate::time::now_iso();
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                file.scratchpad = crate::model::Scratchpad {
                    tabs: tabs.clone(),
                    active_tab_id: active_tab_id.clone(),
                    updated_at: stamp.clone(),
                    extra: Map::new(),
                };
                Ok(json!({ "updatedAt": stamp, "tabs": tabs.len() }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "tabs": tabs.len(), "activeTabId": active_tab_id, "updatedAt": stamp }))
        }
        "set-slogan" => {
            // 标语栏（v0.9.4 单例；v0.9.5 条目级图标 + 六位置 + 主题/字色/透明度/轮播间隔）。
            // 校验做底线：id 非空去重、文本截断、activeIds 必须指向存在的标语、图标在白名单内、
            // 颜色形态、透明度/间隔区间、位置枚举。
            fn slogan_color(path: &str, value: Option<&Value>) -> crate::error::CoreResult<String> {
                let raw = value.and_then(Value::as_str).unwrap_or_default().trim();
                if raw.is_empty() {
                    return Ok(String::new());
                }
                if !crate::ops_config::is_hex_color(raw) {
                    return Err(CoreError::validation(
                        path,
                        format!("颜色 `{raw}` 无效，应为 #rrggbb"),
                    ));
                }
                Ok(raw.to_ascii_lowercase())
            }
            let raw_items = inv
                .params
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut items: Vec<crate::model::SloganItem> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for raw in raw_items {
                let Some(id) = raw.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let id = id.trim().to_string();
                if id.is_empty() || !seen.insert(id.clone()) {
                    continue;
                }
                let text: String = raw
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(crate::model::SLOGAN_TEXT_MAX)
                    .collect();
                if text.is_empty() {
                    continue;
                }
                let icon = raw.get("icon").and_then(Value::as_str).unwrap_or_default().trim();
                let icon = if crate::model::is_slogan_icon_value(icon) {
                    icon.to_string()
                } else {
                    return Err(CoreError::validation(
                        "SLOGAN_ICON_INVALID",
                        format!("图标 `{icon}` 不是已知图标或 emoji"),
                    ));
                };
                let created_at = raw
                    .get("createdAt")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                items.push(crate::model::SloganItem {
                    id,
                    text,
                    icon,
                    created_at: if created_at.is_empty() { crate::time::now_iso() } else { created_at },
                    extra: Map::new(),
                });
                if items.len() >= crate::model::SLOGAN_ITEMS_MAX {
                    break;
                }
            }
            let known: std::collections::HashSet<&str> =
                items.iter().map(|item| item.id.as_str()).collect();
            let mut active_ids: Vec<String> = Vec::new();
            if let Some(raw) = inv.params.get("activeIds").and_then(Value::as_array) {
                for value in raw {
                    if let Some(id) = value.as_str() {
                        if known.contains(id) && !active_ids.iter().any(|item| item == id) {
                            active_ids.push(id.to_string());
                        }
                    }
                }
            }
            let accent = slogan_color("SLOGAN_ACCENT_INVALID", inv.params.get("accent"))?;
            let text_color = slogan_color("SLOGAN_TEXT_COLOR_INVALID", inv.params.get("textColor"))?;
            let default_icon = inv
                .params
                .get("defaultIcon")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let default_icon = if crate::model::is_slogan_icon_value(default_icon) {
                default_icon.to_string()
            } else {
                return Err(CoreError::validation(
                    "SLOGAN_ICON_INVALID",
                    format!("默认图标 `{default_icon}` 不是已知图标或 emoji"),
                ));
            };
            let bounds = inv.params.get("bounds").and_then(Value::as_str).unwrap_or("field");
            if !crate::model::SLOGAN_BOUNDS.contains(&bounds) {
                return Err(CoreError::validation(
                    "SLOGAN_BOUNDS_INVALID",
                    format!("边界基准 `{bounds}` 无效，应为 field 或 app"),
                ));
            }
            let opacity = match inv.params.get("opacity") {
                None => crate::model::Slogan::default().opacity,
                Some(value) => {
                    let raw = value.as_f64().unwrap_or(f64::NAN);
                    if !raw.is_finite()
                        || !(crate::model::SLOGAN_OPACITY_MIN..=crate::model::SLOGAN_OPACITY_MAX)
                            .contains(&raw)
                    {
                        return Err(CoreError::validation(
                            "SLOGAN_OPACITY_INVALID",
                            format!(
                                "透明度应在 {}–{} 之间",
                                crate::model::SLOGAN_OPACITY_MIN,
                                crate::model::SLOGAN_OPACITY_MAX
                            ),
                        ));
                    }
                    raw
                }
            };
            let interval_seconds = match inv.params.get("intervalSeconds") {
                None => crate::model::Slogan::default().interval_seconds,
                Some(value) => {
                    let Some(raw) = value.as_u64() else {
                        return Err(CoreError::validation(
                            "SLOGAN_INTERVAL_INVALID",
                            "轮播间隔应为整数秒",
                        ));
                    };
                    if !(u64::from(crate::model::SLOGAN_INTERVAL_MIN)
                        ..=u64::from(crate::model::SLOGAN_INTERVAL_MAX))
                        .contains(&raw)
                    {
                        return Err(CoreError::validation(
                            "SLOGAN_INTERVAL_INVALID",
                            format!(
                                "轮播间隔应在 {}–{} 秒之间",
                                crate::model::SLOGAN_INTERVAL_MIN,
                                crate::model::SLOGAN_INTERVAL_MAX
                            ),
                        ));
                    }
                    raw as u32
                }
            };
            let position = inv
                .params
                .get("position")
                .and_then(Value::as_str)
                .unwrap_or("top-right");
            if !crate::model::SLOGAN_POSITIONS.contains(&position) {
                return Err(CoreError::validation(
                    "SLOGAN_POSITION_INVALID",
                    format!(
                        "位置 `{position}` 无效，应为 {} 之一",
                        crate::model::SLOGAN_POSITIONS.join("/")
                    ),
                ));
            }
            let enabled = inv.params.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let stamp = crate::time::now_iso();
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                file.slogan = crate::model::Slogan {
                    items: items.clone(),
                    active_ids: active_ids.clone(),
                    bounds: bounds.to_string(),
                    default_icon: default_icon.clone(),
                    accent: accent.clone(),
                    text_color: text_color.clone(),
                    opacity,
                    interval_seconds,
                    position: position.to_string(),
                    enabled,
                    updated_at: stamp.clone(),
                    extra: Map::new(),
                };
                Ok(json!({ "updatedAt": stamp, "items": items.len() }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "items": items.len(), "activeIds": active_ids.len(), "updatedAt": stamp }))
        }
        "apply-tree-order" => {
            let ordered_ids: Vec<String> = inv
                .params
                .get("orderedIds")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let parent_changes = inv
                .params
                .get("parentChanges")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                let mut current: Vec<String> =
                    file.nodes.iter().map(|node| node.id.clone()).collect();
                let mut requested = ordered_ids.clone();
                current.sort();
                requested.sort();
                if current != requested {
                    return Err(CoreError::conflict(
                        "TREE_ORDER_MISMATCH",
                        "节点集合与当前数据不一致，请刷新后重试",
                    ));
                }
                // 真正换了父节点的 id：下面写 order 时要用它一起判定「这个节点变了」。
                let mut reparented: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                for (id, parent) in &parent_changes {
                    let parent_id = if parent.is_null() {
                        None
                    } else {
                        parent.as_str().map(str::to_string)
                    };
                    ensure_category_target(file, parent_id.as_deref())?;
                    if would_create_cycle(file, id, parent_id.as_deref()) {
                        return Err(CoreError::conflict(
                            "CATEGORY_CYCLE",
                            "移动会形成分类循环，已拒绝",
                        ));
                    }
                    let node = task_ops::find_node_mut(file, id).ok_or_else(|| {
                        CoreError::not_found("NODE_NOT_FOUND", format!("未找到节点 {id}"))
                    })?;
                    if node.parent_id != parent_id {
                        reparented.insert(id.clone());
                    }
                    node.parent_id = parent_id;
                }
                let position: std::collections::HashMap<&str, usize> = ordered_ids
                    .iter()
                    .enumerate()
                    .map(|(index, id)| (id.as_str(), index))
                    .collect();
                file.nodes.sort_by_key(|node| {
                    position
                        .get(node.id.as_str())
                        .copied()
                        .unwrap_or(usize::MAX)
                });
                // 同步地基：排序后按同级分组写 order 字段，order/parent 变化的节点刷新 updatedAt。
                let now = crate::time::now_iso();
                let mut orders: std::collections::HashMap<String, f64> =
                    std::collections::HashMap::new();
                for node in file.nodes.iter_mut() {
                    let group_key = node.parent_id.clone().unwrap_or_default();
                    let next = orders.entry(group_key).or_insert(0.0);
                    let new_order = *next;
                    *next += 1.0;
                    // **换了父节点也算变了**，不能只看 order：从「分类 A 第 3 位」拖到「根级第 3 位」
                    // 时组内序号恰好相同，漏刷 updatedAt 的节点会被同步的 pushed 水位判成「已对账」
                    // 而跳过推送 —— 移动分组在多设备间静默丢失，而同组其它节点却推了出去，
                    // 对端的树因此不自洽。
                    let changed = node.order != new_order || reparented.contains(&node.id);
                    node.order = new_order;
                    if changed {
                        node.updated_at = Some(now.clone());
                    }
                }
                Ok(json!({ "count": ordered_ids.len() }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "applied": true, "revision": outcome.revision }))
        }
        "clear-schedule-output" => {
            let id = required_str(&inv.params, "id")?;
            let (_file, outcome) = ctx.repo.write_schedule(None, None, &inv.command, |file| {
                let entry = file
                    .tasks
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| {
                        CoreError::not_found("SCHEDULE_NOT_FOUND", format!("未找到定时任务 {id}"))
                    })?;
                entry.state.last_exit_code = None;
                entry.state.last_stdout = None;
                entry.state.last_stderr = None;
                Ok(json!({ "id": id }))
            })?;
            meta.revision_domain = Some(Domain::Schedule);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "id": id }))
        }
        "import-state" => {
            let state = inv
                .params
                .get("state")
                .cloned()
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 state"))?;
            let (_file, outcome) = ctx.repo.write_data(None, None, &inv.command, |file| {
                apply_import_state(file, &state)?;
                Ok(json!({ "imported": true }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "imported": true, "revision": outcome.revision }))
        }
        // 链接元数据（超链接自动标题 / 预览卡片）：只读、不碰任何领域文件，
        // 结果缓存在 runtime/linkmeta.json（纯缓存，丢了重抓）。
        "link-meta" => {
            let url = required_str(&inv.params, "url")?;
            let meta = crate::linkmeta::link_meta(&ctx.repo.layout.runtime_dir(), &url)?;
            serde_json::to_value(meta)
                .map_err(|error| CoreError::internal(format!("链接元数据序列化失败：{error}")))
        }
        "link-meta-put" => {
            let url = required_str(&inv.params, "url")?;
            let value = inv.params.get("metadata")
                .ok_or_else(|| CoreError::validation("MISSING_PARAM", "缺少 metadata"))?;
            let metadata: crate::linkmeta::LinkMeta = serde_json::from_value(value.clone())
                .map_err(|_| CoreError::validation("LINK_META_INVALID", "链接元数据格式错误"))?;
            let metadata = crate::linkmeta::put_link_meta(&ctx.repo.layout.runtime_dir(), &url, metadata)?;
            serde_json::to_value(metadata)
                .map_err(|error| CoreError::internal(format!("链接元数据序列化失败：{error}")))
        }
        other => Err(CoreError::validation(
            "UNKNOWN_ACTION",
            format!("未知 gui 动作 `{other}`"),
        )),
    }
}

fn ensure_category_target(data: &DataFile, parent_id: Option<&str>) -> CoreResult<()> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    let node = task_ops::find_node(data, parent_id).ok_or_else(|| {
        CoreError::not_found("CATEGORY_NOT_FOUND", format!("未找到分类 {parent_id}"))
    })?;
    if node.kind != NodeKind::Category {
        return Err(CoreError::validation(
            "INVALID_PARENT",
            format!(
                "父级 {parent_id} 的类型是 {}，只能归属 root 或 category",
                node.kind.as_str()
            ),
        ));
    }
    Ok(())
}

fn would_create_cycle(data: &DataFile, node_id: &str, new_parent: Option<&str>) -> bool {
    let mut current = new_parent.map(str::to_string);
    let mut guard = 0;
    while let Some(id) = current {
        if id == node_id {
            return true;
        }
        guard += 1;
        if guard > data.nodes.len() {
            return true;
        }
        current = task_ops::find_node(data, &id).and_then(|node| node.parent_id.clone());
    }
    false
}

/// Import a full data-domain snapshot (GUI 导入). One transaction.
fn apply_import_state(file: &mut DataFile, state: &Value) -> CoreResult<()> {
    let imported: DataFile = serde_json::from_value(state.clone()).map_err(|error| {
        CoreError::validation("IMPORT_INVALID", format!("导入数据无效：{error}"))
    })?;
    // 同步地基：导入前记录被替换掉的实体墓碑，避免旧数据在下次拉取时复活。
    let now = crate::time::now_iso();
    let new_node_ids: std::collections::HashSet<&str> =
        imported.nodes.iter().map(|node| node.id.as_str()).collect();
    let new_task_ids: std::collections::HashSet<&str> =
        imported.tasks.iter().map(|item| item.id.as_str()).collect();
    for node in &file.nodes {
        if !new_node_ids.contains(node.id.as_str()) {
            file.meta.record_tombstone(&node.id, "node", &now);
        }
    }
    for item in &file.tasks {
        if !new_task_ids.contains(item.id.as_str()) {
            file.meta.record_tombstone(&item.id, "task", &now);
        }
    }
    let mut imported = imported;
    let mut nodes: Vec<crate::model::Node> = Vec::new();
    for sys_id in SYSTEM_NODE_IDS {
        if let Some(node) = imported
            .nodes
            .iter()
            .find(|node| node.id == sys_id && node.kind == NodeKind::System)
        {
            nodes.push(node.clone());
        } else {
            let default = crate::repo::default_data_file();
            let node = default
                .nodes
                .into_iter()
                .find(|node| node.id == sys_id)
                .expect("system node");
            nodes.push(node);
        }
    }
    nodes.extend(
        imported
            .nodes
            .iter()
            .filter(|node| node.kind != NodeKind::System)
            .cloned(),
    );
    // 按（导入文件给定的 order，数组位置）补齐同级 order，保证确定性。
    let mut orders: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for node in nodes.iter_mut() {
        let group_key = node.parent_id.clone().unwrap_or_default();
        let next = orders.entry(group_key).or_insert(0.0);
        node.order = *next;
        *next += 1.0;
    }
    let mut task_orders: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for item in imported.tasks.iter_mut() {
        let next = task_orders.entry(item.node_id.clone()).or_insert(0.0);
        item.order = *next;
        *next += 1.0;
    }
    file.nodes = nodes;
    file.tasks = imported.tasks;
    file.backgrounds = imported.backgrounds;
    let selected = imported.selected_node_id;
    file.selected_node_id = if task_ops::find_node(file, &selected).is_some() {
        selected
    } else {
        file.nodes
            .iter()
            .find(|node| node.kind == NodeKind::Entry)
            .map(|node| node.id.clone())
            .unwrap_or_else(|| SYSTEM_NODE_IDS[0].to_string())
    };
    Ok(())
}
