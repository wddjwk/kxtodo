//! Authoritative v9 domain models (requirements §4.2).
//! Rust is the single source of truth; JSON Schema / TS types derive from these.
//! Unknown fields are preserved via `extra` flatten maps (保留式读写).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const DATA_SCHEMA_VERSION: u32 = 6;
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const SCHEDULE_SCHEMA_VERSION: u32 = 2;
pub const DIARY_SCHEMA_VERSION: u32 = 1;
pub const LEDGER_SCHEMA_VERSION: u32 = 1;

/// 日记插图复用「按条目分目录」的图片通道（`img/data/<nodeId>/`），伪条目 id 固定为
/// `diary`——与前端 `diary.ts` 的 DIARY_IMAGE_NODE 同名，图片存储与同步一行都不用改。
pub const DIARY_IMAGE_NODE: &str = "diary";

/// 记账条目的附图（v0.7.4）走同一条图片通道，伪条目 id 固定为 `ledger`——与前端同名，
/// 于是图片的存储/同步（entry 类别随「同步数据」范围）/释放空间盘点一行都不用改。
pub const LEDGER_IMAGE_NODE: &str = "ledger";

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
    /// "node" | "task" | "diary" | "schedule"
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
    /// 工具箱草稿纸（v0.8.4）：整篇一段纯文本，改了整段覆盖（LWW）。
    /// 住在数据域而不是 independent 的运行时文件里，因为它是**内容**——
    /// 换台设备该带着走（同步走 data 范围，实体 kind = scratchpad）。
    #[serde(default)]
    pub scratchpad: Scratchpad,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

/// 草稿纸：一段纯文本 + 它的版本戳（LWW 的实体时间戳就是 updatedAt）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Scratchpad {
    #[serde(default)]
    pub text: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
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
    /// 条目的渲染类型：None/"todo" = 待办卡片（可勾选、有「已完成」分区），
    /// "card" = 一般卡片（隐藏勾选框、内容占满整个卡片，用于日志/随笔这类展示型条目）。
    /// 纯渲染差异：任务的增删改查与完成状态一律不受影响。
    #[serde(rename = "cardStyle", default, skip_serializing_if = "Option::is_none")]
    pub card_style: Option<String>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

/// 标签配色：七彩虹 + 灰 + 自定义。
///
/// **`Custom` 必须配 [`Tag::hex`]**（`#rrggbb`）；hex 缺失或非法时按 [`TagColor::Gray`] 渲染，
/// 见 `Tag::effective_color`。九个具名色 = 七彩虹 + 粉 + 灰。
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
pub enum TagColor {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Pink,
    Gray,
    Custom,
}

impl TagColor {
    pub fn as_str(self) -> &'static str {
        match self {
            TagColor::Red => "red",
            TagColor::Orange => "orange",
            TagColor::Yellow => "yellow",
            TagColor::Green => "green",
            TagColor::Cyan => "cyan",
            TagColor::Blue => "blue",
            TagColor::Purple => "purple",
            TagColor::Pink => "pink",
            TagColor::Gray => "gray",
            TagColor::Custom => "custom",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "red" => Some(TagColor::Red),
            "orange" => Some(TagColor::Orange),
            "yellow" => Some(TagColor::Yellow),
            "green" => Some(TagColor::Green),
            "cyan" => Some(TagColor::Cyan),
            "blue" => Some(TagColor::Blue),
            "purple" => Some(TagColor::Purple),
            "pink" => Some(TagColor::Pink),
            "gray" => Some(TagColor::Gray),
            "custom" => Some(TagColor::Custom),
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
    /// 自定义配色的 `#rrggbb`（只在 `color == Custom` 时有意义）。
    #[serde(rename = "hex", skip_serializing_if = "Option::is_none")]
    pub hex: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl Tag {
    /// 实际配色：`Custom` 但 hex 缺失/非法时退回灰色（同步过来的数据可能是手改的）。
    pub fn effective_color(&self) -> TagColor {
        if self.color == TagColor::Custom && tag_hex(self.hex.as_deref()).is_none() {
            TagColor::Gray
        } else {
            self.color
        }
    }

    /// 自定义配色的合法值（非 `Custom` 或非法时 None）。
    pub fn custom_hex(&self) -> Option<String> {
        if self.color != TagColor::Custom {
            return None;
        }
        tag_hex(self.hex.as_deref())
    }
}

/// `#rrggbb`（三字节十六进制，带不带 `#` 都行；返回带 `#` 的小写形式）。
pub fn tag_hex(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim().trim_start_matches('#');
    let lower = text.to_ascii_lowercase();
    (lower.len() == 6 && lower.chars().all(|ch| ch.is_ascii_hexdigit())).then(|| format!("#{lower}"))
}

/// 任务提醒规则（跟着任务一起同步）。
///
/// 两种写法：**绝对时刻**（`at` 是 RFC3339 的 UTC 瞬时，自定义提醒落这一档）与
/// **相对截止**（`minutes` = 到期前多少分钟，0 = 到点时；跟着 dueDate/dueTime 漂移）。
/// 只存规则不存「什么时候响过」——发送台账是本机 runtime 的事（`reminders.rs`），
/// 绝不能进同步载荷，否则一台设备响过另一台就再也不响了。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Reminder {
    /// 绝对时刻：RFC3339（UTC）
    Absolute { at: String },
    /// 截止前 N 分钟（0 = 到点时）；需要 dueDate 与 dueTime 都有值
    BeforeDue { minutes: u32 },
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
    /// 置顶是任务内容，独立于图标/分区显示开关，并跟随任务同步。
    #[serde(default)]
    pub pinned: bool,
    #[serde(rename = "myDay", default)]
    pub my_day: bool,
    #[serde(rename = "plannedDate", skip_serializing_if = "Option::is_none")]
    pub planned_date: Option<String>,
    #[serde(rename = "dueDate", skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// 到期时刻 HH:MM，空 = 只精确到天（与 dueDate 搭配使用）
    #[serde(rename = "dueTime", default, skip_serializing_if = "String::is_empty")]
    pub due_time: String,
    /// 提醒规则；空 = 没有提醒
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reminders: Vec<Reminder>,
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
// diary.json
// ---------------------------------------------------------------------------

/// 日记条目：以「归属日期」为核心属性的 Markdown 记录。
///
/// 一天可以有多篇（按 `createdAt` 先后排列）；除 `date` 外全部可选，
/// 一句话的日记不该被逼着填标题/心情/天气。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiaryEntry {
    pub id: String,
    /// 归属日期 YYYY-MM-DD（可以后补写别的日子，不等于 createdAt 的日期）
    pub date: String,
    /// 标题；空 = 卡片直接展示正文首行
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// 正文 Markdown
    #[serde(default)]
    pub markdown: String,
    /// 心情（emoji；空 = 没记）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mood: String,
    /// 天气（emoji；空 = 没记）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub weather: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// 本机 UI 状态，不参与同步
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

/// diary.json：日记是独立的第四个领域文件。
///
/// 单独成文件的理由是写路径互不干扰——写一篇日记不该抬高 data 域的 revision、
/// 也不该和任务写入抢同一个文件锁与幂等台账。同步上它仍然搭「同步数据」的范围。
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DiaryFile {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    #[serde(rename = "_meta", default)]
    pub meta: DomainMeta,
    #[serde(default)]
    pub entries: Vec<DiaryEntry>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// ledger.json
// ---------------------------------------------------------------------------

/// 记账条目类型：支出 / 收入 / 转账。
///
/// 转账不计入收支统计，只改两个账户的余额——这是记账软件的通用口径
/// （把钱从左口袋挪到右口袋不是消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LedgerKind {
    Expense,
    Income,
    Transfer,
}

impl Default for LedgerKind {
    fn default() -> Self {
        LedgerKind::Expense
    }
}

impl LedgerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LedgerKind::Expense => "expense",
            LedgerKind::Income => "income",
            LedgerKind::Transfer => "transfer",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "expense" | "支出" => Some(LedgerKind::Expense),
            "income" | "收入" => Some(LedgerKind::Income),
            "transfer" | "转账" => Some(LedgerKind::Transfer),
            _ => None,
        }
    }
}

/// 分类归属侧：支出分类与收入分类是两套（转账不配分类侧）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LedgerSide {
    Expense,
    Income,
}

impl Default for LedgerSide {
    fn default() -> Self {
        LedgerSide::Expense
    }
}

impl LedgerSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            LedgerSide::Expense => "expense",
            LedgerSide::Income => "income",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "expense" | "支出" => Some(LedgerSide::Expense),
            "income" | "收入" => Some(LedgerSide::Income),
            _ => None,
        }
    }
}

/// 资金账户类型（v0.7.4 放开为自由字符串）：任意非空字符串都合法，预置清单由前端提供，
/// core 不硬编码——用户想要「公积金」「医保」「数字人民币」这类账户不该被枚举挡住。
///
/// core 唯一认识的语义值是 `credit`（信用卡）：它的负余额计入总负债
/// （`ledger balance` 与前端 `ledger.ts::assetsOverview` 同口径），其余类型只影响界面分组。
pub const ACCOUNT_KIND_CREDIT: &str = "credit";

/// 账户类型缺省值（与旧枚举 `AccountKind::Cash` 的序列化形态一致，serde 直接兼容）。
pub fn default_account_kind() -> String {
    "cash".to_string()
}

/// 资金账户。余额不存现值——当前余额 = 期初 + 流水推导，
/// 否则改一笔历史账目还要回头修余额，多端合并必然打架。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerAccount {
    pub id: String,
    pub name: String,
    /// lucide 图标名（前端 ledgerIcons 白名单）；空 = 按类型取默认
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// #rrggbb；空 = 按类型取默认色
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
    /// 账户类型（自由字符串，见 `default_account_kind`）；`credit` = 信用卡（负债口径）
    #[serde(default = "default_account_kind")]
    pub kind: String,
    /// 期初余额（分）
    #[serde(rename = "initialCents", default)]
    pub initial_cents: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub order: f64,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

/// 记账分类，两级：`parent_id = None` 的是大类，否则是它名下的子分类。
/// 支出与收入各有一套（`side`），图标与颜色都落在分类上（子分类缺省继承大类颜色）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub side: LedgerSide,
    /// 大类 id；None = 自己就是大类
    #[serde(rename = "parentId", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
    #[serde(default)]
    pub order: f64,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

/// 自定义账户类型（v0.7.5）：账本级实体，随「账本」范围同步。
/// 账户的 `kind` 仍是自由字符串——删掉类型不影响已用它的账户（kind 原样保留）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerAccountType {
    pub id: String,
    pub name: String,
    /// lucide 图标名；空 = 前端取默认
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// #rrggbb；空 = 前端取默认色
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

/// 一笔账。金额恒为正的**整数分**（i64）——浮点累加在统计里会 drift，
/// 而分是记账的最小单位，整数加减永远精确。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerEntry {
    pub id: String,
    #[serde(default)]
    pub kind: LedgerKind,
    /// 金额（分），恒为正；方向由 `kind` 决定
    #[serde(rename = "amountCents")]
    pub amount_cents: i64,
    /// 支出/转账 = 付款账户；收入 = 收款账户
    #[serde(rename = "accountId")]
    pub account_id: String,
    /// 转账的转入账户（仅 transfer 有值）
    #[serde(rename = "toAccountId", default, skip_serializing_if = "Option::is_none")]
    pub to_account_id: Option<String>,
    /// 子分类（或大类）id；转账可以为空
    #[serde(rename = "categoryId", default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// 归属日期 YYYY-MM-DD
    pub date: String,
    /// HH:MM:SS；空 = 只记到天
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub time: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// 附图裸文件名列表（存放于 `img/data/ledger/`，走条目插图通道同步）；
    /// 空 = 没有附图。图片本体不进 Excel 导出，只随实体同步。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// v0.7.4 的单图旧字段：只读不写，加载时折进 `images`（唯一的兼容例外，
    /// 见 `fold_legacy_image`）；新写入一律只序列化 `images`。
    #[serde(default, rename = "image", skip_serializing)]
    #[schemars(skip)]
    pub legacy_image: Option<String>,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl LedgerEntry {
    /// 旧单图字段折进多图列表：images 为空且 legacy 有值时 images = [legacy]。
    pub fn fold_legacy_image(&mut self) {
        if self.images.is_empty() {
            if let Some(name) = self.legacy_image.take() {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    self.images = vec![name];
                }
            }
        } else {
            self.legacy_image = None;
        }
    }
}

/// ledger.json：记账是独立的第五个领域文件。
///
/// 与日记同一条理由：记一笔账不该抬高 data 域的 revision、也不该和任务写入抢
/// 同一个文件锁与幂等台账。同步上它有自己的范围勾选（「账本」）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct LedgerFile {
    #[serde(rename = "schemaVersion", default)]
    pub schema_version: u32,
    #[serde(rename = "_meta", default)]
    pub meta: DomainMeta,
    #[serde(default)]
    pub accounts: Vec<LedgerAccount>,
    #[serde(default)]
    pub categories: Vec<LedgerCategory>,
    /// 自定义账户类型（v0.7.5）
    #[serde(rename = "accountTypes", default, skip_serializing_if = "Vec::is_empty")]
    pub account_types: Vec<LedgerAccountType>,
    #[serde(default)]
    pub entries: Vec<LedgerEntry>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl LedgerFile {
    /// 加载归一的唯一点：把 v0.7.4 条目的旧单图字段折进 `images`。
    pub fn fold_legacy_images(&mut self) {
        for entry in &mut self.entries {
            entry.fold_legacy_image();
        }
    }

    /// 首跑种子：一套覆盖日常收支场景的默认账户与两级分类。
    ///
    /// 只在 ledger.json 不存在时调用（`repo::ensure_initialized`）。子分类不存颜色——
    /// 界面上继承大类的颜色，数据保持精简；改大类颜色整族跟着变正是用户想要的。
    pub fn seed_defaults(&mut self) {
        if !self.accounts.is_empty() || !self.categories.is_empty() {
            return;
        }
        let now = crate::time::now_iso();
        // 种子 id 必须**确定性**：账本没落盘时 load_ledger 会在内存里种一份，
        // 记第一笔账按名字解析到的账户 id 要和随后落盘的那本一致，否则流水会指向
        // 不存在的账户。随机 id 会让「加载两次 = 两本不同的账」。
        let mut accounts = Vec::new();
        for (index, (name, kind, icon, color)) in [
            ("现金", "cash", "Wallet", "#e8a33d"),
            ("微信", "other", "MessageCircle", "#2aae67"),
            ("支付宝", "other", "Smartphone", "#1677ff"),
            ("储蓄卡", "debit", "Landmark", "#b23a48"),
        ]
        .iter()
        .enumerate()
        {
            accounts.push(LedgerAccount {
                id: format!("lacc-{:02}", index + 1),
                name: (*name).to_string(),
                icon: (*icon).to_string(),
                color: (*color).to_string(),
                kind: (*kind).to_string(),
                initial_cents: 0,
                note: String::new(),
                order: index as f64 + 1.0,
                created_at: now.clone(),
                updated_at: None,
                extra: Map::new(),
            });
        }
        self.accounts = accounts;

        let mut categories: Vec<LedgerCategory> = Vec::new();
        let mut group = |side: LedgerSide,
                         side_tag: &str,
                         index: usize,
                         name: &str,
                         icon: &str,
                         color: &str,
                         kids: &[(&str, &str)]| {
            let parent_id = format!("lcat-{side_tag}-{:02}", index);
            categories.push(LedgerCategory {
                id: parent_id.clone(),
                name: name.to_string(),
                side,
                parent_id: None,
                icon: icon.to_string(),
                color: color.to_string(),
                order: index as f64,
                created_at: now.clone(),
                updated_at: None,
                extra: Map::new(),
            });
            for (kid_index, (kid, kid_icon)) in kids.iter().enumerate() {
                categories.push(LedgerCategory {
                    id: format!("{parent_id}-{:02}", kid_index + 1),
                    name: (*kid).to_string(),
                    side,
                    parent_id: Some(parent_id.clone()),
                    icon: (*kid_icon).to_string(),
                    color: String::new(),
                    order: (kid_index + 1) as f64,
                    created_at: now.clone(),
                    updated_at: None,
                    extra: Map::new(),
                });
            }
        };
        group(
            LedgerSide::Expense,
            "exp",
            1,
            "餐饮",
            "Utensils",
            "#f0862c",
            &[
                ("早餐", "Coffee"),
                ("午餐", "Utensils"),
                ("晚餐", "UtensilsCrossed"),
                ("零食", "Candy"),
                ("饮料", "CupSoda"),
                ("水果", "Apple"),
                ("买菜", "Carrot"),
            ],
        );
        group(
            LedgerSide::Expense,
            "exp",
            2,
            "交通",
            "Bus",
            "#4a90d9",
            &[
                ("公交地铁", "TrainFront"),
                ("打车", "CarTaxiFront"),
                ("火车飞机", "Plane"),
                ("油费", "Fuel"),
                ("停车", "SquareParking"),
                ("单车", "Bike"),
            ],
        );
        group(
            LedgerSide::Expense,
            "exp",
            3,
            "居住",
            "House",
            "#7f8fa6",
            &[
                ("房租", "KeyRound"),
                ("水电", "Zap"),
                ("燃气", "Flame"),
                ("网费", "Wifi"),
                ("物业维修", "Wrench"),
            ],
        );
        group(
            LedgerSide::Expense,
            "exp",
            4,
            "购物",
            "ShoppingBag",
            "#e67e9c",
            &[
                ("日用百货", "ShoppingCart"),
                ("服饰鞋包", "Shirt"),
                ("数码电器", "Smartphone"),
                ("美妆护肤", "Sparkles"),
            ],
        );
        group(
            LedgerSide::Expense,
            "exp",
            5,
            "娱乐",
            "Gamepad2",
            "#9b59b6",
            &[
                ("游戏", "Gamepad2"),
                ("电影演出", "Clapperboard"),
                ("音乐会员", "Music"),
                ("运动健身", "Dumbbell"),
            ],
        );
        group(
            LedgerSide::Expense,
            "exp",
            6,
            "医疗",
            "HeartPulse",
            "#e74c3c",
            &[("药品", "Pill"), ("门诊诊疗", "Stethoscope")],
        );
        group(
            LedgerSide::Expense,
            "exp",
            7,
            "学习",
            "BookOpen",
            "#16a085",
            &[("书籍课程", "BookOpen"), ("学习办公", "PenLine")],
        );
        group(
            LedgerSide::Expense,
            "exp",
            8,
            "人情",
            "Gift",
            "#d35400",
            &[
                ("红包礼金", "Gift"),
                ("请客吃饭", "PartyPopper"),
                ("孝敬长辈", "HeartHandshake"),
            ],
        );
        group(
            LedgerSide::Expense,
            "exp",
            9,
            "宠物",
            "Dog",
            "#8e6e53",
            &[("宠物食品", "Bone"), ("宠物用品", "PawPrint")],
        );
        group(
            LedgerSide::Expense,
            "exp",
            10,
            "其他",
            "Ellipsis",
            "#95a5a6",
            &[("杂项", "Package")],
        );
        group(
            LedgerSide::Income,
            "inc",
            1,
            "工资",
            "Banknote",
            "#27ae60",
            &[
                ("工资薪金", "Banknote"),
                ("奖金", "Medal"),
                ("补贴", "Coins"),
            ],
        );
        group(
            LedgerSide::Income,
            "inc",
            2,
            "理财",
            "TrendingUp",
            "#2980b9",
            &[("利息", "Percent"), ("基金股票", "ChartLine")],
        );
        group(
            LedgerSide::Income,
            "inc",
            3,
            "兼职",
            "Briefcase",
            "#8e44ad",
            &[("外快", "Briefcase"), ("稿费", "FileText")],
        );
        group(
            LedgerSide::Income,
            "inc",
            4,
            "红包",
            "Gift",
            "#c0392b",
            &[("红包礼金", "Gift")],
        );
        group(
            LedgerSide::Income,
            "inc",
            5,
            "退款",
            "RotateCcw",
            "#7f8c8d",
            &[("退款报销", "ReceiptText")],
        );
        group(
            LedgerSide::Income,
            "inc",
            6,
            "其他",
            "Ellipsis",
            "#95a5a6",
            &[("杂项", "CircleDot")],
        );
        self.categories = categories;
    }
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
    pub transfer: TransferSettings,
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub features: FeatureSettings,
    #[serde(default)]
    pub diary: DiarySettings,
    #[serde(default)]
    pub ledger: LedgerSettings,
    #[serde(default)]
    pub toolbox: ToolboxSettings,
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
    /// 列表排序方式（跨设备共享）；合法值与前端 sort.ts 的 SortMode 一致。
    #[serde(rename = "listSortMode", default = "default_list_sort_mode")]
    pub list_sort_mode: String,
    #[serde(rename = "uiScale", default = "default_ui_scale")]
    pub ui_scale: f64,
    #[serde(rename = "uiFontSize", default = "default_ui_font_size")]
    pub ui_font_size: u32,
    #[serde(rename = "markdownFontSize", default = "default_markdown_font_size")]
    pub markdown_font_size: u32,
    #[serde(rename = "editorFontSize", default = "default_editor_font_size")]
    pub editor_font_size: u32,
    #[serde(rename = "editorWidthPercent", default = "default_editor_width_percent")]
    pub editor_width_percent: u32,
    #[serde(rename = "editorHeightPercent", default = "default_editor_height_percent")]
    pub editor_height_percent: u32,
    #[serde(rename = "tagFontSize", default = "default_tag_font_size")]
    pub tag_font_size: u32,
    /// 记账页字号（v0.7.3，本机偏好不共享）
    #[serde(rename = "ledgerFontSize", default = "default_ledger_font_size")]
    pub ledger_font_size: u32,
    /// 日记页字号（v0.7.3，本机偏好不共享）
    #[serde(rename = "diaryFontSize", default = "default_diary_font_size")]
    pub diary_font_size: u32,
    /// 显示哪些固定导航行，按显示顺序（v0.7.3）。合法 id 见 [`NAV_ITEM_IDS`]：
    /// 前四个是内置 system 节点，diary/ledger/toolbox 是前端渲染的伪行。
    #[serde(rename = "navItems", default = "default_nav_items")]
    pub nav_items: Vec<String>,
    /// 系统导航布局（v0.7.3）：`list` 单列（默认）/ `grid` 两列图标+文字 / `icons` 单行纯图标。
    #[serde(rename = "navLayout", default = "default_nav_layout")]
    pub nav_layout: String,
    #[serde(rename = "themePresets", default = "default_theme_presets")]
    pub theme_presets: Vec<ThemePreset>,
    /// 临期高亮配色：`{ 节点id: ["#rrggbb"; 4] }`（已过期/今天/明天/后天，
    /// 顺序与前端 `dueHighlight.ts::DEFAULT_DUE_COLORS` 一致），**每个页面一套**，所以按节点 id 存。
    #[serde(rename = "dueColors", default)]
    pub due_colors: Map<String, Value>,
    /// 预置标签（v0.8.1）：右键菜单「标签」面板里可一键添加的常用标签。
    /// 跨设备共享——同一份标签库在每台设备上都该能用。
    #[serde(rename = "tagPresets", default)]
    pub tag_presets: Vec<Tag>,
    /// 日记专用的预置标签（v0.8.3）：日记与工作事项的常用标签根本不是一批
    /// （「出差」「报销」对日记没意义，「今天心情」对工作事项没意义），混在一套里
    /// 两边都变难用，所以分成两套、面板里各展示各的。
    #[serde(rename = "diaryTagPresets", default)]
    pub diary_tag_presets: Vec<Tag>,
    #[serde(rename = "uiColors", default)]
    pub ui_colors: Map<String, Value>,
    #[serde(rename = "newNodeDefaults", default)]
    pub new_node_defaults: NewNodeDefaults,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

/// 新建分组/条目的默认外观（v0.7.2）：建出来的节点直接带上这套主题色与背景，
/// 省得每个新条目都要进列表菜单再调一遍。
///
/// 全部字段**空 = 没配置**，新建时跟随应用默认——给每个新节点都写一条与默认值相同的
/// 记录只会让 `uiColors` / `backgrounds` 越长越大，还会挡住以后调整默认值。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NewNodeDefaults {
    /// 主题色（#rrggbb）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accent: String,
    /// 背景色（#rrggbb）
    #[serde(rename = "backgroundColor", default, skip_serializing_if = "String::is_empty")]
    pub background_color: String,
    /// 背景图片：`img:<文件名>`（本地图，落在 img/background/）或 http(s)/data URL
    #[serde(rename = "backgroundImage", default, skip_serializing_if = "String::is_empty")]
    pub background_image: String,
    #[serde(rename = "backgroundOpacity", default = "default_new_node_background_opacity")]
    pub background_opacity: f64,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_new_node_background_opacity() -> f64 {
    0.28
}

impl Default for NewNodeDefaults {
    fn default() -> Self {
        Self {
            accent: String::new(),
            background_color: String::new(),
            background_image: String::new(),
            background_opacity: default_new_node_background_opacity(),
            extra: Map::new(),
        }
    }
}

fn default_link_open_mode() -> LinkOpenMode {
    LinkOpenMode::App
}

/// 与 src/lib/sort.ts 的 SortMode 联合类型保持一致（由 frontend_contract 测试钉住）。
pub const LIST_SORT_MODES: [&str; 7] = [
    "created-desc", "created-asc", "alpha-asc", "alpha-desc", "due-asc", "due-desc", "importance",
];

fn default_list_sort_mode() -> String {
    "created-desc".to_string()
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
fn default_editor_width_percent() -> u32 {
    72
}
fn default_editor_height_percent() -> u32 {
    86
}
fn default_tag_font_size() -> u32 {
    14
}
fn default_ledger_font_size() -> u32 {
    18
}
fn default_diary_font_size() -> u32 {
    18
}

/// 固定导航行的合法 id：前四个是 `kind:"system"` 的内置节点（注意是 `my-day` 不是
/// `today`），后三个是前端渲染的伪行（日记/记账/工具箱）。`config set appearance.navItems`
/// 拒绝这个集合之外的值——CLI/Agent 写进去一个不存在的 id 只会让那一行凭空消失。
pub const NAV_ITEM_IDS: [&str; 7] = [
    "my-day", "planned", "important", "diary", "ledger", "scheduled", "toolbox",
];

/// 可以钉进固定导航的工具 id（v0.8.3，navItems 里写作 `tool:<id>`）。
/// 与前端 `src/lib/tools/catalog.ts` 的 `TOOL_CATALOG` 是同一份清单的两份拷贝
/// （core 要在 config.set 时挡住拼错的 id，而它不 import 前端）——
/// 有 `nav_tool_ids_match_frontend_catalog` 测试用 include_str! 钉住两边。
pub const NAV_TOOL_IDS: [&str; 4] = ["random", "rmb", "scratchpad", "transfer"];

/// navItems 里工具行的前缀
pub const NAV_TOOL_PREFIX: &str = "tool:";

/// `appearance.navLayout` 的合法取值。
pub const NAV_LAYOUTS: [&str; 3] = ["list", "grid", "icons"];

pub fn default_nav_items() -> Vec<String> {
    NAV_ITEM_IDS.iter().map(|id| id.to_string()).collect()
}

fn default_nav_layout() -> String {
    "list".to_string()
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            link_open_mode: default_link_open_mode(),
            list_sort_mode: default_list_sort_mode(),
            ui_scale: default_ui_scale(),
            ui_font_size: default_ui_font_size(),
            markdown_font_size: default_markdown_font_size(),
            editor_font_size: default_editor_font_size(),
            editor_width_percent: default_editor_width_percent(),
            editor_height_percent: default_editor_height_percent(),
            tag_font_size: default_tag_font_size(),
            ledger_font_size: default_ledger_font_size(),
            diary_font_size: default_diary_font_size(),
            nav_items: default_nav_items(),
            tag_presets: Vec::new(),
            diary_tag_presets: Vec::new(),
            due_colors: Map::new(),
            nav_layout: default_nav_layout(),
            theme_presets: default_theme_presets(),
            ui_colors: Map::new(),
            new_node_defaults: NewNodeDefaults::default(),
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

fn default_due_highlight() -> String {
    "off".to_string()
}

fn default_link_render() -> String {
    "card".to_string()
}

fn default_week_start() -> String {
    "monday".to_string()
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
    /// 立即同步的应用内快捷键（默认 F5）
    #[serde(rename = "syncNow", default = "default_shortcut_sync_now")]
    pub sync_now: String,
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
fn default_shortcut_sync_now() -> String {
    "F5".to_string()
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            new_task: default_shortcut_new_task(),
            focus_search: default_shortcut_focus_search(),
            toggle_window: default_shortcut_toggle_window(),
            open_settings: default_shortcut_open_settings(),
            sync_now: default_shortcut_sync_now(),
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
    /// P2P 高级覆盖：iroh relay 地址。空 = n0 免费公共服务；`disabled` = 不用 relay
    /// （只有直连/自部署 relay 的场景才填，默认绝不能要求用户自建）
    #[serde(rename = "p2pRelay", default, skip_serializing_if = "String::is_empty")]
    pub p2p_relay: String,
    /// P2P 高级覆盖：pkarr 目录地址。空 = n0 免费公共服务（dns.iroh.link/pkarr）
    #[serde(rename = "p2pDirectory", default, skip_serializing_if = "String::is_empty")]
    pub p2p_directory: String,
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
    /// 同步日记（默认开；v0.6.x 及以前搭「同步数据」的车，v0.7.0 起独立勾选，
    /// 默认开是为了升级上来的用户行为不变）
    #[serde(rename = "syncDiary", default = "default_true")]
    pub sync_diary: bool,
    /// 同步账本（账户/分类/记账流水，默认开）
    #[serde(rename = "syncLedger", default = "default_true")]
    pub sync_ledger: bool,
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

/// 文件传输助手（v0.8.3）的本机设置。
///
/// **不参与设置同步**：传输与同步账户完全无关（口令配对），每台设备自己的 relay
/// 偏好被别的设备改掉只会莫名其妙连不上。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TransferSettings {
    /// 自选 iroh relay 地址。三态（v0.8.7 起 GUI 的三选一与这里一一对应）：
    /// `default`（**出厂默认**）= n0 公共 relay；空 = 复用同步的 `sync.p2pRelay`
    /// （再空也落到 n0 公共）；其它 = 自部署地址（`disabled` = 不用 relay，只走直连）。
    #[serde(
        rename = "relay",
        default = "default_transfer_relay",
        skip_serializing_if = "String::is_empty"
    )]
    pub relay: String,
    /// 本机在传输房间里展示的设备名（v0.8.4）。**本机偏好**：不同设备本来就该有
    /// 不同的名字，不进同步共享子集（`is_shared_settings_path` 不含 transfer.*）。
    #[serde(rename = "deviceName", default, skip_serializing_if = "String::is_empty")]
    pub device_name: String,
    /// 自动接收（默认关）：收到传输请求不再弹确认卡，直接收。
    #[serde(rename = "autoAccept", default)]
    pub auto_accept: bool,
}

/// 传输 relay 的出厂默认：n0 公共 relay（「使用默认服务」那一档，v0.8.7 追加需求）。
fn default_transfer_relay() -> String {
    "default".to_string()
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self {
            relay: default_transfer_relay(),
            device_name: String::new(),
            auto_accept: false,
        }
    }
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
            p2p_relay: String::new(),
            p2p_directory: String::new(),
            username: String::new(),
            secret: String::new(),
            sync_data: default_true(),
            sync_settings: default_true(),
            sync_schedules: false,
            sync_diary: default_true(),
            sync_ledger: default_true(),
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
    /// 显示置顶图标；与置顶分区独立，两者都关也不改变任务的置顶排序。
    #[serde(rename = "pinnedIcon", default = "default_true")]
    pub pinned_icon: bool,
    /// 置顶任务单独分区；与图标可同时开启，也可同时关闭。
    #[serde(rename = "pinnedSection", default)]
    pub pinned_section: bool,
    /// 同步功能总开关（特性开关）：关掉后同步配置隐藏、自动同步停、sync 命令拒执行、
    /// 内置主机/P2P 运行时停。默认开——已有配对的用户不该升级后静默失去同步。
    #[serde(rename = "sync", default = "default_true")]
    pub sync: bool,
    /// 编辑器 markdown 工具栏（特性开关，桌面；移动端常显不受它管）
    #[serde(rename = "editorToolbar", default = "default_true")]
    pub editor_toolbar: bool,
    /// 移动端页面左上角返回按钮（特性开关，默认关；桌面无感）
    #[serde(rename = "mobileBack", default)]
    pub mobile_back: bool,
    /// 超链接渲染样式（**三档单选**，默认 "card"）："off" 原样链接；
    /// "title" 裸链接抓网页标题按 [标题](链接) 渲染；"card" 所有超链接渲染成预览卡。
    /// v0.8.1 之前是两个独立布尔（autoLinkTitle / linkCards），四种组合里有两组等价。
    #[serde(rename = "linkRender", default = "default_link_render")]
    pub link_render: String,
    /// 一周的第一天："monday"（默认，国内习惯）或 "sunday"。日历视图、日期选择器、
    /// 周统计与「本周」分组共用这一个值。
    #[serde(rename = "weekStart", default = "default_week_start")]
    pub week_start: String,
    /// 临期高亮（v0.8.1）："off"（默认）/"solid" 按档取整色 /"gradient" 按剩余时间插值
    #[serde(rename = "dueHighlight", default = "default_due_highlight")]
    pub due_highlight: String,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl Default for FeatureSettings {
    fn default() -> Self {
        Self {
            show_category_badges: true,
            pinned_icon: true,
            pinned_section: false,
            sync: true,
            editor_toolbar: true,
            mobile_back: false,
            week_start: default_week_start(),
            due_highlight: default_due_highlight(),
            link_render: default_link_render(),
            extra: Map::new(),
        }
    }
}

/// 日记视图：列表（时间轴）/ 日历（月历 + 当日卡片）/ 分组（按年月折叠）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DiaryView {
    List,
    Calendar,
    Group,
}

impl Default for DiaryView {
    fn default() -> Self {
        DiaryView::List
    }
}

impl DiaryView {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiaryView::List => "list",
            DiaryView::Calendar => "calendar",
            DiaryView::Group => "group",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "list" => Some(DiaryView::List),
            "calendar" => Some(DiaryView::Calendar),
            "group" => Some(DiaryView::Group),
            _ => None,
        }
    }
}

/// 日记偏好。
///
/// `view` 是**本机 UI 状态**（每台设备各看各的视图，不进同步的共享子集）；
/// 主题色与背景是**外观**，跟着共享子集走——和条目背景、`appearance.uiColors`
/// 一个待遇，日记换台设备不该变脸。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiarySettings {
    #[serde(default)]
    pub view: DiaryView,
    /// 主题色（#rrggbb）；空 = 用默认日记色
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accent: String,
    /// 背景色（#rrggbb）
    #[serde(rename = "backgroundColor", default = "default_diary_background_color")]
    pub background_color: String,
    /// 背景图片：`img:<文件名>`（本地图，落在 img/background/）或 http(s)/data URL；空 = 无图
    #[serde(rename = "backgroundImage", default, skip_serializing_if = "String::is_empty")]
    pub background_image: String,
    #[serde(rename = "backgroundOpacity", default = "default_diary_background_opacity")]
    pub background_opacity: f64,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_diary_background_color() -> String {
    "#f4f1ea".to_string()
}

fn default_diary_background_opacity() -> f64 {
    0.28
}

impl Default for DiarySettings {
    fn default() -> Self {
        Self {
            view: DiaryView::default(),
            accent: String::new(),
            background_color: default_diary_background_color(),
            background_image: String::new(),
            background_opacity: default_diary_background_opacity(),
            extra: Map::new(),
        }
    }
}

/// 记账视图：列表（按天卡片）/ 日历（热力图 + 当日明细）/ 统计（曲线 + 分类占比）/ 资产（账户与总额）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LedgerView {
    List,
    Calendar,
    Stats,
    Assets,
}

impl Default for LedgerView {
    fn default() -> Self {
        LedgerView::List
    }
}

impl LedgerView {
    pub fn as_str(&self) -> &'static str {
        match self {
            LedgerView::List => "list",
            LedgerView::Calendar => "calendar",
            LedgerView::Stats => "stats",
            LedgerView::Assets => "assets",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "list" => Some(LedgerView::List),
            "calendar" => Some(LedgerView::Calendar),
            "stats" => Some(LedgerView::Stats),
            "assets" => Some(LedgerView::Assets),
            _ => None,
        }
    }
}

/// 记账偏好。与日记同一条口径：`view` 是**本机 UI 状态**不进同步共享子集，
/// 主题色与背景是**外观**跟着共享子集走。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LedgerSettings {
    #[serde(default)]
    pub view: LedgerView,
    /// 主题色（#rrggbb）；空 = 用默认记账色
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accent: String,
    /// 背景色（#rrggbb）
    #[serde(rename = "backgroundColor", default = "default_ledger_background_color")]
    pub background_color: String,
    /// 背景图片：`img:<文件名>` 或 http(s)/data URL；空 = 无图
    #[serde(rename = "backgroundImage", default, skip_serializing_if = "String::is_empty")]
    pub background_image: String,
    #[serde(rename = "backgroundOpacity", default = "default_ledger_background_opacity")]
    pub background_opacity: f64,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_ledger_background_color() -> String {
    "#eef3ee".to_string()
}

fn default_ledger_background_opacity() -> f64 {
    0.28
}

impl Default for LedgerSettings {
    fn default() -> Self {
        Self {
            view: LedgerView::default(),
            accent: String::new(),
            background_color: default_ledger_background_color(),
            background_image: String::new(),
            background_opacity: default_ledger_background_opacity(),
            extra: Map::new(),
        }
    }
}

/// 工具箱外观（v0.8.4）。与日记/记账同一口径：主题色与背景色是**外观**，
/// 跟着同步的共享子集走——换台设备打开工具箱该是同一副样子。
///
/// 字段分两层（v0.8.7）：`accent` / `backgroundColor` 是**工具箱主界面**的，
/// `toolAccents` / `toolBackgrounds` 是**每个工具子页各自的**（工具 id → #rrggbb）。
/// 工具子页没配过就跟主界面走——「每个工具单独调」与「一次调好全部」两种用法都成立。
/// 背景图与透明度还没有入口，不加没有消费者的字段。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolboxSettings {
    /// 主题色（#rrggbb）；空 = 用默认主题色
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accent: String,
    /// 背景色（#rrggbb）
    #[serde(rename = "backgroundColor", default = "default_toolbox_background_color")]
    pub background_color: String,
    /// 每个工具子页自己的主题色（工具 id → #rrggbb）；缺省 / 没配过的键跟主界面
    #[serde(rename = "toolAccents", default, skip_serializing_if = "Map::is_empty")]
    pub tool_accents: Map<String, Value>,
    /// 每个工具子页自己的背景色（工具 id → #rrggbb）
    #[serde(rename = "toolBackgrounds", default, skip_serializing_if = "Map::is_empty")]
    pub tool_backgrounds: Map<String, Value>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

fn default_toolbox_background_color() -> String {
    "#f0f0f0".to_string()
}

impl Default for ToolboxSettings {
    fn default() -> Self {
        Self {
            accent: String::new(),
            background_color: default_toolbox_background_color(),
            tool_accents: Map::new(),
            tool_backgrounds: Map::new(),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<ScheduleGate>,
    /// Inclusive local date; calendar schedules use their IANA timezone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleGate {
    pub windows: Vec<ScheduleWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<Probe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<Match>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduleWindow {
    pub start: String,
    pub end: String,
    /// 0 = Sunday .. 6 = Saturday. Overnight windows belong to the start day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weekdays: Option<Vec<u8>>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        cooldown: Option<String>,
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
