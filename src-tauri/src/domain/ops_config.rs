//! config domain: dot-path settings management (§3.6).

use serde_json::{json, Map, Value};

use crate::domain::error::{CoreError, CoreResult};
use crate::domain::model::{
    CloudProvider, LinkOpenMode, NotificationPosition, SettingsFile, ThemePreset,
};

#[derive(Debug, Clone)]
pub struct FieldMeta {
    pub path: &'static str,
    pub kind: &'static str,
    pub description: &'static str,
    pub is_map: bool,
}

pub const KNOWN_FIELDS: &[FieldMeta] = &[
    FieldMeta {
        path: "profile.displayName",
        kind: "string",
        description: "用户显示名",
        is_map: false,
    },
    FieldMeta {
        path: "profile.email",
        kind: "string",
        description: "用户邮箱",
        is_map: false,
    },
    FieldMeta {
        path: "profile.avatar",
        kind: "string",
        description: "头像（文件名或 data URL）",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.linkOpenMode",
        kind: "enum(app|system)",
        description: "链接打开方式",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.uiScale",
        kind: "number(0.5-1.5)",
        description: "界面缩放",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.uiFontSize",
        kind: "integer(14-22)",
        description: "界面字号",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.markdownFontSize",
        kind: "integer(14-26)",
        description: "Markdown 字号",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.editorFontSize",
        kind: "integer(14-26)",
        description: "编辑器字号",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.tagFontSize",
        kind: "integer(11-30)",
        description: "标签字号",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.themePresets",
        kind: "array<{name,color}>",
        description: "配色盘预设",
        is_map: false,
    },
    FieldMeta {
        path: "appearance.uiColors",
        kind: "map<entryId,color>",
        description: "条目自定义颜色（需 --map-key）",
        is_map: true,
    },
    FieldMeta {
        path: "lifecycle.closeToTray",
        kind: "boolean",
        description: "关闭时最小化到托盘",
        is_map: false,
    },
    FieldMeta {
        path: "lifecycle.launchAtStartup",
        kind: "boolean",
        description: "开机自启（默认数据目录）",
        is_map: false,
    },
    FieldMeta {
        path: "notifications.durationMs",
        kind: "integer(1200-60000)",
        description: "通知默认时长（毫秒）",
        is_map: false,
    },
    FieldMeta {
        path: "notifications.position",
        kind: "enum(bottom-right|top-right|bottom-left|top-left)",
        description: "通知默认位置",
        is_map: false,
    },
    FieldMeta {
        path: "notifications.width",
        kind: "integer(280-600)",
        description: "通知窗口宽度",
        is_map: false,
    },
    FieldMeta {
        path: "notifications.height",
        kind: "integer(50-200)",
        description: "通知窗口高度",
        is_map: false,
    },
    FieldMeta {
        path: "notifications.titleFontSize",
        kind: "integer(10-24)",
        description: "通知标题字号",
        is_map: false,
    },
    FieldMeta {
        path: "notifications.bodyFontSize",
        kind: "integer(8-20)",
        description: "通知正文字号",
        is_map: false,
    },
    FieldMeta {
        path: "shortcuts.newTask",
        kind: "string",
        description: "新建任务快捷键",
        is_map: false,
    },
    FieldMeta {
        path: "shortcuts.focusSearch",
        kind: "string",
        description: "搜索快捷键",
        is_map: false,
    },
    FieldMeta {
        path: "shortcuts.toggleWindow",
        kind: "string",
        description: "全局唤起快捷键",
        is_map: false,
    },
    FieldMeta {
        path: "shortcuts.openSettings",
        kind: "string",
        description: "打开设置快捷键",
        is_map: false,
    },
    FieldMeta {
        path: "cloud.provider",
        kind: "enum(none|webdav|s3|custom)",
        description: "云同步提供商",
        is_map: false,
    },
    FieldMeta {
        path: "cloud.endpoint",
        kind: "string",
        description: "云同步端点",
        is_map: false,
    },
    FieldMeta {
        path: "cloud.enabled",
        kind: "boolean",
        description: "启用云同步",
        is_map: false,
    },
];

pub fn field_meta(path: &str) -> Option<&'static FieldMeta> {
    KNOWN_FIELDS.iter().find(|field| field.path == path)
}

fn unknown_field(path: &str) -> CoreError {
    CoreError::validation("UNKNOWN_CONFIG_KEY", format!("未知配置项 `{path}`"))
        .with_hint("运行 kxtodo config list 查看全部已知配置项")
}

fn is_hex_color(raw: &str) -> bool {
    let value = raw.trim();
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

fn invalid_value(path: &str, reason: impl Into<String>) -> CoreError {
    CoreError::validation(
        "INVALID_CONFIG_VALUE",
        format!("配置 {path} 无效：{}", reason.into()),
    )
}

// ---------------------------------------------------------------------------
// read
// ---------------------------------------------------------------------------

/// Public accessor for a typed settings value by dot path (used by core results).
pub fn get_value_public(settings: &SettingsFile, path: &str) -> CoreResult<Value> {
    get_typed(settings, path)
}

fn get_typed(settings: &SettingsFile, path: &str) -> CoreResult<Value> {
    let value = match path {
        "profile.displayName" => json!(settings.profile.display_name),
        "profile.email" => json!(settings.profile.email),
        "profile.avatar" => json!(settings.profile.avatar),
        "appearance.linkOpenMode" => json!(match settings.appearance.link_open_mode {
            LinkOpenMode::App => "app",
            LinkOpenMode::System => "system",
        }),
        "appearance.uiScale" => json!(settings.appearance.ui_scale),
        "appearance.uiFontSize" => json!(settings.appearance.ui_font_size),
        "appearance.markdownFontSize" => json!(settings.appearance.markdown_font_size),
        "appearance.editorFontSize" => json!(settings.appearance.editor_font_size),
        "appearance.tagFontSize" => json!(settings.appearance.tag_font_size),
        "appearance.themePresets" => json!(settings.appearance.theme_presets),
        "appearance.uiColors" => json!(settings.appearance.ui_colors),
        "lifecycle.closeToTray" => json!(settings.lifecycle.close_to_tray),
        "lifecycle.launchAtStartup" => json!(settings.lifecycle.launch_at_startup),
        "notifications.durationMs" => json!(settings.notifications.duration_ms),
        "notifications.position" => json!(settings.notifications.position.as_str()),
        "notifications.width" => json!(settings.notifications.width),
        "notifications.height" => json!(settings.notifications.height),
        "notifications.titleFontSize" => json!(settings.notifications.title_font_size),
        "notifications.bodyFontSize" => json!(settings.notifications.body_font_size),
        "shortcuts.newTask" => json!(settings.shortcuts.new_task),
        "shortcuts.focusSearch" => json!(settings.shortcuts.focus_search),
        "shortcuts.toggleWindow" => json!(settings.shortcuts.toggle_window),
        "shortcuts.openSettings" => json!(settings.shortcuts.open_settings),
        "cloud.provider" => json!(match settings.cloud.provider {
            CloudProvider::None => "none",
            CloudProvider::Webdav => "webdav",
            CloudProvider::S3 => "s3",
            CloudProvider::Custom => "custom",
        }),
        "cloud.endpoint" => json!(settings.cloud.endpoint),
        "cloud.enabled" => json!(settings.cloud.enabled),
        _ => return Err(unknown_field(path)),
    };
    Ok(value)
}

fn default_settings() -> SettingsFile {
    SettingsFile::default()
}

/// Whether the raw settings JSON explicitly contains this dot path.
fn raw_contains(raw: &Value, path: &str) -> bool {
    let mut current = raw;
    for segment in path.split('.') {
        match current.get(segment) {
            Some(next) => current = next,
            None => return false,
        }
    }
    true
}

pub fn get_value(
    settings: &SettingsFile,
    raw: &Value,
    path: &str,
    map_key: Option<&str>,
) -> CoreResult<Value> {
    let meta = field_meta(path).ok_or_else(|| unknown_field(path))?;
    if meta.is_map {
        let key = map_key.ok_or_else(|| {
            CoreError::validation(
                "MAP_KEY_REQUIRED",
                format!("{path} 是动态 map，必须通过 --map-key 指定键"),
            )
        })?;
        let map = &settings.appearance.ui_colors;
        let value = map.get(key).cloned().ok_or_else(|| {
            CoreError::not_found("MAP_KEY_NOT_FOUND", format!("{path} 中不存在键 `{key}`"))
        })?;
        return Ok(json!({
            "path": path,
            "mapKey": key,
            "value": value,
            "source": "user",
        }));
    }
    if map_key.is_some() {
        return Err(CoreError::validation(
            "MAP_KEY_UNEXPECTED",
            format!("{path} 不是 map，不接受 --map-key"),
        ));
    }
    let value = get_typed(settings, path)?;
    let source = if raw_contains(raw, path) {
        "user"
    } else {
        "default"
    };
    Ok(json!({
        "path": path,
        "value": value,
        "source": source,
        "kind": meta.kind,
        "description": meta.description,
    }))
}

pub fn list_values(
    settings: &SettingsFile,
    raw: &Value,
    prefix: Option<&str>,
) -> CoreResult<Vec<Value>> {
    let mut out = Vec::new();
    for field in KNOWN_FIELDS {
        if let Some(prefix) = prefix {
            if !field.path.starts_with(prefix) {
                continue;
            }
        }
        let value = get_typed(settings, field.path)?;
        let source = if raw_contains(raw, field.path) {
            "user"
        } else {
            "default"
        };
        out.push(json!({
            "path": field.path,
            "value": value,
            "source": source,
            "kind": field.kind,
            "description": field.description,
        }));
    }
    if out.is_empty() {
        if let Some(prefix) = prefix {
            return Err(CoreError::validation(
                "UNKNOWN_CONFIG_PREFIX",
                format!("没有匹配前缀 `{prefix}` 的配置项"),
            ));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// write
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct SetOutcome {
    pub previous: Value,
    pub value: Value,
    pub native_effects: Vec<&'static str>,
}

fn expect_string(path: &str, value: &Value) -> CoreResult<String> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| invalid_value(path, "应为字符串"))
}

fn expect_bool(path: &str, value: &Value) -> CoreResult<bool> {
    value
        .as_bool()
        .ok_or_else(|| invalid_value(path, "应为布尔值"))
}

fn expect_int(path: &str, value: &Value, min: i64, max: i64) -> CoreResult<i64> {
    let number = value
        .as_i64()
        .or_else(|| value.as_f64().map(|v| v.round() as i64))
        .ok_or_else(|| invalid_value(path, "应为整数"))?;
    if number < min || number > max {
        return Err(invalid_value(path, format!("应在 {min}-{max} 之间")));
    }
    Ok(number)
}

fn expect_scale(path: &str, value: &Value) -> CoreResult<f64> {
    let number = value
        .as_f64()
        .ok_or_else(|| invalid_value(path, "应为数字"))?;
    if !(0.5..=1.5).contains(&number) {
        return Err(invalid_value(path, "应在 0.5-1.5 之间"));
    }
    Ok(number)
}

fn expect_enum(path: &str, value: &Value, allowed: &[&str]) -> CoreResult<String> {
    let raw = expect_string(path, value)?;
    if allowed.contains(&raw.as_str()) {
        Ok(raw)
    } else {
        Err(invalid_value(
            path,
            format!("应为 {} 之一", allowed.join("|")),
        ))
    }
}

fn expect_color(path: &str, value: &Value) -> CoreResult<String> {
    let raw = expect_string(path, value)?;
    if is_hex_color(&raw) {
        Ok(raw.trim().to_string())
    } else {
        Err(invalid_value(path, "应为 #rrggbb 颜色"))
    }
}

fn expect_theme_presets(path: &str, value: &Value) -> CoreResult<Vec<ThemePreset>> {
    let items = value
        .as_array()
        .ok_or_else(|| invalid_value(path, "应为数组"))?;
    let mut presets = Vec::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid_value(path, "每项需要非空 name"))?;
        let color = item
            .get("color")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_value(path, "每项需要 color"))?;
        if !is_hex_color(color) {
            return Err(invalid_value(path, format!("color `{color}` 应为 #rrggbb")));
        }
        presets.push(ThemePreset {
            name: name.chars().take(24).collect(),
            color: color.trim().to_string(),
        });
    }
    Ok(presets)
}

pub fn set_value(
    settings: &mut SettingsFile,
    path: &str,
    value: Value,
    map_key: Option<&str>,
) -> CoreResult<SetOutcome> {
    let meta = field_meta(path).ok_or_else(|| unknown_field(path))?;
    let mut outcome = SetOutcome {
        previous: get_typed(settings, path).unwrap_or(Value::Null),
        value: value.clone(),
        native_effects: Vec::new(),
    };

    if meta.is_map {
        let key = map_key.ok_or_else(|| {
            CoreError::validation("MAP_KEY_REQUIRED", format!("{path} 需要 --map-key"))
        })?;
        if key.trim().is_empty() {
            return Err(CoreError::validation(
                "MAP_KEY_REQUIRED",
                "--map-key 不能为空",
            ));
        }
        let color = expect_color(path, &value)?;
        outcome.previous = settings
            .appearance
            .ui_colors
            .get(key)
            .cloned()
            .unwrap_or(Value::Null);
        outcome.value = json!(color);
        settings
            .appearance
            .ui_colors
            .insert(key.to_string(), json!(color));
        return Ok(outcome);
    }
    if map_key.is_some() {
        return Err(CoreError::validation(
            "MAP_KEY_UNEXPECTED",
            format!("{path} 不是 map，不接受 --map-key"),
        ));
    }

    match path {
        "profile.displayName" => {
            let v = expect_string(path, &value)?;
            settings.profile.display_name = v;
        }
        "profile.email" => settings.profile.email = expect_string(path, &value)?,
        "profile.avatar" => settings.profile.avatar = expect_string(path, &value)?,
        "appearance.linkOpenMode" => {
            settings.appearance.link_open_mode =
                match expect_enum(path, &value, &["app", "system"])?.as_str() {
                    "system" => LinkOpenMode::System,
                    _ => LinkOpenMode::App,
                };
        }
        "appearance.uiScale" => {
            settings.appearance.ui_scale = expect_scale(path, &value)?;
            outcome.native_effects.push("webviewZoom");
        }
        "appearance.uiFontSize" => {
            settings.appearance.ui_font_size = expect_int(path, &value, 14, 22)? as u32;
        }
        "appearance.markdownFontSize" => {
            settings.appearance.markdown_font_size = expect_int(path, &value, 14, 26)? as u32;
        }
        "appearance.editorFontSize" => {
            settings.appearance.editor_font_size = expect_int(path, &value, 14, 26)? as u32;
        }
        "appearance.tagFontSize" => {
            settings.appearance.tag_font_size = expect_int(path, &value, 11, 30)? as u32;
        }
        "appearance.themePresets" => {
            settings.appearance.theme_presets = expect_theme_presets(path, &value)?;
        }
        "lifecycle.closeToTray" => {
            settings.lifecycle.close_to_tray = expect_bool(path, &value)?;
            outcome.native_effects.push("closeToTray");
        }
        "lifecycle.launchAtStartup" => {
            settings.lifecycle.launch_at_startup = expect_bool(path, &value)?;
            outcome.native_effects.push("autostart");
        }
        "notifications.durationMs" => {
            settings.notifications.duration_ms = expect_int(path, &value, 1200, 60_000)? as u64;
        }
        "notifications.position" => {
            settings.notifications.position = match expect_enum(
                path,
                &value,
                &["bottom-right", "top-right", "bottom-left", "top-left"],
            )?
            .as_str()
            {
                "top-right" => NotificationPosition::TopRight,
                "bottom-left" => NotificationPosition::BottomLeft,
                "top-left" => NotificationPosition::TopLeft,
                _ => NotificationPosition::BottomRight,
            };
        }
        "notifications.width" => {
            settings.notifications.width = expect_int(path, &value, 280, 600)? as u32;
        }
        "notifications.height" => {
            settings.notifications.height = expect_int(path, &value, 50, 200)? as u32;
        }
        "notifications.titleFontSize" => {
            settings.notifications.title_font_size = expect_int(path, &value, 10, 24)? as u32;
        }
        "notifications.bodyFontSize" => {
            settings.notifications.body_font_size = expect_int(path, &value, 8, 20)? as u32;
        }
        "shortcuts.newTask" => {
            settings.shortcuts.new_task = expect_shortcut(path, &value)?;
            outcome.native_effects.push("shortcuts");
        }
        "shortcuts.focusSearch" => {
            settings.shortcuts.focus_search = expect_shortcut(path, &value)?;
            outcome.native_effects.push("shortcuts");
        }
        "shortcuts.toggleWindow" => {
            settings.shortcuts.toggle_window = expect_shortcut(path, &value)?;
            outcome.native_effects.push("globalShortcut");
        }
        "shortcuts.openSettings" => {
            settings.shortcuts.open_settings = expect_shortcut(path, &value)?;
            outcome.native_effects.push("shortcuts");
        }
        "cloud.provider" => {
            settings.cloud.provider =
                match expect_enum(path, &value, &["none", "webdav", "s3", "custom"])?.as_str() {
                    "webdav" => CloudProvider::Webdav,
                    "s3" => CloudProvider::S3,
                    "custom" => CloudProvider::Custom,
                    _ => CloudProvider::None,
                };
        }
        "cloud.endpoint" => settings.cloud.endpoint = expect_string(path, &value)?,
        "cloud.enabled" => settings.cloud.enabled = expect_bool(path, &value)?,
        _ => return Err(unknown_field(path)),
    }
    outcome.value = get_typed(settings, path)?;
    Ok(outcome)
}

fn expect_shortcut(path: &str, value: &Value) -> CoreResult<String> {
    let raw = expect_string(path, value)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_value(path, "快捷键不能为空"));
    }
    Ok(trimmed.to_string())
}

pub fn unset_value(
    settings: &mut SettingsFile,
    path: &str,
    map_key: Option<&str>,
) -> CoreResult<Value> {
    let meta = field_meta(path).ok_or_else(|| unknown_field(path))?;
    if !meta.is_map {
        return Err(CoreError::validation(
            "UNSET_UNSUPPORTED",
            format!("{path} 是带默认值的标量配置，请使用 kxtodo config reset {path} 恢复默认"),
        ));
    }
    let key = map_key.ok_or_else(|| {
        CoreError::validation("MAP_KEY_REQUIRED", format!("{path} 需要 --map-key"))
    })?;
    let previous = settings.appearance.ui_colors.remove(key).ok_or_else(|| {
        CoreError::not_found("MAP_KEY_NOT_FOUND", format!("{path} 中不存在键 `{key}`"))
    })?;
    Ok(previous)
}

/// Reset a branch (prefix) or all settings to defaults. Returns changed paths with before/after.
pub fn reset_values(settings: &mut SettingsFile, prefix: Option<&str>) -> CoreResult<Vec<Value>> {
    let defaults = default_settings();
    let mut changes = Vec::new();
    for field in KNOWN_FIELDS {
        if let Some(prefix) = prefix {
            let matches = field.path == prefix || field.path.starts_with(&format!("{prefix}."));
            if !matches {
                continue;
            }
        }
        let before = get_typed(settings, field.path)?;
        set_default(settings, &defaults, field.path)?;
        let after = get_typed(settings, field.path)?;
        if before != after {
            changes.push(json!({
                "path": field.path,
                "before": before,
                "after": after,
            }));
        }
    }
    if changes.is_empty() {
        if let Some(prefix) = prefix {
            let any = KNOWN_FIELDS
                .iter()
                .any(|field| field.path == prefix || field.path.starts_with(&format!("{prefix}.")));
            if !any {
                return Err(CoreError::validation(
                    "UNKNOWN_CONFIG_PREFIX",
                    format!("没有匹配前缀 `{prefix}` 的配置项"),
                ));
            }
        }
    }
    Ok(changes)
}

fn set_default(target: &mut SettingsFile, defaults: &SettingsFile, path: &str) -> CoreResult<()> {
    match path {
        "profile.displayName" => {
            target.profile.display_name = defaults.profile.display_name.clone()
        }
        "profile.email" => target.profile.email = defaults.profile.email.clone(),
        "profile.avatar" => target.profile.avatar = defaults.profile.avatar.clone(),
        "appearance.linkOpenMode" => {
            target.appearance.link_open_mode = defaults.appearance.link_open_mode
        }
        "appearance.uiScale" => target.appearance.ui_scale = defaults.appearance.ui_scale,
        "appearance.uiFontSize" => {
            target.appearance.ui_font_size = defaults.appearance.ui_font_size
        }
        "appearance.markdownFontSize" => {
            target.appearance.markdown_font_size = defaults.appearance.markdown_font_size
        }
        "appearance.editorFontSize" => {
            target.appearance.editor_font_size = defaults.appearance.editor_font_size
        }
        "appearance.tagFontSize" => {
            target.appearance.tag_font_size = defaults.appearance.tag_font_size
        }
        "appearance.themePresets" => {
            target.appearance.theme_presets = defaults.appearance.theme_presets.clone()
        }
        "appearance.uiColors" => target.appearance.ui_colors = Map::new(),
        "lifecycle.closeToTray" => {
            target.lifecycle.close_to_tray = defaults.lifecycle.close_to_tray
        }
        "lifecycle.launchAtStartup" => {
            target.lifecycle.launch_at_startup = defaults.lifecycle.launch_at_startup
        }
        "notifications.durationMs" => {
            target.notifications.duration_ms = defaults.notifications.duration_ms
        }
        "notifications.position" => target.notifications.position = defaults.notifications.position,
        "notifications.width" => target.notifications.width = defaults.notifications.width,
        "notifications.height" => target.notifications.height = defaults.notifications.height,
        "notifications.titleFontSize" => {
            target.notifications.title_font_size = defaults.notifications.title_font_size
        }
        "notifications.bodyFontSize" => {
            target.notifications.body_font_size = defaults.notifications.body_font_size
        }
        "shortcuts.newTask" => target.shortcuts.new_task = defaults.shortcuts.new_task.clone(),
        "shortcuts.focusSearch" => {
            target.shortcuts.focus_search = defaults.shortcuts.focus_search.clone()
        }
        "shortcuts.toggleWindow" => {
            target.shortcuts.toggle_window = defaults.shortcuts.toggle_window.clone()
        }
        "shortcuts.openSettings" => {
            target.shortcuts.open_settings = defaults.shortcuts.open_settings.clone()
        }
        "cloud.provider" => target.cloud.provider = defaults.cloud.provider,
        "cloud.endpoint" => target.cloud.endpoint = defaults.cloud.endpoint.clone(),
        "cloud.enabled" => target.cloud.enabled = defaults.cloud.enabled,
        _ => return Err(unknown_field(path)),
    }
    Ok(())
}

/// Raw value from CLI flags: positional string, --json-value, or --value-file.
pub fn parse_cli_value(raw: &str) -> CoreResult<Value> {
    let trimmed = raw.trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }
    Ok(Value::String(raw.to_string()))
}

/// Settings validation report (config validate + doctor).
pub fn validate_settings(
    layout: &crate::domain::repo::Layout,
    settings: &SettingsFile,
) -> Vec<Value> {
    let mut issues = Vec::new();
    if !(0.5..=1.5).contains(&settings.appearance.ui_scale) {
        issues.push(json!({
            "path": "appearance.uiScale",
            "issue": format!("uiScale {} 超出 0.5-1.5", settings.appearance.ui_scale),
            "suggestion": "kxtodo config set appearance.uiScale 0.75",
        }));
    }
    if !settings.profile.avatar.trim().is_empty() && !settings.profile.avatar.starts_with("data:") {
        let avatar_path = layout
            .img_dir()
            .join("avator")
            .join(settings.profile.avatar.trim());
        if !avatar_path.is_file() {
            issues.push(json!({
                "path": "profile.avatar",
                "issue": format!("头像文件不存在：{}", settings.profile.avatar),
                "suggestion": "kxtodo config set profile.avatar \"\"",
            }));
        }
    }
    for (node_id, color) in &settings.appearance.ui_colors {
        if !color.as_str().map(is_hex_color).unwrap_or(false) {
            issues.push(json!({
                "path": "appearance.uiColors",
                "issue": format!("键 {node_id} 的颜色无效"),
                "suggestion": format!("kxtodo config unset appearance.uiColors --map-key {node_id}"),
            }));
        }
    }
    for (index, preset) in settings.appearance.theme_presets.iter().enumerate() {
        if !is_hex_color(&preset.color) {
            issues.push(json!({
                "path": "appearance.themePresets",
                "issue": format!("第 {} 项颜色无效：{}", index + 1, preset.color),
                "suggestion": "kxtodo config reset appearance.themePresets",
            }));
        }
    }
    issues
}
