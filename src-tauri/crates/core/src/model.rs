//! Authoritative v9 domain models (requirements §4.2).
//! Rust is the single source of truth; JSON Schema / TS types derive from these.
//! Unknown fields are preserved via `extra` flatten maps (保留式读写).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const DATA_SCHEMA_VERSION: u32 = 6;
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const SCHEDULE_SCHEMA_VERSION: u32 = 2;

pub const SYSTEM_NODE_IDS: [&str; 4] = ["my-day", "planned", "important", "scheduled"];

// ---------------------------------------------------------------------------
// _meta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DomainMeta {
    #[serde(default)]
    pub revision: u64,
    #[serde(rename = "schemaVersion", skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub idempotency: Vec<IdempotencyRecord>,
    /// 跨设备同步的删除墓碑（data/schedule 域使用；服务器只见密文，删除必须显式传播）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tombstones: Vec<Tombstone>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Tombstone {
    pub id: String,
    /// "node" | "task" | "schedule"
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

impl DomainMeta {
    /// 记录/覆盖墓碑（同 id 保留最新时间戳）。
    pub fn record_tombstone(&mut self, id: &str, kind: &str, at: &str) {
        if let Some(existing) = self.tombstones.iter_mut().find(|item| item.id == id) {
            existing.kind = kind.to_string();
            existing.updated_at = at.to_string();
        } else {
            self.tombstones.push(Tombstone {
                id: id.to_string(),
                kind: kind.to_string(),
                updated_at: at.to_string(),
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IdempotencyRecord {
    pub key: String,
    pub command: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// Compact outcome summary used to replay the first result.
    pub summary: Value,
}

// ---------------------------------------------------------------------------
// data.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DataFile {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    #[serde(rename = "_meta", default)]
    pub meta: DomainMeta,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub tasks: Vec<Item>,
    #[serde(rename = "selectedNodeId", default)]
    pub selected_node_id: String,
    #[serde(default)]
    pub backgrounds: Map<String, Value>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    System,
    Category,
    Entry,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::System => "system",
            NodeKind::Category => "category",
            NodeKind::Entry => "entry",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    #[serde(rename = "parentId", default)]
    pub parent_id: Option<String>,
    /// 同级排序（父节点内相对位置；跨设备合并按 (order, id) 排序）。
    #[serde(default)]
    pub order: f64,
    #[serde(rename = "collapsed", skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
pub enum TagColor {
    Red,
    Yellow,
    Blue,
    Green,
    Gray,
}

impl TagColor {
    pub fn as_str(self) -> &'static str {
        match self {
            TagColor::Red => "red",
            TagColor::Yellow => "yellow",
            TagColor::Blue => "blue",
            TagColor::Green => "green",
            TagColor::Gray => "gray",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "red" => Some(TagColor::Red),
            "yellow" => Some(TagColor::Yellow),
            "blue" => Some(TagColor::Blue),
            "green" => Some(TagColor::Green),
            "gray" => Some(TagColor::Gray),
            _ => None,
        }
    }
}

impl Serialize for TagColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TagColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        // Lenient like the legacy frontend normalizer: unknown colors degrade to gray.
        Ok(TagColor::parse(&raw).unwrap_or(TagColor::Gray))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Tag {
    pub id: String,
    pub color: TagColor,
    #[serde(rename = "text", skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub id: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    /// 同级排序（条目内相对位置；跨设备合并按 (order, id) 排序）。
    #[serde(default)]
    pub order: f64,
    #[serde(default)]
    pub markdown: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub important: bool,
    #[serde(rename = "myDay", default)]
    pub my_day: bool,
    #[serde(rename = "plannedDate", skip_serializing_if = "Option::is_none")]
    pub planned_date: Option<String>,
    #[serde(rename = "dueDate", skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(rename = "completedAt", skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emojis: Vec<String>,
    #[serde(rename = "expanded", skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// settings.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SettingsFile {
    #[serde(rename = "_meta", default)]
    pub meta: DomainMeta,
    #[serde(default)]
    pub profile: ProfileSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub lifecycle: LifecycleSettings,
    #[serde(default)]
    pub notifications: NotificationSettings,
    #[serde(default)]
    pub shortcuts: ShortcutSettings,
    #[serde(default)]
    pub sync: SyncSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub features: FeatureSettings,
    /// 设置同步实体的 LWW 时间戳（仅共享子集变化时刷新）。
    #[serde(rename = "syncUpdatedAt", default, skip_serializing_if = "Option::is_none")]
    pub sync_updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileSettings {
    #[serde(rename = "displayName", default = "default_display_name")]
    pub display_name: String,
    #[serde(default = "default_email")]
    pub email: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_display_name() -> String {
    "Example User".to_string()
}
fn default_email() -> String {
    "example@example.com".to_string()
}

impl Default for ProfileSettings {
    fn default() -> Self {
        Self {
            display_name: default_display_name(),
            email: default_email(),
            avatar: String::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum LinkOpenMode {
    #[serde(rename = "app")]
    App,
    #[serde(rename = "system")]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ThemePreset {
    pub name: String,
    pub color: String,
}

pub fn default_theme_presets() -> Vec<ThemePreset> {
    [
        ("雾瓷", "#f4f1ea"),
        ("睡莲灰绿", "#dfe8df"),
        ("晨雾蓝", "#dbe4e6"),
        ("粉霞", "#ead9d5"),
        ("鸢尾雾紫", "#ded8e6"),
        ("亚麻麦秆", "#ece2ca"),
        ("石英灰", "#e3e0d8"),
        ("鼠尾草", "#d8dfd2"),
        ("贵族蓝灰", "#cfd9df"),
        ("陶土玫瑰", "#e5d4cb"),
    ]
    .iter()
    .map(|(name, color)| ThemePreset {
        name: name.to_string(),
        color: color.to_string(),
    })
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppearanceSettings {
    #[serde(rename = "linkOpenMode", default = "default_link_open_mode")]
    pub link_open_mode: LinkOpenMode,
    #[serde(rename = "uiScale", default = "default_ui_scale")]
    pub ui_scale: f64,
    #[serde(rename = "uiFontSize", default = "default_ui_font_size")]
    pub ui_font_size: u32,
    #[serde(rename = "markdownFontSize", default = "default_markdown_font_size")]
    pub markdown_font_size: u32,
    #[serde(rename = "editorFontSize", default = "default_editor_font_size")]
    pub editor_font_size: u32,
    #[serde(rename = "tagFontSize", default = "default_tag_font_size")]
    pub tag_font_size: u32,
    #[serde(rename = "themePresets", default = "default_theme_presets")]
    pub theme_presets: Vec<ThemePreset>,
    #[serde(rename = "uiColors", default)]
    pub ui_colors: Map<String, Value>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_link_open_mode() -> LinkOpenMode {
    LinkOpenMode::App
}
fn default_ui_scale() -> f64 {
    0.75
}
fn default_ui_font_size() -> u32 {
    18
}
fn default_markdown_font_size() -> u32 {
    20
}
fn default_editor_font_size() -> u32 {
    20
}
fn default_tag_font_size() -> u32 {
    14
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            link_open_mode: default_link_open_mode(),
            ui_scale: default_ui_scale(),
            ui_font_size: default_ui_font_size(),
            markdown_font_size: default_markdown_font_size(),
            editor_font_size: default_editor_font_size(),
            tag_font_size: default_tag_font_size(),
            theme_presets: default_theme_presets(),
            ui_colors: Map::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LifecycleSettings {
    #[serde(rename = "closeToTray", default = "default_true")]
    pub close_to_tray: bool,
    #[serde(rename = "launchAtStartup", default)]
    pub launch_at_startup: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_true() -> bool {
    true
}

impl Default for LifecycleSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            launch_at_startup: false,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum NotificationPosition {
    #[serde(rename = "bottom-right")]
    BottomRight,
    #[serde(rename = "top-right")]
    TopRight,
    #[serde(rename = "bottom-left")]
    BottomLeft,
    #[serde(rename = "top-left")]
    TopLeft,
}

impl NotificationPosition {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationPosition::BottomRight => "bottom-right",
            NotificationPosition::TopRight => "top-right",
            NotificationPosition::BottomLeft => "bottom-left",
            NotificationPosition::TopLeft => "top-left",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Tone {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Info => "info",
            Tone::Success => "success",
            Tone::Warning => "warning",
            Tone::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotificationSettings {
    #[serde(rename = "durationMs", default = "default_notification_duration_ms")]
    pub duration_ms: u64,
    #[serde(default = "default_notification_position")]
    pub position: NotificationPosition,
    #[serde(default = "default_notification_width")]
    pub width: u32,
    #[serde(default = "default_notification_height")]
    pub height: u32,
    #[serde(rename = "titleFontSize", default = "default_title_font_size")]
    pub title_font_size: u32,
    #[serde(rename = "bodyFontSize", default = "default_body_font_size")]
    pub body_font_size: u32,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_notification_duration_ms() -> u64 {
    3000
}
fn default_notification_position() -> NotificationPosition {
    NotificationPosition::BottomRight
}
fn default_notification_width() -> u32 {
    400
}
fn default_notification_height() -> u32 {
    68
}
fn default_title_font_size() -> u32 {
    14
}
fn default_body_font_size() -> u32 {
    12
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            duration_ms: default_notification_duration_ms(),
            position: default_notification_position(),
            width: default_notification_width(),
            height: default_notification_height(),
            title_font_size: default_title_font_size(),
            body_font_size: default_body_font_size(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShortcutSettings {
    #[serde(rename = "newTask", default = "default_shortcut_new_task")]
    pub new_task: String,
    #[serde(rename = "focusSearch", default = "default_shortcut_focus_search")]
    pub focus_search: String,
    #[serde(rename = "toggleWindow", default = "default_shortcut_toggle_window")]
    pub toggle_window: String,
    #[serde(rename = "openSettings", default = "default_shortcut_open_settings")]
    pub open_settings: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_shortcut_new_task() -> String {
    "Ctrl+N".to_string()
}
fn default_shortcut_focus_search() -> String {
    "Ctrl+F".to_string()
}
fn default_shortcut_toggle_window() -> String {
    "Ctrl+Shift+Space".to_string()
}
fn default_shortcut_open_settings() -> String {
    "Ctrl+,".to_string()
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            new_task: default_shortcut_new_task(),
            focus_search: default_shortcut_focus_search(),
            toggle_window: default_shortcut_toggle_window(),
            open_settings: default_shortcut_open_settings(),
            extra: Map::new(),
        }
    }
}

/// 通信方式（v0.6.0）：三种方式共用同一套同步内核（密钥派生、LWW 合并、墓碑、
/// 水位、图片通道、范围、排程），差别只在「base url 从哪来」——见 `sync/endpoint.rs`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    /// P2P：没有公网 IP 也能跨网络直连（iroh 隧道；v0.6.1 提供）
    P2p,
    /// 局域网：本机作为主机，或从「发现」列表里选定一台主机
    Lan,
    /// 自建服务：手填 ip:port 连一台常开的 kxtodo-server
    Server,
}

impl Default for SyncMode {
    fn default() -> Self {
        SyncMode::Lan
    }
}

impl SyncMode {
    /// 序列化/CLI 用的小写标识
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncMode::P2p => "p2p",
            SyncMode::Lan => "lan",
            SyncMode::Server => "server",
        }
    }

    /// 用户可见名称（设置面板下拉框、状态与日志）
    pub fn label(&self) -> &'static str {
        match self {
            SyncMode::P2p => "P2P",
            SyncMode::Lan => "局域网",
            SyncMode::Server => "自建服务",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "p2p" => Some(SyncMode::P2p),
            "lan" => Some(SyncMode::Lan),
            "server" => Some(SyncMode::Server),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SyncSettings {
    /// 已配对并启用同步
    #[serde(default)]
    pub enabled: bool,
    /// 通信方式；None = 用户还没显式选过，按已有配置推断（见 [`SyncSettings::effective_mode`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SyncMode>,
    #[serde(rename = "serverUrl", default)]
    pub server_url: String,
    /// 局域网：本机作为主机（内置 server 随应用启停，取消勾选不影响自己当客户端）
    #[serde(rename = "lanHost", default)]
    pub lan_host: bool,
    /// 局域网：本机作为主机时的监听端口。被占用会自动向上找，
    /// **实际端口以 `runtime/sync-host.json` 为准**（自动发现回包带的也是实际端口）
    #[serde(rename = "lanPort", default = "default_lan_port")]
    pub lan_port: u16,
    /// 局域网：本机作为主机时的展示名，也就是它在局域网里的**身份**（要求唯一）
    #[serde(rename = "lanName", default)]
    pub lan_name: String,
    /// 局域网：选定的远端主机名（从「发现」列表里点选，持久化，不必每次重选）
    #[serde(rename = "lanPeer", default)]
    pub lan_peer: String,
    #[serde(default)]
    pub username: String,
    /// 同步密码（派生 auth/enc 密钥；只存本机，不随设置同步）
    #[serde(default)]
    pub secret: String,
    /// 同步数据（节点/任务 + markdown 插图文件本体，默认开）
    #[serde(rename = "syncData", default = "default_true")]
    pub sync_data: bool,
    /// 同步设置共享子集（配置/配色 + 背景与头像文件本体，默认开）
    #[serde(rename = "syncSettings", default = "default_true")]
    pub sync_settings: bool,
    /// 同步定时任务 spec（默认关；spec 含各机器绝对路径，跨平台通常不可执行）
    #[serde(rename = "syncSchedules", default)]
    pub sync_schedules: bool,
    /// 自动同步间隔（秒）
    #[serde(rename = "intervalSeconds", default = "default_sync_interval_seconds")]
    pub interval_seconds: u32,
    /// 掉线后的静默重连探测间隔（秒）
    #[serde(rename = "reconnectSeconds", default = "default_sync_reconnect_seconds")]
    pub reconnect_seconds: u32,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_lan_port() -> u16 {
    crate::sync::discovery::DEFAULT_SERVER_PORT
}

fn default_sync_interval_seconds() -> u32 {
    30
}

fn default_sync_reconnect_seconds() -> u32 {
    300
}

impl SyncSettings {
    /// 生效的通信方式：用户没显式选过时按已有配置推断——填过服务器地址就是
    /// 「自建服务」（升级上来的旧配置直接可用），否则「局域网」。
    ///
    /// 这不是兼容层：用户在下拉框里选过一次之后 `mode` 就落盘了，推断只发生在那之前。
    pub fn effective_mode(&self) -> SyncMode {
        self.mode.unwrap_or_else(|| {
            if self.server_url.trim().is_empty() {
                SyncMode::Lan
            } else {
                SyncMode::Server
            }
        })
    }

    /// 是否已配对：有用户名 + 密码，且当前通信方式有一个明确的对端。
    ///
    /// 与 `enabled` 是两件事——`enabled = false` 表示用户「暂停同步」，配置全部保留；
    /// 只有 `sync unpair` 才会清掉密码（于是这里变成 false）。
    pub fn is_paired(&self) -> bool {
        if self.username.trim().is_empty() || self.secret.trim().is_empty() {
            return false;
        }
        match self.effective_mode() {
            SyncMode::Server => !self.server_url.trim().is_empty(),
            // 局域网：要么本机是主机（连自己的 localhost），要么已经选定了一台主机
            SyncMode::Lan => self.lan_host || !self.lan_peer.trim().is_empty(),
            // P2P：对端地址由账户凭据派生的目录解析出来，不需要额外配置
            SyncMode::P2p => true,
        }
    }

    /// 应用局域网角色的不变式。`sync configure` 与 `config set` 共用这一条口径，
    /// 免得两条写路径各写一半、界面上同时显示两种身份。
    ///
    /// - **主机与客户端二选一**：勾选本机作为主机就清掉选定的远端主机；
    ///   选定了一台远端主机就不再是主机（同一次调用里两者都给时，主机开关优先）
    /// - 勾选主机就等于选了局域网模式（在别的模式下这个开关没有意义）
    /// - 主机必须有名字——名字是它在局域网里的**身份**，缺省用机器名
    pub fn apply_lan_role(&mut self, host: Option<bool>, name: Option<&str>, peer: Option<&str>) {
        if let Some(value) = name {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.lan_name = trimmed.to_string();
            }
        }
        if let Some(value) = peer {
            self.lan_peer = value.trim().to_string();
        }
        if let Some(value) = host {
            self.lan_host = value;
        }
        if host == Some(true) {
            self.lan_peer = String::new();
        } else if !self.lan_peer.trim().is_empty() {
            self.lan_host = false;
        }
        if self.lan_host {
            self.mode = Some(SyncMode::Lan);
            if self.lan_name.trim().is_empty() {
                self.lan_name = crate::sync::endpoint::default_host_name();
            }
        }
    }
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: None,
            server_url: String::new(),
            lan_host: false,
            lan_port: default_lan_port(),
            lan_name: String::new(),
            lan_peer: String::new(),
            username: String::new(),
            secret: String::new(),
            sync_data: default_true(),
            sync_settings: default_true(),
            sync_schedules: false,
            interval_seconds: default_sync_interval_seconds(),
            reconnect_seconds: default_sync_reconnect_seconds(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateSettings {
    /// 启动后自动检查更新（GitHub latest release）
    #[serde(rename = "autoCheck", default = "default_true")]
    pub auto_check: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check: true,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSettings {
    /// 侧栏分类行显示未完成条目数角标（特性开关）
    #[serde(rename = "showCategoryBadges", default = "default_true")]
    pub show_category_badges: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl Default for FeatureSettings {
    fn default() -> Self {
        Self {
            show_category_badges: true,
            extra: Map::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// tasks.json (schedules)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleFile {
    #[serde(rename = "_meta", default)]
    pub meta: DomainMeta,
    #[serde(default)]
    pub runtimes: Runtimes,
    #[serde(default)]
    pub tasks: Vec<ScheduleEntry>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Runtimes {
    #[serde(default)]
    pub python: String,
    #[serde(default)]
    pub node: String,
    #[serde(default)]
    pub pwsh: String,
    #[serde(default)]
    pub bash: String,
    #[serde(default)]
    pub make: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleEntry {
    pub id: String,
    pub spec: ScheduleSpec,
    #[serde(default)]
    pub state: ScheduleState,
    #[serde(default)]
    pub ui: ScheduleUi,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleUi {
    #[serde(rename = "expanded", skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(rename = "editing", skip_serializing_if = "Option::is_none")]
    pub editing: Option<bool>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub enum ScheduleStatus {
    #[default]
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "stopped")]
    Stopped,
}

impl ScheduleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleStatus::Idle => "idle",
            ScheduleStatus::Running => "running",
            ScheduleStatus::Success => "success",
            ScheduleStatus::Failed => "failed",
            ScheduleStatus::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleState {
    #[serde(rename = "runCount", default)]
    pub run_count: u64,
    #[serde(rename = "running", default, skip_serializing_if = "is_false")]
    pub running: bool,
    #[serde(rename = "lastRunAt", skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    #[serde(rename = "nextRunAt", skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
    #[serde(rename = "lastStatus", default)]
    pub last_status: ScheduleStatus,
    #[serde(rename = "lastExitCode", skip_serializing_if = "Option::is_none")]
    pub last_exit_code: Option<i32>,
    #[serde(rename = "lastStdout", skip_serializing_if = "Option::is_none")]
    pub last_stdout: Option<String>,
    #[serde(rename = "lastStderr", skip_serializing_if = "Option::is_none")]
    pub last_stderr: Option<String>,
    #[serde(rename = "lastMissedAt", skip_serializing_if = "Option::is_none")]
    pub last_missed_at: Option<String>,
    #[serde(rename = "missedCount", default, skip_serializing_if = "is_zero")]
    pub missed_count: u64,
    #[serde(rename = "lastProbe", skip_serializing_if = "Option::is_none")]
    pub last_probe: Option<ProbeState>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn is_false(value: &bool) -> bool {
    !*value
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProbeState {
    pub at: String,
    pub status: ScheduleStatus,
    #[serde(rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

// ---------------------------------------------------------------------------
// ScheduleSpec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleSpec {
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum MissedPolicy {
    #[serde(rename = "skip")]
    Skip,
    #[serde(rename = "run-once")]
    RunOnce,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Trigger {
    #[serde(rename = "once")]
    Once {
        at: String,
        #[serde(rename = "missedPolicy", skip_serializing_if = "Option::is_none")]
        missed_policy: Option<MissedPolicy>,
    },
    #[serde(rename = "interval")]
    Interval {
        every: String,
        #[serde(rename = "maxRuns", skip_serializing_if = "Option::is_none")]
        max_runs: Option<u64>,
        #[serde(rename = "stopWhen", skip_serializing_if = "Option::is_none")]
        stop_when: Option<Match>,
        #[serde(rename = "missedPolicy", skip_serializing_if = "Option::is_none")]
        missed_policy: Option<MissedPolicy>,
    },
    #[serde(rename = "calendar")]
    Calendar {
        cron: String,
        timezone: String,
        #[serde(rename = "missedPolicy", skip_serializing_if = "Option::is_none")]
        missed_policy: Option<MissedPolicy>,
    },
    #[serde(rename = "condition")]
    Condition {
        every: String,
        probe: Probe,
        when: Match,
        #[serde(rename = "missedPolicy", skip_serializing_if = "Option::is_none")]
        missed_policy: Option<MissedPolicy>,
    },
}

impl Trigger {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Trigger::Once { .. } => "once",
            Trigger::Interval { .. } => "interval",
            Trigger::Calendar { .. } => "calendar",
            Trigger::Condition { .. } => "condition",
        }
    }

    /// Default missed policy per trigger type (§3.5.2).
    pub fn effective_missed_policy(&self) -> MissedPolicy {
        let explicit = match self {
            Trigger::Once { missed_policy, .. }
            | Trigger::Interval { missed_policy, .. }
            | Trigger::Calendar { missed_policy, .. }
            | Trigger::Condition { missed_policy, .. } => *missed_policy,
        };
        explicit.unwrap_or(match self {
            Trigger::Calendar { .. } => MissedPolicy::Skip,
            _ => MissedPolicy::RunOnce,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Match {
    pub stream: MatchStream,
    #[serde(default = "default_match_mode")]
    pub mode: MatchMode,
    pub pattern: String,
}

fn default_match_mode() -> MatchMode {
    MatchMode::Contains
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum MatchStream {
    #[serde(rename = "stdout")]
    Stdout,
    #[serde(rename = "stderr")]
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum MatchMode {
    #[serde(rename = "contains")]
    Contains,
    #[serde(rename = "regex")]
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ScriptLanguage {
    #[serde(rename = "python")]
    Python,
    #[serde(rename = "javascript")]
    Javascript,
    #[serde(rename = "powershell")]
    Powershell,
    #[serde(rename = "bash")]
    Bash,
    #[serde(rename = "makefile")]
    Makefile,
}

impl ScriptLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            ScriptLanguage::Python => "python",
            ScriptLanguage::Javascript => "javascript",
            ScriptLanguage::Powershell => "powershell",
            ScriptLanguage::Bash => "bash",
            ScriptLanguage::Makefile => "makefile",
        }
    }

    pub fn runtime_key(self) -> &'static str {
        match self {
            ScriptLanguage::Python => "python",
            ScriptLanguage::Javascript => "node",
            ScriptLanguage::Powershell => "pwsh",
            ScriptLanguage::Bash => "bash",
            ScriptLanguage::Makefile => "make",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Source {
    #[serde(rename = "file")]
    File { path: String },
    #[serde(rename = "inline")]
    Inline { code: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Action {
    #[serde(rename = "script")]
    Script {
        language: ScriptLanguage,
        source: Source,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        interpreter: Option<String>,
        #[serde(rename = "workingDirectory", skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        notifications: Option<ActionNotifications>,
    },
    #[serde(rename = "executable")]
    Executable {
        program: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(rename = "workingDirectory", skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        notifications: Option<ActionNotifications>,
    },
    #[serde(rename = "notification")]
    Notification { notification: Notification },
}

impl Action {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Action::Script { .. } => "script",
            Action::Executable { .. } => "executable",
            Action::Notification { .. } => "notification",
        }
    }

    pub fn is_code_execution(&self) -> bool {
        matches!(self, Action::Script { .. } | Action::Executable { .. })
    }

    pub fn notifications(&self) -> Option<&ActionNotifications> {
        match self {
            Action::Script { notifications, .. } | Action::Executable { notifications, .. } => {
                notifications.as_ref()
            }
            Action::Notification { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Probe {
    #[serde(rename = "script")]
    Script {
        language: ScriptLanguage,
        source: Source,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        interpreter: Option<String>,
        #[serde(rename = "workingDirectory", skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<String>,
    },
    #[serde(rename = "executable")]
    Executable {
        program: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(rename = "workingDirectory", skip_serializing_if = "Option::is_none")]
        working_directory: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Notification {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<NotificationPosition>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActionNotifications {
    #[serde(rename = "onComplete", skip_serializing_if = "Option::is_none")]
    pub on_complete: Option<Notification>,
    #[serde(rename = "onOutput", skip_serializing_if = "Option::is_none")]
    pub on_output: Option<OnOutputNotification>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OnOutputNotification {
    pub when: Match,
    pub notification: Notification,
}
