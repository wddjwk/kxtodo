//! CLI parsing and dispatch (§3.1–3.7). clap definitions are the single source
//! for help text and command schemas.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;

use crate::domain::core::{Controls, ExecOutcome, Invocation};
use crate::domain::error::{CoreError, CoreResult};
use crate::domain::render::{render, Format};

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// 本次命令使用指定数据目录（默认可执行文件同级 todo-note-data/）
    #[arg(long, global = true, value_name = "path")]
    pub data_dir: Option<PathBuf>,

    /// 输出格式
    #[arg(long, global = true, value_name = "json|pretty|table|ndjson")]
    pub format: Option<String>,

    /// --format json 的简写
    #[arg(long, global = true)]
    pub json: bool,

    /// 对成功 JSON 结果应用 jq 表达式（子集见 kxtodo schema jq）
    #[arg(short = 'q', long, global = true, value_name = "expr")]
    pub jq: Option<String>,

    /// 预执行：只校验并展示影响，不写数据、不执行动作
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// 对高风险操作表示已获得明确确认
    #[arg(long, global = true)]
    pub yes: bool,

    /// 写操作幂等键（重试返回首次结果）
    #[arg(long, global = true, value_name = "key")]
    pub idempotency_key: Option<String>,

    /// 仅在目标数据域 revision 匹配时写入
    #[arg(long, global = true, value_name = "revision")]
    pub if_revision: Option<u64>,

    /// 禁止终端颜色
    #[arg(long, global = true)]
    pub no_color: bool,

    /// 内部：以隐藏 Host 模式运行（非用户命令）
    #[arg(long, hide = true, global = true)]
    pub kxtodo_host: bool,
}

impl GlobalArgs {
    fn controls(&self) -> Controls {
        Controls {
            dry_run: self.dry_run,
            yes: self.yes,
            idempotency_key: self.idempotency_key.clone(),
            if_revision: self.if_revision,
        }
    }

    fn format(&self) -> CoreResult<Format> {
        if self.json {
            return Ok(Format::Json);
        }
        match self.format.as_deref() {
            None => Ok(Format::Json),
            Some(raw) => Format::parse(raw).ok_or_else(|| {
                CoreError::validation(
                    "INVALID_FORMAT",
                    format!("无效 --format `{raw}`，支持 json|pretty|table|ndjson"),
                )
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Command tree
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(
    name = "kxtodo",
    version,
    about = "KXToDo：本地优先的待办与 Markdown 笔记（无参数启动 GUI）",
    long_about = "KXToDo v9 CLI\n\n无参数启动 GUI；带命令时以 CLI 运行，不打开主窗口。\n默认输出 JSON envelope（ok/command/data|error/meta）。\n全局选项可放在任意命令之后；逐级帮助：kxtodo <领域> --help、kxtodo <领域> <动作> --help。",
    subcommand_required = false,
    arg_required_else_help = false,
    disable_version_flag = true
)]
pub struct Cli {
    #[command(flatten)]
    pub globals: GlobalArgs,

    /// 显示版本和数据 schema 版本
    #[arg(short = 'V', long)]
    pub version: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// 发送桌面通知（Risk: write）
    #[command(long_about = "Risk: write\n\n发送桌面通知。通知由与目标数据目录匹配的 Background Host 持有；\n默认在 Host 确认创建后立即返回，--wait 等待通知窗口关闭。\n\n输出：{ notificationId, delivered, wait }。\n\n示例：\n  kxtodo notify \"构建完成\" --title \"CI\" --tone success --duration 5s")]
    Notify(Box<NotifyArgs>),
    /// 管理分类、左栏条目和具体任务
    #[command(long_about = "管理 category（分类）、entry（条目）、item（具体任务）；system 为只读内置视图。\n\n动作：\n  add      新增\n  get      按稳定 ID 读取\n  list     结构化列表\n  find     关键词搜索\n  modify   修改/移动\n  remove   删除（high-risk-write）\n  tree     层级树\n\n逐级帮助：kxtodo task <动作> --help")]
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// 管理和运行定时任务
    #[command(long_about = "定时任务的完整定义只能通过 --spec/--patch JSON 输入（结构见 kxtodo schema schedule.spec）。\n\n动作：\n  add/validate/get/list/find/modify/remove  定义管理\n  enable/disable/run/stop/logs/status       运行控制\n  runtime list/detect/set                   脚本运行时\n\n示例流程见 kxtodo skills read kxtodo。")]
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },
    /// 查询和修改 KXTodo 设置
    #[command(long_about = "使用与 settings.json 一致的点路径；动态 map（appearance.uiColors）用 --map-key 指定键。\n\n动作：list/get/set/unset/reset/path/validate\n\n示例：\n  kxtodo config get appearance.uiScale\n  kxtodo config set notifications.position top-right\n  kxtodo config set appearance.uiColors \"#dfe8df\" --map-key \"entry-xxxx\"")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// 查看命令的机器可读输入结构（Risk: read）
    #[command(long_about = "Risk: read\n\n返回由当前版本模型/命令树生成的 JSON Schema、示例与 jq 子集说明。\n\n目标：\n  task.add / task.list / ...   任意命令的参数结构\n  schedule.spec                ScheduleSpec JSON Schema\n  schedule.patch               SchedulePatch JSON Schema（自动派生）\n  notification / match         复用结构\n  jq                           --jq 支持的子集\n\n示例：\n  kxtodo schema schedule.spec --example interval-script")]
    Schema(SchemaArgs),
    /// 列出或读取随版本发布的 Agent SKILL（Risk: read）
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// 检查数据目录、完整性、运行主机和脚本环境（Risk: read）
    Doctor(DoctorArgs),
    /// 显示版本和数据 schema 版本
    Version,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyArgs {
    /// 通知正文（与 --message 二选一）
    #[arg(value_name = "message", conflicts_with = "message_flag")]
    pub message: Option<String>,

    /// 通知正文
    #[arg(long = "message", value_name = "text")]
    pub message_flag: Option<String>,

    /// 通知标题（默认 KXToDo）
    #[arg(long, value_name = "text")]
    pub title: Option<String>,

    /// 显示时长，如 5s、5200ms
    #[arg(long, value_name = "duration")]
    pub duration: Option<String>,

    /// 通知样式
    #[arg(long, value_name = "info|success|warning|error")]
    pub tone: Option<String>,

    /// 弹窗位置
    #[arg(long, value_name = "bottom-right|top-right|bottom-left|top-left")]
    pub position: Option<String>,

    /// 等待通知窗口关闭后再结束
    #[arg(long)]
    pub wait: bool,
}

#[derive(Debug, Subcommand)]
pub enum TaskAction {
    /// 新增 category、entry 或 item（Risk: write）
    #[command(
        visible_alias = "create",
        long_about = "Risk: write\n\n新增对象。--type category/entry 需要 --name；--type item 需要 --entry-id 与 --markdown/--markdown-file。\n\n互斥：--markdown 与 --markdown-file 二选一。\n输出：创建后的完整资源（含系统生成的 ID）。\n\n示例：\n  kxtodo task add --type category --name \"2026\"\n  kxtodo task add --type entry --name \"0727-0731\" --parent-id category-xxxx\n  kxtodo task add --type item --entry-id entry-xxxx --markdown \"完成 XXX 需求\" --due-date 2026-07-31 --tag \"blue:需求\"\n\n建议：Agent 调用使用 --idempotency-key 防重试重复创建。"
    )]
    Add(TaskAddArgs),
    /// 按稳定 ID 获取完整对象（Risk: read）
    #[command(long_about = "Risk: read\n\n按 --type 与 --id 精确读取；--type 与实际类型不符时返回冲突错误。\n\n输出：完整资源；item 含 Markdown 全文与位置上下文；category/entry 含计数摘要。")]
    Get(TypeIdArgs),
    /// 结构化列表（Risk: read）
    #[command(long_about = "Risk: read\n\n无关键词的结构化查询。category/entry 用 --parent-id <id|root>（--recursive 含后代）；\nitem 用 --entry-id / --category-id [--recursive]，并可叠加状态、标签、时间范围、排序与分页。\n\n时间范围：--created-from/to、--updated-from/to、--changed-from/to、--planned-from/to、--due-from/to、--completed-from/to\n（changed = 创建或修改落入范围；值为 ISO 时间或 YYYY-MM-DD）。\n排序：--sort createdAt|updatedAt|dueDate|completedAt|name|position，--order asc|desc。\n分页：--limit（默认 50）、--cursor、--all。\n\n示例：\n  kxtodo task list --type item --entry-id entry-xxxx --status all\n  kxtodo task list --type item --changed-from 2026-07-20 --changed-to 2026-07-27 --all")]
    List(TaskListArgs),
    /// 关键词搜索（Risk: read）
    #[command(
        visible_alias = "search",
        long_about = "Risk: read\n\n全文搜索：system/category/entry 名称与路径、item 的 Markdown 和标签文本。\n--type 默认 all；可叠加 list 的同类型过滤条件。\n\n注意：只有时间/状态/位置范围而没有关键词时，应使用 task list 而不是 find。"
    )]
    Find(TaskFindArgs),
    /// 修改或移动对象（Risk: write）
    #[command(
        visible_alias = "update",
        long_about = "Risk: write\n\n按稳定 ID 修改。category/entry 支持 --name/--icon/--parent-id（移动）/--collapsed；\nitem 支持 --entry-id（移动）、--markdown、布尔状态、日期（--clear-* 清空）、标签与 emoji 管理。\n\n标签：--add-tag \"blue:需求\"（可重复）；--remove-tag <tag-id>；--replace-tags \"red:a\" \"blue:b\"（整体替换）。\nEmoji：--add-emoji / --remove-emoji / --replace-emojis。\n完成语义：勾选完成时记录 completedAt，取消时清除；有效修改更新 updatedAt。"
    )]
    Modify(TaskModifyArgs),
    /// 删除对象（Risk: high-risk-write）
    #[command(
        visible_alias = "delete",
        long_about = "Risk: high-risk-write\n\n删除对象。非空 entry/category 必须显式 --cascade；级联删除同时处理后代节点、任务、背景与关联图片。\n未带 --yes 返回退出码 10；--dry-run 返回将删除的节点、任务与图片数量。\n\n示例：\n  kxtodo task remove --type item --id task-xxxx --yes\n  kxtodo task remove --type entry --id entry-xxxx --cascade --yes"
    )]
    Remove(TaskRemoveArgs),
    /// 查看 category/entry 层级树及任务计数（Risk: read）
    #[command(long_about = "Risk: read\n\n输出层级树（不内嵌 item）；需要任务详情时用 task list --type item --entry-id ...。\n\n示例：kxtodo task tree --root-id category-xxxx --depth 3 --include-counts")]
    Tree(TaskTreeArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAddArgs {
    /// 对象类型
    #[arg(long, value_name = "category|entry|item")]
    pub r#type: String,
    /// 名称（category/entry 必填）
    #[arg(long, value_name = "text")]
    pub name: Option<String>,
    /// 父级分类 ID 或 root
    #[arg(long, value_name = "category-id|root")]
    pub parent_id: Option<String>,
    /// 图标名或 emoji
    #[arg(long, value_name = "icon")]
    pub icon: Option<String>,
    /// 是否折叠（category）
    #[arg(long, value_name = "true|false")]
    pub collapsed: Option<bool>,
    /// 归属条目 ID（item 必填）
    #[arg(long, value_name = "entry-id")]
    pub entry_id: Option<String>,
    /// Markdown 正文（item，与 --markdown-file 二选一）
    #[arg(long, value_name = "text", conflicts_with = "markdown_file")]
    pub markdown: Option<String>,
    /// 从文件或标准输入读取 Markdown
    #[arg(long, value_name = "path|-")]
    pub markdown_file: Option<String>,
    /// 完成状态
    #[arg(long, value_name = "true|false")]
    pub completed: Option<bool>,
    /// 重要标记
    #[arg(long, value_name = "true|false")]
    pub important: Option<bool>,
    /// 加入我的一天
    #[arg(long, value_name = "true|false")]
    pub my_day: Option<bool>,
    /// 计划日期 YYYY-MM-DD（或 +Nd）
    #[arg(long, value_name = "date")]
    pub planned_date: Option<String>,
    /// 截止日期 YYYY-MM-DD（或 +Nd）
    #[arg(long, value_name = "date")]
    pub due_date: Option<String>,
    /// 标签 <color>:<text>（可重复；颜色 red/yellow/blue/green/gray）
    #[arg(long = "tag", value_name = "color:text")]
    pub tags: Vec<String>,
    /// Emoji（可重复）
    #[arg(long = "emoji", value_name = "emoji")]
    pub emojis: Vec<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeIdArgs {
    /// 对象类型
    #[arg(long, value_name = "system|category|entry|item")]
    pub r#type: String,
    /// 稳定 ID
    #[arg(long, value_name = "id")]
    pub id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListArgs {
    /// 对象类型
    #[arg(long, value_name = "system|category|entry|item")]
    pub r#type: String,
    /// 父级分类 ID 或 root（category/entry）
    #[arg(long, value_name = "category-id|root")]
    pub parent_id: Option<String>,
    /// 包含全部后代
    #[arg(long)]
    pub recursive: bool,
    /// 条目 ID（item）
    #[arg(long, value_name = "entry-id")]
    pub entry_id: Option<String>,
    /// 分类 ID（item，配 --recursive 含后代条目）
    #[arg(long, value_name = "category-id")]
    pub category_id: Option<String>,
    /// 完成状态过滤
    #[arg(long, value_name = "open|completed|all")]
    pub status: Option<String>,
    /// 重要过滤
    #[arg(long, value_name = "true|false")]
    pub important: Option<bool>,
    /// 我的一天过滤
    #[arg(long, value_name = "true|false")]
    pub my_day: Option<bool>,
    /// 标签文本或 ID（可重复）
    #[arg(long = "tag", value_name = "text-or-id")]
    pub tags: Vec<String>,
    /// 创建时间范围（ISO 或 YYYY-MM-DD）
    #[arg(long, value_name = "time")]
    pub created_from: Option<String>,
    #[arg(long, value_name = "time")]
    pub created_to: Option<String>,
    /// 修改时间范围
    #[arg(long, value_name = "time")]
    pub updated_from: Option<String>,
    #[arg(long, value_name = "time")]
    pub updated_to: Option<String>,
    /// 创建或修改时间范围
    #[arg(long, value_name = "time")]
    pub changed_from: Option<String>,
    #[arg(long, value_name = "time")]
    pub changed_to: Option<String>,
    /// 计划日期范围
    #[arg(long, value_name = "date")]
    pub planned_from: Option<String>,
    #[arg(long, value_name = "date")]
    pub planned_to: Option<String>,
    /// 截止日期范围
    #[arg(long, value_name = "date")]
    pub due_from: Option<String>,
    #[arg(long, value_name = "date")]
    pub due_to: Option<String>,
    /// 完成时间范围
    #[arg(long, value_name = "time")]
    pub completed_from: Option<String>,
    #[arg(long, value_name = "time")]
    pub completed_to: Option<String>,
    /// 排序键
    #[arg(long, value_name = "createdAt|updatedAt|dueDate|completedAt|name|position")]
    pub sort: Option<String>,
    /// 排序方向
    #[arg(long, value_name = "asc|desc")]
    pub order: Option<String>,
    /// 每页数量（默认 50）
    #[arg(long, value_name = "n")]
    pub limit: Option<u64>,
    /// 分页游标
    #[arg(long, value_name = "cursor")]
    pub cursor: Option<String>,
    /// 输出全部（忽略分页）
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFindArgs {
    /// 关键词（必填）
    #[arg(long, value_name = "text")]
    pub query: String,
    /// 类型范围（默认 all）
    #[arg(long, value_name = "system|category|entry|item|all")]
    pub r#type: Option<String>,
    /// 完成状态过滤
    #[arg(long, value_name = "open|completed|all")]
    pub status: Option<String>,
    /// 重要过滤
    #[arg(long, value_name = "true|false")]
    pub important: Option<bool>,
    /// 我的一天过滤
    #[arg(long, value_name = "true|false")]
    pub my_day: Option<bool>,
    /// 条目 ID（item）
    #[arg(long, value_name = "entry-id")]
    pub entry_id: Option<String>,
    /// 分类 ID（item）
    #[arg(long, value_name = "category-id")]
    pub category_id: Option<String>,
    /// 包含后代
    #[arg(long)]
    pub recursive: bool,
    /// 标签文本或 ID（可重复）
    #[arg(long = "tag", value_name = "text-or-id")]
    pub tags: Vec<String>,
    /// 排序键
    #[arg(long, value_name = "createdAt|updatedAt|dueDate|completedAt|name|position")]
    pub sort: Option<String>,
    /// 排序方向
    #[arg(long, value_name = "asc|desc")]
    pub order: Option<String>,
    /// 每页数量
    #[arg(long, value_name = "n")]
    pub limit: Option<u64>,
    /// 分页游标
    #[arg(long, value_name = "cursor")]
    pub cursor: Option<String>,
    /// 输出全部
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskModifyArgs {
    /// 对象类型
    #[arg(long, value_name = "category|entry|item")]
    pub r#type: String,
    /// 稳定 ID
    #[arg(long, value_name = "id")]
    pub id: String,
    /// 新名称
    #[arg(long, value_name = "text")]
    pub name: Option<String>,
    /// 新图标
    #[arg(long, value_name = "icon")]
    pub icon: Option<String>,
    /// 移动到分类或 root
    #[arg(long, value_name = "category-id|root")]
    pub parent_id: Option<String>,
    /// 折叠状态（category）
    #[arg(long, value_name = "true|false")]
    pub collapsed: Option<bool>,
    /// 移动 item 到条目
    #[arg(long, value_name = "entry-id")]
    pub entry_id: Option<String>,
    /// 替换 Markdown
    #[arg(long, value_name = "text", conflicts_with = "markdown_file")]
    pub markdown: Option<String>,
    /// 从文件或标准输入读取 Markdown
    #[arg(long, value_name = "path|-")]
    pub markdown_file: Option<String>,
    /// 完成状态
    #[arg(long, value_name = "true|false")]
    pub completed: Option<bool>,
    /// 重要标记
    #[arg(long, value_name = "true|false")]
    pub important: Option<bool>,
    /// 我的一天
    #[arg(long, value_name = "true|false")]
    pub my_day: Option<bool>,
    /// 计划日期
    #[arg(long, value_name = "date")]
    pub planned_date: Option<String>,
    /// 截止日期
    #[arg(long, value_name = "date")]
    pub due_date: Option<String>,
    /// 清空计划日期
    #[arg(long)]
    pub clear_planned_date: bool,
    /// 清空截止日期
    #[arg(long)]
    pub clear_due_date: bool,
    /// 添加标签（可重复）
    #[arg(long = "add-tag", value_name = "color:text")]
    pub add_tags: Vec<String>,
    /// 按 ID 删除标签（可重复）
    #[arg(long = "remove-tag", value_name = "tag-id")]
    pub remove_tags: Vec<String>,
    /// 整体替换标签列表
    #[arg(long = "replace-tags", value_name = "color:text", num_args = 0..)]
    pub replace_tags: Option<Vec<String>>,
    /// 添加 emoji（可重复）
    #[arg(long = "add-emoji", value_name = "emoji")]
    pub add_emojis: Vec<String>,
    /// 删除 emoji（可重复）
    #[arg(long = "remove-emoji", value_name = "emoji")]
    pub remove_emojis: Vec<String>,
    /// 整体替换 emoji 列表
    #[arg(long = "replace-emojis", value_name = "emoji", num_args = 0..)]
    pub replace_emojis: Option<Vec<String>>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRemoveArgs {
    /// 对象类型
    #[arg(long, value_name = "category|entry|item")]
    pub r#type: String,
    /// 稳定 ID
    #[arg(long, value_name = "id")]
    pub id: String,
    /// 级联删除非空节点
    #[arg(long)]
    pub cascade: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTreeArgs {
    /// 只显示指定节点子树
    #[arg(long, value_name = "id")]
    pub root_id: Option<String>,
    /// 最大深度
    #[arg(long, value_name = "n")]
    pub depth: Option<u64>,
    /// 包含任务计数（默认 true）
    #[arg(long, value_name = "true|false")]
    pub include_counts: Option<bool>,
}

#[derive(Debug, Subcommand)]
pub enum ScheduleAction {
    /// 使用完整 ScheduleSpec 新增定时任务（Risk: write；enabled+代码执行为 high-risk-write）
    #[command(long_about = "Risk: write（spec.enabled=true 且含代码执行时 high-risk-write，需要 --yes）\n\n唯一完整输入为 --spec JSON（'<json>' | @file | -）。结构见 kxtodo schema schedule.spec。\n默认创建为 disabled；spec 中显式 \"enabled\": true 才启用。\n相对路径以当前工作目录解析后保存。\n\n示例：\n  kxtodo schedule add --spec @schedule.json --yes --idempotency-key weekly-v1")]
    Add(ScheduleSpecArgs),
    /// 校验 spec 或 patch，不写入、不执行（Risk: read）
    #[command(long_about = "Risk: read\n\n校验 --spec（完整定义）或 --id + --patch（结合当前 spec 校验最终对象）。\n检查 schema、路径、runtime、cron/时区与模板变量；输出规范化 spec 与 warnings。")]
    Validate(ScheduleValidateArgs),
    /// 获取完整 spec 和运行状态（Risk: read）
    Get(ScheduleIdArgs),
    /// 结构化列出定时任务（Risk: read）
    #[command(long_about = "Risk: read\n\n过滤：--enabled、--status、--trigger-type；排序 --sort name|createdAt|updatedAt|lastRunAt|nextRunAt；\n分页 --limit/--cursor/--all。默认 JSON 返回完整 spec 与 state。")]
    List(ScheduleListArgs),
    /// 按关键词搜索定时任务（Risk: read）
    #[command(alias = "search", long_about = "Risk: read\n\n搜索名称、脚本路径与通知文本；其余过滤同 schedule list。")]
    Find(ScheduleFindArgs),
    /// 使用 SchedulePatch 修改定时任务（Risk: write）
    #[command(long_about = "Risk: write\n\n--patch 使用统一 patch 语义：缺省不变、null 清除、数组合并整体替换；\n修改 trigger.type/action.type/source.type 时需提供该分支完整对象；\n不允许 id/runCount/lastStatus 等运行时字段。结构见 kxtodo schema schedule.patch。\n\n示例：kxtodo schedule modify --id schedule-xxxx --patch '{\"trigger\":{\"every\":\"30m\"}}'")]
    Modify(SchedulePatchArgs),
    /// 删除定时任务（Risk: high-risk-write）
    #[command(long_about = "Risk: high-risk-write\n\n删除定义；正在运行时先停止并回收子进程。需要 --yes；--dry-run 预览。")]
    Remove(ScheduleIdArgs),
    /// 启用任务（Risk: write；代码执行需 --yes）
    #[command(long_about = "Risk: write（script/executable 需 --yes）\n\n启用前重新校验 spec/runtime/path；重算 nextRunAt。")]
    Enable(ScheduleIdArgs),
    /// 禁用任务（Risk: write）
    #[command(long_about = "Risk: write\n\n阻止后续运行但不终止当前实例；立即终止用 schedule stop。")]
    Disable(ScheduleIdArgs),
    /// 立即执行一次（Risk: high-risk-write）
    #[command(long_about = "Risk: high-risk-write\n\n默认入队后立即返回；--wait 等待最终退出码与输出。代码执行需要 --yes。")]
    Run(ScheduleRunArgs),
    /// 终止正在运行的任务（Risk: write）
    Stop(ScheduleIdArgs),
    /// 查看运行历史、stdout 和 stderr（Risk: read）
    Logs(ScheduleLogsArgs),
    /// 查看 Host、运行任务和下次执行时间（Risk: read）
    Status,
    /// 脚本运行时管理
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSpecArgs {
    /// 完整 ScheduleSpec：'<json>' | @file | -
    #[arg(long, value_name = "json|@file|-")]
    pub spec: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleValidateArgs {
    /// 完整 ScheduleSpec
    #[arg(long, value_name = "json|@file|-", conflicts_with = "patch", required_unless_present = "patch")]
    pub spec: Option<String>,
    /// 目标任务 ID（校验 patch 时必填）
    #[arg(long, value_name = "id", requires = "patch")]
    pub id: Option<String>,
    /// SchedulePatch
    #[arg(long, value_name = "json|@file|-", requires = "id")]
    pub patch: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleIdArgs {
    /// 定时任务 ID
    #[arg(long, value_name = "id")]
    pub id: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleListArgs {
    /// 启用状态过滤
    #[arg(long, value_name = "true|false")]
    pub enabled: Option<bool>,
    /// 运行状态过滤
    #[arg(long, value_name = "idle|running|success|failed|stopped|all")]
    pub status: Option<String>,
    /// 触发器类型过滤
    #[arg(long, value_name = "once|interval|calendar|condition")]
    pub trigger_type: Option<String>,
    /// 排序键
    #[arg(long, value_name = "name|createdAt|updatedAt|lastRunAt|nextRunAt")]
    pub sort: Option<String>,
    /// 排序方向
    #[arg(long, value_name = "asc|desc")]
    pub order: Option<String>,
    /// 每页数量
    #[arg(long, value_name = "n")]
    pub limit: Option<u64>,
    /// 分页游标
    #[arg(long, value_name = "cursor")]
    pub cursor: Option<String>,
    /// 输出全部
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleFindArgs {
    /// 关键词
    #[arg(long, value_name = "text")]
    pub query: String,
    /// 启用状态过滤
    #[arg(long, value_name = "true|false")]
    pub enabled: Option<bool>,
    /// 运行状态过滤
    #[arg(long, value_name = "idle|running|success|failed|stopped|all")]
    pub status: Option<String>,
    /// 触发器类型过滤
    #[arg(long, value_name = "once|interval|calendar|condition")]
    pub trigger_type: Option<String>,
    /// 排序键
    #[arg(long, value_name = "name|createdAt|updatedAt|lastRunAt|nextRunAt")]
    pub sort: Option<String>,
    /// 排序方向
    #[arg(long, value_name = "asc|desc")]
    pub order: Option<String>,
    /// 每页数量
    #[arg(long, value_name = "n")]
    pub limit: Option<u64>,
    /// 分页游标
    #[arg(long, value_name = "cursor")]
    pub cursor: Option<String>,
    /// 输出全部
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePatchArgs {
    /// 定时任务 ID
    #[arg(long, value_name = "id")]
    pub id: String,
    /// SchedulePatch：'<json>' | @file | -
    #[arg(long, value_name = "json|@file|-")]
    pub patch: String,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRunArgs {
    /// 定时任务 ID
    #[arg(long, value_name = "id")]
    pub id: String,
    /// 等待执行完成并返回退出码与输出
    #[arg(long)]
    pub wait: bool,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleLogsArgs {
    /// 定时任务 ID
    #[arg(long, value_name = "id")]
    pub id: String,
    /// 最近 N 次记录（默认 20）
    #[arg(long, value_name = "n")]
    pub limit: Option<u64>,
}

#[derive(Debug, Subcommand)]
pub enum RuntimeAction {
    /// 查看脚本运行时（Risk: read）
    List,
    /// 重新探测可用运行时（Risk: write）
    Detect,
    /// 设置 python/node/pwsh/bash/make 路径（Risk: write）
    Set(RuntimeSetArgs),
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSetArgs {
    /// 运行时名称
    #[arg(value_name = "python|node|pwsh|bash|make")]
    pub name: String,
    /// 解释器路径（空字符串清除自定义）
    #[arg(value_name = "path")]
    pub path: String,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// 列出配置，可按前缀过滤（Risk: read）
    List(ConfigListArgs),
    /// 获取一个配置项（Risk: read）
    Get(ConfigPathArgs),
    /// 设置一个配置项（Risk: write）
    #[command(long_about = "Risk: write\n\n值来源三选一：位置值、--json-value、--value-file。\nset 先完整校验再原子写入；涉及开机启动/全局快捷键/缩放时由运行主机同步应用。\n\n示例：\n  kxtodo config set appearance.uiScale 0.85\n  kxtodo config set appearance.uiColors \"#dfe8df\" --map-key \"entry-xxxx\"\n  kxtodo config set appearance.themePresets --json-value '[{\"name\":\"项目蓝\",\"color\":\"#dbeafe\"}]'")]
    Set(ConfigSetArgs),
    /// 清除可选配置（map 项），恢复默认继承（Risk: write）
    Unset(ConfigPathArgs),
    /// 将一个配置分支或全部设置恢复默认值（Risk: high-risk-write）
    #[command(long_about = "Risk: high-risk-write\n\n恢复默认值；需要 --yes；--dry-run 返回将恢复的字段及前后值。\n\n示例：kxtodo config reset appearance --yes")]
    Reset(ConfigResetArgs),
    /// 显示数据目录及各数据文件的实际路径（Risk: read）
    Path,
    /// 校验配置和引用的本地资源（Risk: read）
    Validate,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigListArgs {
    /// 前缀过滤，如 appearance
    #[arg(long, value_name = "prefix")]
    pub prefix: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPathArgs {
    /// 配置点路径
    #[arg(value_name = "path")]
    pub path: String,
    /// 动态 map 的键（如 entry ID）
    #[arg(long, value_name = "key")]
    pub map_key: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetArgs {
    /// 配置点路径
    #[arg(value_name = "path")]
    pub path: String,
    /// 配置值（位置值）
    #[arg(value_name = "value", conflicts_with_all = ["json_value", "value_file"])]
    pub value: Option<String>,
    /// JSON 形式配置值
    #[arg(long, value_name = "json", conflicts_with_all = ["value", "value_file"])]
    pub json_value: Option<String>,
    /// 从文件读取 JSON 配置值
    #[arg(long, value_name = "path", conflicts_with_all = ["value", "json_value"])]
    pub value_file: Option<String>,
    /// 动态 map 的键
    #[arg(long, value_name = "key")]
    pub map_key: Option<String>,
}

#[derive(Debug, Args, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResetArgs {
    /// 配置分支前缀（缺省重置全部）
    #[arg(value_name = "prefix")]
    pub prefix: Option<String>,
}

#[derive(Debug, Args)]
pub struct SchemaArgs {
    /// 目标：命令（如 task.add）、schedule.spec、schedule.patch、notification、match、jq
    #[arg(value_name = "target")]
    pub target: String,
    /// 输出指定名称的校验示例（仅 schedule.spec）
    #[arg(long, value_name = "name")]
    pub example: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum SkillsAction {
    /// 列出发布的 SKILL（Risk: read）
    List,
    /// 读取 SKILL 正文（Risk: read）
    Read(SkillsReadArgs),
    /// 返回 SKILL 文件实际路径（Risk: read）
    Path,
    /// 校验 SKILL 与当前 CLI 的一致性（Risk: read）
    Validate,
}

#[derive(Debug, Args)]
pub struct SkillsReadArgs {
    /// SKILL 名称（当前版本为 kxtodo）
    #[arg(value_name = "name")]
    pub name: String,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// 只运行指定检查项
    #[arg(long, value_name = "name")]
    pub check: Option<String>,
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub enum Dispatch {
    Gui,
    HiddenHost,
    Exit(i32),
}

pub struct CliOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Host routing mode (tests can force standalone execution).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    Auto,
    Local,
}

pub fn dispatch(args: &[String]) -> Dispatch {
    if args.is_empty() {
        return Dispatch::Gui;
    }
    let cli = match Cli::try_parse_from(std::iter::once("kxtodo").chain(args.iter().map(String::as_str))) {
        Ok(cli) => cli,
        Err(error) => {
            // GUI 子系统 exe 的 stdout/stderr 未连接控制台，
            // 必须经 AttachConsole + WriteConsoleW 输出。
            let rendered = format!("{error}");
            print_cli_text(&rendered, error.use_stderr());
            let code = match error.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
                _ => 2,
            };
            return Dispatch::Exit(code);
        }
    };
    if cli.globals.kxtodo_host {
        return Dispatch::HiddenHost;
    }
    if cli.version && cli.command.is_none() {
        let output = run_cli(args, &std::env::current_dir().unwrap_or_default(), Routing::Auto);
        print_output(&output);
        return Dispatch::Exit(output.code);
    }
    if cli.command.is_none() {
        return Dispatch::Gui;
    }
    let output = run_cli(args, &std::env::current_dir().unwrap_or_default(), Routing::Auto);
    print_output(&output);
    Dispatch::Exit(output.code)
}

/// Console 子系统下 stdout/stderr 天然连接，直接输出即可。
/// 所有终端（pwsh / cmd / Git Bash / WSL interop）均正常工作。
#[cfg(desktop)]
pub fn print_cli_text(message: &str, is_stderr: bool) {
    use std::io::Write;
    if is_stderr {
        let _ = std::io::stderr().write_all(message.as_bytes());
        let _ = std::io::stderr().write_all(b"\n");
        let _ = std::io::stderr().flush();
    } else {
        let _ = std::io::stdout().write_all(message.as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
        let _ = std::io::stdout().flush();
    }
}

fn print_output(output: &CliOutput) {
    if !output.stdout.is_empty() {
        print_cli_text(&output.stdout, false);
    }
    if !output.stderr.is_empty() {
        print_cli_text(&output.stderr, true);
    }
}

// ---------------------------------------------------------------------------
// run_cli (also used by tests)
// ---------------------------------------------------------------------------

pub fn run_cli(args: &[String], cwd: &Path, routing: Routing) -> CliOutput {
    let cli = match Cli::try_parse_from(std::iter::once("kxtodo").chain(args.iter().map(String::as_str))) {
        Ok(cli) => cli,
        Err(error) => {
            return CliOutput {
                code: match error.kind() {
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
                    _ => 2,
                },
                stdout: String::new(),
                stderr: error.to_string(),
            };
        }
    };
    match build_invocation(&cli) {
        Ok(Some((invocation, payload_note))) => {
            let _ = payload_note;
            // Pure local commands (schema/skills) carry prebuilt data; render directly.
            if invocation.command.starts_with("schema.") || invocation.command.starts_with("skills.") {
                let outcome = ExecOutcome {
                    code: 0,
                    envelope: crate::domain::envelope::success(
                        &invocation.command,
                        invocation.params.clone(),
                        crate::domain::envelope::Meta::default(),
                    ),
                };
                return finish(outcome, &cli.globals);
            }
            let data_dir = cli
                .globals
                .data_dir
                .clone()
                .unwrap_or_else(default_data_dir);
            let outcome = crate::domain::host::route(&invocation, &data_dir, cwd, routing);
            finish(outcome, &cli.globals)
        }
        Ok(None) => CliOutput {
            code: 0,
            stdout: String::new(),
            stderr: "无命令".to_string(),
        },
        Err(error) => {
            let envelope = crate::domain::envelope::failure(
                "cli",
                &error,
                crate::domain::envelope::Meta::default(),
            );
            CliOutput {
                code: error.exit_code(),
                stdout: String::new(),
                stderr: serde_json::to_string_pretty(&envelope).unwrap_or_default(),
            }
        }
    }
}

fn finish(outcome: ExecOutcome, globals: &GlobalArgs) -> CliOutput {
    let format = match globals.format() {
        Ok(format) => format,
        Err(error) => {
            let envelope = crate::domain::envelope::failure(
                "cli",
                &error,
                crate::domain::envelope::Meta::default(),
            );
            return CliOutput {
                code: error.exit_code(),
                stdout: String::new(),
                stderr: serde_json::to_string_pretty(&envelope).unwrap_or_default(),
            };
        }
    };
    let (code, stdout, stderr) = render(&outcome, format, globals.jq.as_deref());
    CliOutput {
        code,
        stdout,
        stderr,
    }
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("todo-note-data");
        }
    }
    PathBuf::from("todo-note-data")
}

// ---------------------------------------------------------------------------
// Invocation building (parse → params)
// ---------------------------------------------------------------------------

fn serialize_args<T: Serialize>(args: &T) -> Value {
    let mut value = serde_json::to_value(args).unwrap_or(Value::Null);
    // Absent flags serialize as null; strip them so executors see "unset".
    if let Some(map) = value.as_object_mut() {
        map.retain(|_, item| !item.is_null());
    }
    value
}

fn build_invocation(cli: &Cli) -> CoreResult<Option<(Invocation, Option<String>)>> {
    let Some(command) = &cli.command else {
        if cli.version {
            return Ok(Some((Invocation::new("version", Value::Null), None)));
        }
        return Ok(None);
    };
    let controls = cli.globals.controls();
    let mut invocation = match command {
        Commands::Version => Invocation::new("version", Value::Null),
        Commands::Notify(args) => {
            let mut params = serialize_args(&**args);
            let message = args
                .message
                .clone()
                .or_else(|| args.message_flag.clone())
                .unwrap_or_default();
            if message.trim().is_empty() {
                return Err(CoreError::validation(
                    "MESSAGE_REQUIRED",
                    "notify 需要通知正文（位置参数或 --message）",
                ));
            }
            params["message"] = Value::String(message);
            params.as_object_mut().map(|map| map.remove("messageFlag"));
            Invocation::new("notify", params)
        }
        Commands::Task { action } => build_task_invocation(action)?,
        Commands::Schedule { action } => build_schedule_invocation(action)?,
        Commands::Config { action } => build_config_invocation(action)?,
        Commands::Schema(args) => return build_schema_output(cli, args).map(Some),
        Commands::Skills { action } => return build_skills_output(cli, action).map(Some),
        Commands::Doctor(_args) => Invocation::new("doctor", Value::Null),
    };
    invocation.controls = controls;
    Ok(Some((invocation, None)))
}

/// Pure commands executed locally without a Repository (schema/skills).
fn local_pure_output(command: &str, data: Value) -> Invocation {
    let mut inv = Invocation::new(command, Value::Null);
    inv.params = data;
    inv
}

fn build_schema_output(cli: &Cli, args: &SchemaArgs) -> CoreResult<(Invocation, Option<String>)> {
    let target = args.target.as_str();
    let data = if let Some(example_name) = &args.example {
        if target != "schedule.spec" {
            return Err(CoreError::validation(
                "INVALID_SCHEMA_TARGET",
                "--example 仅支持 schedule.spec",
            ));
        }
        let example = crate::domain::schema::spec_example(example_name).ok_or_else(|| {
            CoreError::not_found(
                "EXAMPLE_NOT_FOUND",
                format!(
                    "未知示例 `{example_name}`，可用：{}",
                    crate::domain::schema::example_names().join(", ")
                ),
            )
        })?;
        // Examples must validate against the real model.
        crate::domain::ops_schedule::validate_spec_value(&example, &Default::default())?;
        serde_json::json!({ "name": example_name, "example": example })
    } else {
        match target {
            "schedule.spec" => crate::domain::schema::spec_schema(),
            "schedule.patch" => crate::domain::schema::patch_schema(),
            "notification" => crate::domain::schema::notification_schema(),
            "match" => crate::domain::schema::match_schema(),
            "jq" => crate::domain::schema::jq_schema(),
            other => {
                let root = Cli::command();
                crate::domain::schema::command_schema(&root, other)?
            }
        }
    };
    let invocation = local_pure_output(
        &format!("schema.{target}"),
        data,
    );
    let _ = cli;
    Ok((invocation, Some("pure".to_string())))
}

fn build_skills_output(cli: &Cli, action: &SkillsAction) -> CoreResult<(Invocation, Option<String>)> {
    let (command, data) = match action {
        SkillsAction::List => ("skills.list", crate::domain::skills::cmd_list()?),
        SkillsAction::Read(args) => ("skills.read", crate::domain::skills::cmd_read(&args.name)?),
        SkillsAction::Path => ("skills.path", crate::domain::skills::cmd_path()?),
        SkillsAction::Validate => {
            let catalog = command_catalog();
            ("skills.validate", crate::domain::skills::cmd_validate(&catalog)?)
        }
    };
    let _ = cli;
    Ok((local_pure_output(command, data), Some("pure".to_string())))
}

/// Dotted names of every executable command (for skills validate + schema lookup).
pub fn command_catalog() -> Vec<String> {
    use clap::CommandFactory;
    let root = Cli::command();
    let mut out = Vec::new();
    fn walk(cmd: &clap::Command, prefix: &str, out: &mut Vec<String>) {
        for sub in cmd.get_subcommands() {
            let name = if prefix.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{prefix}.{}", sub.get_name())
            };
            out.push(name.clone());
            walk(sub, &name, out);
        }
    }
    walk(&root, "", &mut out);
    out
}

fn build_task_invocation(action: &TaskAction) -> CoreResult<Invocation> {
    let (name, params) = match action {
        TaskAction::Add(args) => {
            let mut params = serialize_args(args);
            if args.r#type == "item" {
                let markdown = resolve_markdown(args.markdown.clone(), args.markdown_file.clone())?
                    .ok_or_else(|| {
                        CoreError::validation("MARKDOWN_REQUIRED", "--type item 需要 --markdown 或 --markdown-file")
                    })?;
                params["markdown"] = Value::String(markdown);
            } else if args.markdown.is_some() || args.markdown_file.is_some() {
                return Err(CoreError::validation(
                    "MARKDOWN_UNEXPECTED",
                    "--markdown 仅用于 --type item",
                ));
            }
            params.as_object_mut().map(|map| map.remove("markdownFile"));
            ("task.add", params)
        }
        TaskAction::Get(args) => ("task.get", serialize_args(args)),
        TaskAction::List(args) => ("task.list", serialize_args(args)),
        TaskAction::Find(args) => ("task.find", serialize_args(args)),
        TaskAction::Modify(args) => {
            let mut params = serialize_args(args);
            let markdown = resolve_markdown(args.markdown.clone(), args.markdown_file.clone())?;
            match markdown {
                Some(value) => {
                    params["markdown"] = Value::String(value);
                }
                None => {
                    params.as_object_mut().map(|map| map.remove("markdown"));
                }
            }
            params.as_object_mut().map(|map| map.remove("markdownFile"));
            ("task.modify", params)
        }
        TaskAction::Remove(args) => ("task.remove", serialize_args(args)),
        TaskAction::Tree(args) => ("task.tree", serialize_args(args)),
    };
    Ok(Invocation::new(name, params))
}

fn build_schedule_invocation(action: &ScheduleAction) -> CoreResult<Invocation> {
    let (name, params) = match action {
        ScheduleAction::Add(args) => {
            let spec = resolve_json_payload(&args.spec)?;
            ("schedule.add", serde_json::json!({ "spec": spec }))
        }
        ScheduleAction::Validate(args) => {
            let mut params = serde_json::json!({});
            if let Some(spec) = &args.spec {
                params["spec"] = resolve_json_payload(spec)?;
            }
            if let Some(id) = &args.id {
                params["id"] = serde_json::json!(id);
            }
            if let Some(patch) = &args.patch {
                params["patch"] = resolve_json_payload(patch)?;
            }
            ("schedule.validate", params)
        }
        ScheduleAction::Get(args) => ("schedule.get", serialize_args(args)),
        ScheduleAction::List(args) => ("schedule.list", serialize_args(args)),
        ScheduleAction::Find(args) => ("schedule.find", serialize_args(args)),
        ScheduleAction::Modify(args) => {
            let patch = resolve_json_payload(&args.patch)?;
            (
                "schedule.modify",
                serde_json::json!({ "id": args.id, "patch": patch }),
            )
        }
        ScheduleAction::Remove(args) => ("schedule.remove", serialize_args(args)),
        ScheduleAction::Enable(args) => ("schedule.enable", serialize_args(args)),
        ScheduleAction::Disable(args) => ("schedule.disable", serialize_args(args)),
        ScheduleAction::Run(args) => ("schedule.run", serialize_args(args)),
        ScheduleAction::Stop(args) => ("schedule.stop", serialize_args(args)),
        ScheduleAction::Logs(args) => ("schedule.logs", serialize_args(args)),
        ScheduleAction::Status => ("schedule.status", Value::Null),
        ScheduleAction::Runtime { action } => match action {
            RuntimeAction::List => ("schedule.runtime.list", Value::Null),
            RuntimeAction::Detect => ("schedule.runtime.detect", Value::Null),
            RuntimeAction::Set(args) => ("schedule.runtime.set", serialize_args(args)),
        },
    };
    Ok(Invocation::new(name, params))
}

fn build_config_invocation(action: &ConfigAction) -> CoreResult<Invocation> {
    let (name, params) = match action {
        ConfigAction::List(args) => ("config.list", serialize_args(args)),
        ConfigAction::Get(args) => ("config.get", serialize_args(args)),
        ConfigAction::Set(args) => {
            let provided = [&args.value, &args.json_value, &args.value_file]
                .iter()
                .filter(|item| item.is_some())
                .count();
            if provided == 0 {
                return Err(CoreError::validation(
                    "MISSING_VALUE",
                    "config set 需要提供位置值、--json-value 或 --value-file 之一",
                ));
            }
            let value: Value = if let Some(raw) = &args.value {
                crate::domain::ops_config::parse_cli_value(raw)?
            } else if let Some(raw) = &args.json_value {
                serde_json::from_str(raw).map_err(|error| {
                    CoreError::validation("INVALID_JSON", format!("--json-value 不是有效 JSON：{error}"))
                })?
            } else if let Some(path) = &args.value_file {
                let raw = std::fs::read_to_string(path).map_err(|error| {
                    CoreError::validation("PAYLOAD_FILE_ERROR", format!("无法读取 {path}：{error}"))
                })?;
                serde_json::from_str(&raw).map_err(|error| {
                    CoreError::validation("INVALID_JSON", format!("--value-file 不是有效 JSON：{error}"))
                })?
            } else {
                unreachable!()
            };
            (
                "config.set",
                serde_json::json!({
                    "path": args.path,
                    "value": value,
                    "mapKey": args.map_key,
                }),
            )
        }
        ConfigAction::Unset(args) => ("config.unset", serialize_args(args)),
        ConfigAction::Reset(args) => ("config.reset", serialize_args(args)),
        ConfigAction::Path => ("config.path", Value::Null),
        ConfigAction::Validate => ("config.validate", Value::Null),
    };
    Ok(Invocation::new(name, params))
}

// ---------------------------------------------------------------------------
// Payload resolution
// ---------------------------------------------------------------------------

/// `'<text>' | @file | -` resolution for text payloads.
pub fn resolve_text_payload(raw: &str) -> CoreResult<String> {
    if let Some(path) = raw.strip_prefix('@') {
        return std::fs::read_to_string(path).map_err(|error| {
            CoreError::validation("PAYLOAD_FILE_ERROR", format!("无法读取 {path}：{error}"))
        });
    }
    if raw == "-" {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|error| CoreError::validation("STDIN_ERROR", format!("无法读取标准输入：{error}")))?;
        return Ok(buffer);
    }
    Ok(raw.to_string())
}

pub fn resolve_json_payload(raw: &str) -> CoreResult<Value> {
    let text = resolve_text_payload(raw)?;
    serde_json::from_str(&text).map_err(|error| {
        CoreError::validation("INVALID_JSON", format!("JSON 输入无效：{error}"))
    })
}

fn resolve_markdown(markdown: Option<String>, markdown_file: Option<String>) -> CoreResult<Option<String>> {
    if let Some(markdown) = markdown {
        return Ok(Some(markdown));
    }
    if let Some(path) = markdown_file {
        if path == "-" {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|error| {
                    CoreError::validation("STDIN_ERROR", format!("无法读取标准输入：{error}"))
                })?;
            return Ok(Some(buffer));
        }
        return Ok(Some(std::fs::read_to_string(&path).map_err(|error| {
            CoreError::validation("PAYLOAD_FILE_ERROR", format!("无法读取 {path}：{error}"))
        })?));
    }
    Ok(None)
}
