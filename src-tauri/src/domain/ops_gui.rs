//! gui namespace: GUI-sourced business writes (§4.3; internal, not a CLI surface).

use serde_json::{json, Value};

use crate::domain::core::{ExecContext, Invocation};
use crate::domain::envelope::Meta;
use crate::domain::error::{CoreError, CoreResult};
use crate::domain::model::{DataFile, NodeKind, SYSTEM_NODE_IDS};
use crate::domain::ops_task as task_ops;
use crate::domain::repo::Domain;

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
        "set-item-ui" => {
            let id = required_str(&inv.params, "id")?;
            let expanded = inv.params.get("expanded").and_then(Value::as_bool);
            let editing = inv.params.get("editing").and_then(Value::as_bool);
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
                if let Some(editing) = editing {
                    item.editing = Some(editing);
                }
                Ok(json!({ "id": id }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "id": id, "expanded": expanded, "editing": editing }))
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
                for item in file.tasks.iter_mut().filter(|item| ids.contains(&item.id)) {
                    if let Some(expanded) = expanded {
                        item.expanded = Some(expanded);
                    }
                    item.editing = Some(false);
                }
                Ok(json!({ "count": ids.len() }))
            })?;
            meta.revision_domain = Some(Domain::Data);
            meta.revision = Some(outcome.revision);
            Ok(json!({ "updated": ids.len() }))
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
                    node.parent_id = parent_id;
                }
                let position: std::collections::HashMap<&str, usize> = ordered_ids
                    .iter()
                    .enumerate()
                    .map(|(index, id)| (id.as_str(), index))
                    .collect();
                file.nodes.sort_by_key(|node| {
                    position.get(node.id.as_str()).copied().unwrap_or(usize::MAX)
                });
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
            format!("父级 {parent_id} 的类型是 {}，只能归属 root 或 category", node.kind.as_str()),
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
    let mut nodes: Vec<crate::domain::model::Node> = Vec::new();
    for sys_id in SYSTEM_NODE_IDS {
        if let Some(node) = imported
            .nodes
            .iter()
            .find(|node| node.id == sys_id && node.kind == NodeKind::System)
        {
            nodes.push(node.clone());
        } else {
            let default = crate::domain::repo::default_data_file();
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
            .into_iter()
            .filter(|node| node.kind != NodeKind::System),
    );
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
