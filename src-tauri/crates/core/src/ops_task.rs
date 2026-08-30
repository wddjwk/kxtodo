//! task domain: category / entry / item business operations (§3.4).

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::{CoreError, CoreResult};
use crate::ids::{gen_id, is_reserved_id};
use crate::model::{DataFile, Item, Node, NodeKind, Tag, TagColor, SYSTEM_NODE_IDS};
use crate::time::{now_iso, parse_date};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub fn find_node<'a>(data: &'a DataFile, id: &str) -> Option<&'a Node> {
    data.nodes.iter().find(|node| node.id == id)
}

pub fn find_node_mut<'a>(data: &'a mut DataFile, id: &str) -> Option<&'a mut Node> {
    data.nodes.iter_mut().find(|node| node.id == id)
}

pub fn find_item<'a>(data: &'a DataFile, id: &str) -> Option<&'a Item> {
    data.tasks.iter().find(|item| item.id == id)
}

pub fn require_node_kind(node: &Node, expected: NodeKind, type_flag: &str) -> CoreResult<()> {
    if node.kind != expected {
        return Err(CoreError::conflict(
            "TYPE_MISMATCH",
            format!(
                "对象 {} 的实际类型是 {}，与 --type {} 不符",
                node.id,
                node.kind.as_str(),
                type_flag
            ),
        ));
    }
    Ok(())
}

pub fn get_node_typed<'a>(
    data: &'a DataFile,
    id: &str,
    expected: NodeKind,
) -> CoreResult<&'a Node> {
    let node = find_node(data, id).ok_or_else(|| {
        CoreError::not_found(
            "NODE_NOT_FOUND",
            format!("未找到 {type} {id}", type = expected.as_str(), id = id),
        )
        .with_hint("先运行 kxtodo-cli task find 或 task tree 获取稳定 ID")
    })?;
    require_node_kind(node, expected, expected.as_str())?;
    Ok(node)
}

pub fn get_item_typed<'a>(data: &'a DataFile, id: &str) -> CoreResult<&'a Item> {
    find_item(data, id).ok_or_else(|| {
        CoreError::not_found("TASK_NOT_FOUND", format!("未找到任务 {id}"))
            .with_hint("先运行 kxtodo-cli task find --type item --query ...")
    })
}

fn ensure_mutable_node(node: &Node) -> CoreResult<()> {
    if node.kind == NodeKind::System {
        return Err(CoreError::validation(
            "SYSTEM_NODE_READONLY",
            format!("系统节点 {} 为内置视图，不允许增删改", node.id),
        ));
    }
    Ok(())
}

/// Ancestor chain from root to the node's parent (ids), excluding the node itself.
pub fn ancestor_ids(data: &DataFile, node: &Node) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = node.parent_id.clone();
    let mut guard = 0;
    while let Some(id) = current {
        if guard > data.nodes.len() {
            break;
        }
        guard += 1;
        let Some(parent) = find_node(data, &id) else {
            break;
        };
        chain.push(parent.id.clone());
        current = parent.parent_id.clone();
    }
    chain.reverse();
    chain
}

pub fn node_path(data: &DataFile, node: &Node) -> String {
    let mut names: Vec<String> = ancestor_ids(data, node)
        .iter()
        .filter_map(|id| find_node(data, id))
        .map(|ancestor| ancestor.name.clone())
        .collect();
    names.push(node.name.clone());
    names.join(" / ")
}

pub fn descendant_node_ids(data: &DataFile, root_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root_id.to_string()];
    while let Some(id) = stack.pop() {
        for node in data
            .nodes
            .iter()
            .filter(|node| node.parent_id.as_deref() == Some(id.as_str()))
        {
            out.push(node.id.clone());
            stack.push(node.id.clone());
        }
    }
    out
}

pub fn child_ids(data: &DataFile, parent: Option<&str>) -> Vec<String> {
    data.nodes
        .iter()
        .filter(|node| node.parent_id.as_deref() == parent)
        .map(|node| node.id.clone())
        .collect()
}

pub fn orphan_item_ids(data: &DataFile) -> Vec<String> {
    data.tasks
        .iter()
        .filter(|item| {
            find_node(data, &item.node_id)
                .map(|node| node.kind != NodeKind::Entry)
                .unwrap_or(true)
        })
        .map(|item| item.id.clone())
        .collect()
}

fn ensure_entry_target(data: &DataFile, entry_id: &str) -> CoreResult<()> {
    let node = find_node(data, entry_id).ok_or_else(|| {
        CoreError::not_found("ENTRY_NOT_FOUND", format!("未找到条目 {entry_id}"))
            .with_hint("先运行 kxtodo-cli task find --type entry --query ...")
    })?;
    if node.kind != NodeKind::Entry {
        return Err(CoreError::validation(
            "INVALID_ENTRY",
            format!(
                "{entry_id} 的类型是 {}，item 只能归属 entry",
                node.kind.as_str()
            ),
        ));
    }
    Ok(())
}

fn ensure_category_target(data: &DataFile, parent_id: Option<&str>) -> CoreResult<()> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    if is_reserved_id(parent_id) {
        return Ok(());
    }
    let node = find_node(data, parent_id).ok_or_else(|| {
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
        current = find_node(data, &id).and_then(|node| node.parent_id.clone());
    }
    false
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AddNodeParams {
    pub name: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
    pub collapsed: Option<bool>,
}

pub fn add_node(data: &mut DataFile, kind: NodeKind, params: AddNodeParams) -> CoreResult<Node> {
    let name = params.name.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::validation("NAME_REQUIRED", "--name 不能为空"));
    }
    let parent_id = normalize_parent(params.parent_id.as_deref());
    ensure_category_target(data, parent_id.as_deref())?;
    let now = now_iso();
    let prefix = match kind {
        NodeKind::Category => "category",
        NodeKind::Entry => "entry",
        NodeKind::System => {
            return Err(CoreError::validation(
                "SYSTEM_NODE_READONLY",
                "系统节点不允许创建",
            ))
        }
    };
    let node = Node {
        id: gen_id(prefix),
        kind,
        name,
        icon: params
            .icon
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| match kind {
                NodeKind::Category => "folder".to_string(),
                _ => "notebook".to_string(),
            }),
        parent_id,
        collapsed: match kind {
            NodeKind::Category => Some(params.collapsed.unwrap_or(false)),
            _ => params.collapsed,
        },
        created_at: now.clone(),
        updated_at: Some(now),
        extra: Map::new(),
    };
    data.nodes.push(node.clone());
    Ok(node)
}

fn normalize_parent(parent_id: Option<&str>) -> Option<String> {
    match parent_id {
        None => None,
        Some("root") => None,
        Some(value) => Some(value.to_string()),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TagInput {
    pub color: TagColor,
    pub text: Option<String>,
}

pub fn parse_tag_input(raw: &str) -> CoreResult<TagInput> {
    let (color_raw, text) = match raw.split_once(':') {
        Some((color, text)) => (color, Some(text.trim().to_string())),
        None => (raw, None),
    };
    let color = TagColor::parse(color_raw).ok_or_else(|| {
        CoreError::validation(
            "INVALID_TAG_COLOR",
            format!("无效标签颜色 `{color_raw}`，支持 red/yellow/blue/green/gray"),
        )
    })?;
    let text = text.filter(|value| !value.is_empty());
    Ok(TagInput { color, text })
}

fn build_tag(input: &TagInput) -> Tag {
    Tag {
        id: gen_id("tag"),
        color: input.color,
        text: input
            .text
            .clone()
            .map(|value| value.chars().take(20).collect()),
        extra: Map::new(),
    }
}

#[derive(Debug, Default)]
pub struct AddItemParams {
    pub entry_id: String,
    pub markdown: String,
    pub completed: bool,
    pub important: bool,
    pub my_day: bool,
    pub planned_date: Option<String>,
    pub due_date: Option<String>,
    pub tags: Vec<TagInput>,
    pub emojis: Vec<String>,
}

pub fn add_item(data: &mut DataFile, params: AddItemParams) -> CoreResult<Item> {
    if params.entry_id.trim().is_empty() {
        return Err(CoreError::validation("ENTRY_REQUIRED", "--entry-id 必填"));
    }
    ensure_entry_target(data, &params.entry_id)?;
    let markdown = params.markdown;
    if markdown.trim().is_empty() {
        return Err(CoreError::validation(
            "MARKDOWN_REQUIRED",
            "--markdown / --markdown-file 必须提供非空内容",
        ));
    }
    let now = now_iso();
    let completed = params.completed;
    let item = Item {
        id: gen_id("task"),
        node_id: params.entry_id,
        markdown,
        completed,
        important: params.important,
        my_day: params.my_day,
        planned_date: params.planned_date,
        due_date: params.due_date,
        completed_at: if completed { Some(now.clone()) } else { None },
        tags: params.tags.iter().map(build_tag).collect(),
        emojis: params.emojis,
        expanded: None,
        created_at: now.clone(),
        updated_at: Some(now),
        extra: Map::new(),
    };
    data.tasks.push(item.clone());
    Ok(item)
}

// ---------------------------------------------------------------------------
// views
// ---------------------------------------------------------------------------

pub fn node_view(data: &DataFile, node: &Node, with_counts: bool) -> Value {
    let mut view = json!({
        "id": node.id,
        "type": node.kind.as_str(),
        "name": node.name,
        "icon": node.icon,
        "parentId": node.parent_id,
        "path": node_path(data, node),
        "createdAt": node.created_at,
    });
    if let Some(collapsed) = node.collapsed {
        view["collapsed"] = json!(collapsed);
    }
    if let Some(updated) = &node.updated_at {
        view["updatedAt"] = json!(updated);
    }
    if with_counts {
        view["counts"] = node_counts(data, node);
    }
    view
}

pub fn node_counts(data: &DataFile, node: &Node) -> Value {
    match node.kind {
        NodeKind::Entry => {
            let direct = data
                .tasks
                .iter()
                .filter(|item| item.node_id == node.id)
                .count();
            json!({ "directItems": direct, "recursiveItems": direct })
        }
        NodeKind::Category => {
            let descendants = descendant_node_ids(data, &node.id);
            let entry_ids: Vec<&String> = descendants
                .iter()
                .filter(|id| {
                    find_node(data, id)
                        .map(|candidate| candidate.kind == NodeKind::Entry)
                        .unwrap_or(false)
                })
                .collect();
            let recursive_items = data
                .tasks
                .iter()
                .filter(|item| entry_ids.iter().any(|id| **id == item.node_id))
                .count();
            json!({
                "directChildren": child_ids(data, Some(&node.id)).len(),
                "recursiveNodes": descendants.len(),
                "recursiveItems": recursive_items,
            })
        }
        NodeKind::System => {
            let count = match node.id.as_str() {
                "my-day" => data
                    .tasks
                    .iter()
                    .filter(|item| item.my_day && !item.completed)
                    .count(),
                "planned" => data
                    .tasks
                    .iter()
                    .filter(|item| item.planned_date.is_some() && !item.completed)
                    .count(),
                "important" => data
                    .tasks
                    .iter()
                    .filter(|item| item.important && !item.completed)
                    .count(),
                _ => 0,
            };
            json!({ "items": count })
        }
    }
}

pub fn item_view(data: &DataFile, item: &Item) -> Value {
    let entry = find_node(data, &item.node_id);
    let mut view = json!({
        "id": item.id,
        "type": "item",
        "nodeId": item.node_id,
        "markdown": item.markdown,
        "completed": item.completed,
        "important": item.important,
        "myDay": item.my_day,
        "tags": item.tags,
        "emojis": item.emojis,
        "createdAt": item.created_at,
    });
    if let Some(value) = &item.planned_date {
        view["plannedDate"] = json!(value);
    }
    if let Some(value) = &item.due_date {
        view["dueDate"] = json!(value);
    }
    if let Some(value) = &item.completed_at {
        view["completedAt"] = json!(value);
    }
    if let Some(value) = &item.updated_at {
        view["updatedAt"] = json!(value);
    }
    match entry {
        Some(entry) => {
            let path = node_path(data, entry);
            let ancestors: Vec<Value> = ancestor_ids(data, entry)
                .iter()
                .filter_map(|id| find_node(data, id))
                .map(|ancestor| {
                    json!({
                        "id": ancestor.id,
                        "type": ancestor.kind.as_str(),
                        "name": ancestor.name,
                    })
                })
                .collect();
            view["entry"] = json!({
                "id": entry.id,
                "name": entry.name,
                "path": path,
            });
            view["path"] = json!(path);
            view["ancestors"] = json!(ancestors);
        }
        None => {
            view["integrityIssue"] = json!(format!("item 归属的 entry {} 不存在", item.node_id));
        }
    }
    view
}

// ---------------------------------------------------------------------------
// list / find
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    Open,
    Completed,
    #[default]
    All,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum SortKey {
    CreatedAt,
    #[default]
    UpdatedAt,
    DueDate,
    CompletedAt,
    Name,
    Position,
}

#[derive(Debug, Default)]
pub struct ItemFilter {
    pub entry_id: Option<String>,
    pub category_id: Option<String>,
    pub recursive: bool,
    pub status: StatusFilter,
    pub important: Option<bool>,
    pub my_day: Option<bool>,
    pub tags: Vec<String>,
    pub created_from: Option<String>,
    pub created_to: Option<String>,
    pub updated_from: Option<String>,
    pub updated_to: Option<String>,
    pub changed_from: Option<String>,
    pub changed_to: Option<String>,
    pub planned_from: Option<String>,
    pub planned_to: Option<String>,
    pub due_from: Option<String>,
    pub due_to: Option<String>,
    pub completed_from: Option<String>,
    pub completed_to: Option<String>,
    pub query: Option<String>,
    pub sort: SortKey,
    pub descending: bool,
}

fn instant_in_range(value: Option<&str>, from_ms: Option<i64>, to_ms: Option<i64>) -> bool {
    let Some(raw) = value else {
        return from_ms.is_none() && to_ms.is_none();
    };
    let Ok(ms) = crate::time::instant_millis(raw) else {
        return false;
    };
    if let Some(from) = from_ms {
        if ms < from {
            return false;
        }
    }
    if let Some(to) = to_ms {
        if ms > to {
            return false;
        }
    }
    true
}

/// Parse a range bound: full ISO instant, or `YYYY-MM-DD` (from → local start of day,
/// to → local end of day).
pub fn parse_range_bound(raw: &str, is_to: bool) -> CoreResult<i64> {
    let value = raw.trim();
    if value.len() == 10 && value.chars().nth(4) == Some('-') {
        let date = parse_date(value)?;
        let naive = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map_err(|_| CoreError::validation("INVALID_DATE", format!("无效日期 `{value}`")))?;
        let time = if is_to {
            naive.and_hms_milli_opt(23, 59, 59, 999)
        } else {
            naive.and_hms_milli_opt(0, 0, 0, 0)
        }
        .ok_or_else(|| CoreError::validation("INVALID_DATE", format!("无效日期 `{value}`")))?;
        let local = chrono::Local;
        use chrono::TimeZone;
        let at = local
            .from_local_datetime(&time)
            .single()
            .ok_or_else(|| CoreError::validation("INVALID_DATE", format!("无效日期 `{value}`")))?;
        return Ok(at.timestamp_millis());
    }
    crate::time::instant_millis(value)
}

fn opt_ms(value: &Option<String>, is_to: bool) -> CoreResult<Option<i64>> {
    value
        .as_deref()
        .map(|raw| parse_range_bound(raw, is_to))
        .transpose()
}

pub fn filter_items<'a>(data: &'a DataFile, filter: &ItemFilter) -> CoreResult<Vec<&'a Item>> {
    let mut scope_entry_ids: Option<Vec<String>> = None;
    if let Some(entry_id) = &filter.entry_id {
        ensure_entry_target(data, entry_id)?;
        scope_entry_ids = Some(vec![entry_id.clone()]);
    } else if let Some(category_id) = &filter.category_id {
        let category = get_node_typed(data, category_id, NodeKind::Category)?;
        let mut ids = if filter.recursive {
            descendant_node_ids(data, &category.id)
        } else {
            child_ids(data, Some(&category.id))
        };
        ids.retain(|id| {
            find_node(data, id)
                .map(|node| node.kind == NodeKind::Entry)
                .unwrap_or(false)
        });
        scope_entry_ids = Some(ids);
    }

    let created_from = opt_ms(&filter.created_from, false)?;
    let created_to = opt_ms(&filter.created_to, true)?;
    let updated_from = opt_ms(&filter.updated_from, false)?;
    let updated_to = opt_ms(&filter.updated_to, true)?;
    let changed_from = opt_ms(&filter.changed_from, false)?;
    let changed_to = opt_ms(&filter.changed_to, true)?;
    let completed_from = opt_ms(&filter.completed_from, false)?;
    let completed_to = opt_ms(&filter.completed_to, true)?;

    let planned_from = filter.planned_from.as_deref().map(parse_date).transpose()?;
    let planned_to = filter.planned_to.as_deref().map(parse_date).transpose()?;
    let due_from = filter.due_from.as_deref().map(parse_date).transpose()?;
    let due_to = filter.due_to.as_deref().map(parse_date).transpose()?;

    let query = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);

    let mut items: Vec<&Item> = data
        .tasks
        .iter()
        .filter(|item| {
            if let Some(scope) = &scope_entry_ids {
                if !scope.contains(&item.node_id) {
                    return false;
                }
            }
            match filter.status {
                StatusFilter::Open if item.completed => return false,
                StatusFilter::Completed if !item.completed => return false,
                _ => {}
            }
            if let Some(important) = filter.important {
                if item.important != important {
                    return false;
                }
            }
            if let Some(my_day) = filter.my_day {
                if item.my_day != my_day {
                    return false;
                }
            }
            for tag_filter in &filter.tags {
                let matches = item.tags.iter().any(|tag| {
                    tag.id == *tag_filter
                        || tag.text.as_deref().map(str::to_lowercase)
                            == Some(tag_filter.to_lowercase())
                });
                if !matches {
                    return false;
                }
            }
            if !instant_in_range(Some(&item.created_at), created_from, created_to) {
                return false;
            }
            if !instant_in_range(item.updated_at.as_deref(), updated_from, updated_to) {
                return false;
            }
            if changed_from.is_some() || changed_to.is_some() {
                let created_ok = instant_in_range(Some(&item.created_at), changed_from, changed_to);
                let updated_ok =
                    instant_in_range(item.updated_at.as_deref(), changed_from, changed_to);
                if !created_ok && !updated_ok {
                    return false;
                }
            }
            if !instant_in_range(item.completed_at.as_deref(), completed_from, completed_to) {
                return false;
            }
            if planned_from.is_some() || planned_to.is_some() {
                let Some(planned) = &item.planned_date else {
                    return false;
                };
                if let Some(from) = &planned_from {
                    if planned < from {
                        return false;
                    }
                }
                if let Some(to) = &planned_to {
                    if planned > to {
                        return false;
                    }
                }
            }
            if due_from.is_some() || due_to.is_some() {
                let Some(due) = &item.due_date else {
                    return false;
                };
                if let Some(from) = &due_from {
                    if due < from {
                        return false;
                    }
                }
                if let Some(to) = &due_to {
                    if due > to {
                        return false;
                    }
                }
            }
            if let Some(query) = &query {
                let in_markdown = item.markdown.to_lowercase().contains(query);
                let in_tags = item.tags.iter().any(|tag| {
                    tag.text
                        .as_deref()
                        .map(str::to_lowercase)
                        .map(|text| text.contains(query))
                        .unwrap_or(false)
                });
                if !in_markdown && !in_tags {
                    return false;
                }
            }
            true
        })
        .collect();

    let position_of = |item: &Item| {
        data.tasks
            .iter()
            .position(|candidate| candidate.id == item.id)
            .unwrap_or(usize::MAX)
    };
    items.sort_by(|a, b| {
        let ordering = match filter.sort {
            SortKey::CreatedAt => a.created_at.cmp(&b.created_at),
            SortKey::UpdatedAt => a
                .updated_at
                .as_deref()
                .unwrap_or("")
                .cmp(b.updated_at.as_deref().unwrap_or("")),
            SortKey::DueDate => a
                .due_date
                .as_deref()
                .unwrap_or("")
                .cmp(b.due_date.as_deref().unwrap_or("")),
            SortKey::CompletedAt => a
                .completed_at
                .as_deref()
                .unwrap_or("")
                .cmp(b.completed_at.as_deref().unwrap_or("")),
            SortKey::Name => a.markdown.cmp(&b.markdown),
            SortKey::Position => position_of(a).cmp(&position_of(b)),
        };
        if filter.descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    Ok(items)
}

pub struct Page {
    pub offset: usize,
    pub limit: usize,
    pub all: bool,
}

pub const DEFAULT_PAGE_LIMIT: usize = 50;

pub fn paginate<T>(items: Vec<T>, page: &Page) -> (Vec<T>, Option<String>, usize) {
    let total = items.len();
    if page.all {
        return (items, None, total);
    }
    let start = page.offset.min(total);
    let end = (start + page.limit).min(total);
    let next_cursor = if end < total {
        Some(end.to_string())
    } else {
        None
    };
    (
        items.into_iter().skip(start).take(end - start).collect(),
        next_cursor,
        total,
    )
}

pub fn parse_cursor(raw: Option<&str>) -> CoreResult<usize> {
    match raw {
        None => Ok(0),
        Some(value) => value.parse::<usize>().map_err(|_| {
            CoreError::validation("INVALID_CURSOR", format!("无效分页游标 `{value}`"))
        }),
    }
}

// ---------------------------------------------------------------------------
// node listing / search
// ---------------------------------------------------------------------------

pub fn list_nodes<'a>(
    data: &'a DataFile,
    kind: NodeKind,
    parent_id: Option<&str>,
    recursive: bool,
) -> CoreResult<Vec<&'a Node>> {
    let parent = normalize_parent(parent_id);
    ensure_category_target(data, parent.as_deref())?;
    let ids: Vec<String> = match (&parent, recursive) {
        (Some(parent), true) => descendant_node_ids(data, parent),
        (Some(parent), false) => child_ids(data, Some(parent)),
        (None, true) => data.nodes.iter().map(|node| node.id.clone()).collect(),
        (None, false) => child_ids(data, None),
    };
    Ok(ids
        .iter()
        .filter_map(|id| find_node(data, id))
        .filter(|node| node.kind == kind)
        .collect())
}

pub fn find_nodes<'a>(data: &'a DataFile, query: &str, kind: Option<NodeKind>) -> Vec<&'a Node> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    data.nodes
        .iter()
        .filter(|node| kind.map(|expected| node.kind == expected).unwrap_or(true))
        .filter(|node| {
            node.name.to_lowercase().contains(&needle)
                || node_path(data, node).to_lowercase().contains(&needle)
                || node.id.to_lowercase() == needle
        })
        .collect()
}

// ---------------------------------------------------------------------------
// modify
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct NodeChanges {
    pub name: Option<String>,
    pub icon: Option<String>,
    /// Some(None) → move to root.
    pub parent_id: Option<Option<String>>,
    pub collapsed: Option<bool>,
}

pub fn modify_node(
    data: &mut DataFile,
    id: &str,
    kind: NodeKind,
    changes: NodeChanges,
) -> CoreResult<Node> {
    let (kind_now, old_parent) = {
        let node = find_node(data, id).ok_or_else(|| {
            CoreError::not_found("NODE_NOT_FOUND", format!("未找到 {} {id}", kind.as_str()))
        })?;
        require_node_kind(node, kind, kind.as_str())?;
        ensure_mutable_node(node)?;
        (node.kind, node.parent_id.clone())
    };
    let _ = kind_now;

    if let Some(new_parent) = &changes.parent_id {
        let normalized = normalize_parent(new_parent.as_deref());
        ensure_category_target(data, normalized.as_deref())?;
        if kind == NodeKind::Category && would_create_cycle(data, id, normalized.as_deref()) {
            return Err(CoreError::conflict(
                "CATEGORY_CYCLE",
                "移动会形成分类循环，已拒绝",
            ));
        }
        if normalized != old_parent {
            let now = now_iso();
            let node = find_node_mut(data, id).expect("node checked above");
            node.parent_id = normalized;
            node.updated_at = Some(now);
        }
    }

    let now = now_iso();
    let node = find_node_mut(data, id).expect("node checked above");
    let mut touched = false;
    if let Some(name) = changes.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(CoreError::validation("NAME_REQUIRED", "--name 不能为空"));
        }
        node.name = name;
        touched = true;
    }
    if let Some(icon) = changes.icon {
        node.icon = icon.trim().to_string();
        touched = true;
    }
    if let Some(collapsed) = changes.collapsed {
        if kind == NodeKind::Category {
            node.collapsed = Some(collapsed);
        } else {
            node.collapsed = Some(collapsed);
        }
        touched = true;
    }
    if touched {
        node.updated_at = Some(now);
    }
    Ok(node.clone())
}

#[derive(Debug, Default)]
pub struct ItemChanges {
    pub entry_id: Option<String>,
    pub markdown: Option<String>,
    pub completed: Option<bool>,
    pub important: Option<bool>,
    pub my_day: Option<bool>,
    pub planned_date: Option<Option<String>>,
    pub due_date: Option<Option<String>>,
    pub add_tags: Vec<TagInput>,
    pub remove_tag_ids: Vec<String>,
    pub replace_tags: Option<Vec<TagInput>>,
    pub add_emojis: Vec<String>,
    pub remove_emojis: Vec<String>,
    pub replace_emojis: Option<Vec<String>>,
}

pub fn modify_item(data: &mut DataFile, id: &str, changes: ItemChanges) -> CoreResult<Item> {
    if !data.tasks.iter().any(|item| item.id == id) {
        return Err(
            CoreError::not_found("TASK_NOT_FOUND", format!("未找到任务 {id}"))
                .with_hint("先运行 kxtodo-cli task find --type item --query ..."),
        );
    }
    if let Some(entry_id) = &changes.entry_id {
        ensure_entry_target(data, entry_id)?;
    }
    let now = now_iso();
    let item = data
        .tasks
        .iter_mut()
        .find(|item| item.id == id)
        .expect("item checked above");
    let mut touched = false;

    if let Some(entry_id) = changes.entry_id {
        item.node_id = entry_id;
        touched = true;
    }
    if let Some(markdown) = changes.markdown {
        if markdown.trim().is_empty() {
            return Err(CoreError::validation(
                "MARKDOWN_REQUIRED",
                "Markdown 内容不能为空",
            ));
        }
        item.markdown = markdown;
        touched = true;
    }
    if let Some(completed) = changes.completed {
        if completed != item.completed {
            item.completed = completed;
            item.completed_at = if completed { Some(now.clone()) } else { None };
            touched = true;
        }
    }
    if let Some(important) = changes.important {
        item.important = important;
        touched = true;
    }
    if let Some(my_day) = changes.my_day {
        item.my_day = my_day;
        touched = true;
    }
    if let Some(planned) = changes.planned_date {
        item.planned_date = planned;
        touched = true;
    }
    if let Some(due) = changes.due_date {
        item.due_date = due;
        touched = true;
    }
    if let Some(tags) = changes.replace_tags {
        item.tags = tags.iter().map(build_tag).collect();
        touched = true;
    } else {
        if !changes.remove_tag_ids.is_empty() {
            let before = item.tags.len();
            item.tags
                .retain(|tag| !changes.remove_tag_ids.contains(&tag.id));
            if item.tags.len() != before {
                touched = true;
            }
        }
        for input in &changes.add_tags {
            item.tags.push(build_tag(input));
            touched = true;
        }
    }
    if let Some(emojis) = changes.replace_emojis {
        item.emojis = emojis;
        touched = true;
    } else {
        if !changes.remove_emojis.is_empty() {
            let before = item.emojis.len();
            item.emojis
                .retain(|emoji| !changes.remove_emojis.contains(emoji));
            if item.emojis.len() != before {
                touched = true;
            }
        }
        for emoji in changes.add_emojis {
            if !emoji.trim().is_empty() && !item.emojis.contains(&emoji) {
                item.emojis.push(emoji);
                touched = true;
            }
        }
    }
    if touched {
        item.updated_at = Some(now);
    }
    Ok(item.clone())
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RemovePlan {
    pub node_ids: Vec<String>,
    pub item_ids: Vec<String>,
    pub image_dirs: Vec<String>,
    pub background_keys: Vec<String>,
}

pub fn plan_remove_node(data: &DataFile, id: &str, kind: NodeKind) -> CoreResult<RemovePlan> {
    let node = find_node(data, id).ok_or_else(|| {
        CoreError::not_found("NODE_NOT_FOUND", format!("未找到 {} {id}", kind.as_str()))
    })?;
    require_node_kind(node, kind, kind.as_str())?;
    ensure_mutable_node(node)?;

    let mut node_ids = vec![id.to_string()];
    if kind == NodeKind::Category {
        node_ids.extend(descendant_node_ids(data, id));
    }
    let entry_ids: Vec<String> = node_ids
        .iter()
        .filter(|node_id| {
            find_node(data, node_id)
                .map(|candidate| candidate.kind == NodeKind::Entry)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let item_ids: Vec<String> = data
        .tasks
        .iter()
        .filter(|item| entry_ids.contains(&item.node_id))
        .map(|item| item.id.clone())
        .collect();
    let image_dirs = entry_ids.clone();
    let background_keys = node_ids
        .iter()
        .filter(|node_id| data.backgrounds.contains_key(*node_id))
        .cloned()
        .collect();
    Ok(RemovePlan {
        node_ids,
        item_ids,
        image_dirs,
        background_keys,
    })
}

pub fn non_empty_remove_error(data: &DataFile, id: &str, kind: NodeKind) -> Option<CoreError> {
    let plan = plan_remove_node(data, id, kind).ok()?;
    let children = plan.node_ids.len() - 1;
    let items = plan.item_ids.len();
    if children > 0 || items > 0 {
        return Some(
            CoreError::conflict(
                "NOT_EMPTY",
                format!(
                    "{} {id} 非空：包含 {children} 个后代节点、{items} 个任务",
                    kind.as_str()
                ),
            )
            .with_hint("确认后加 --cascade --yes 级联删除，或先运行 --dry-run 查看影响范围"),
        );
    }
    None
}

/// Apply a cascade removal (nodes + items + backgrounds). Image dirs removed by caller.
pub fn apply_remove_node(data: &mut DataFile, plan: &RemovePlan) -> CoreResult<()> {
    data.nodes.retain(|node| !plan.node_ids.contains(&node.id));
    data.tasks.retain(|item| !plan.item_ids.contains(&item.id));
    for key in &plan.background_keys {
        data.backgrounds.remove(key);
    }
    if plan.node_ids.iter().any(|id| *id == data.selected_node_id) {
        data.selected_node_id = data
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::Entry)
            .map(|node| node.id.clone())
            .unwrap_or_else(|| SYSTEM_NODE_IDS[0].to_string());
    }
    Ok(())
}

pub fn remove_item(data: &mut DataFile, id: &str) -> CoreResult<Item> {
    let index = data
        .tasks
        .iter()
        .position(|item| item.id == id)
        .ok_or_else(|| CoreError::not_found("TASK_NOT_FOUND", format!("未找到任务 {id}")))?;
    Ok(data.tasks.remove(index))
}

// ---------------------------------------------------------------------------
// tree
// ---------------------------------------------------------------------------

pub fn tree_view(
    data: &DataFile,
    root_id: Option<&str>,
    depth: usize,
    include_counts: bool,
) -> CoreResult<Value> {
    fn build_node(
        data: &DataFile,
        id: &str,
        level: usize,
        depth: usize,
        include_counts: bool,
    ) -> Option<Value> {
        let node = find_node(data, id)?;
        let mut view = json!({
            "id": node.id,
            "type": node.kind.as_str(),
            "name": node.name,
            "icon": node.icon,
            "path": node_path(data, node),
        });
        if let Some(collapsed) = node.collapsed {
            view["collapsed"] = json!(collapsed);
        }
        if include_counts {
            view["counts"] = node_counts(data, node);
        }
        if node.kind == NodeKind::Category && level < depth {
            let children: Vec<Value> = child_ids(data, Some(&node.id))
                .into_iter()
                .filter_map(|child| build_node(data, &child, level + 1, depth, include_counts))
                .collect();
            view["children"] = json!(children);
        }
        Some(view)
    }

    let roots = match root_id {
        Some(id) => {
            if find_node(data, id).is_none() {
                return Err(CoreError::not_found(
                    "NODE_NOT_FOUND",
                    format!("未找到节点 {id}"),
                ));
            }
            vec![build_node(data, id, 0, depth, include_counts).expect("node checked above")]
        }
        None => child_ids(data, None)
            .into_iter()
            .filter_map(|id| build_node(data, &id, 0, depth, include_counts))
            .collect(),
    };
    Ok(json!({ "roots": roots, "depth": depth }))
}
