# KXTodo v9.0.0 CLI 功能开发需求

> 文档定位：定义 KXTodo v9.0.0 的 CLI 功能、交互契约与宏观实现架构，供后续开发落地。本文不规定具体代码组织和函数实现。

## 一、总体目标

KXTodo v9.0.0 在不改变现有 GUI 外观和使用方式的前提下，引入完整、可脚本化、对 AI Agent 友好的 CLI。

产品继续只发布一个可执行文件，并按启动参数进入不同模式：

- `kxtodo` 不带参数：启动现有 GUI。
- `kxtodo <命令> ...`：执行 CLI 命令，不打开主窗口，完成后以明确退出码正常结束。
- GUI 与 CLI 共用 `todo-note-data` 数据、业务规则和调度能力；任一端的修改都不能覆盖另一端的新数据。

CLI 应覆盖四项核心能力：

1. 发送桌面通知。
2. 完整管理分类、左栏条目及条目内任务。
3. 完整管理和运行定时任务。
4. 查询和修改 KXTodo 设置。

CLI 同时应提供稳定的 JSON 输入输出、逐级帮助、风险提示、预执行、结构定义及配套 SKILL，使人类、脚本和 Agent 都能可靠发现、查询和操作 KXTodo 数据。整体体验以 `lark-cli` 的领域分层、默认结构化输出和 Agent 工具化能力为标杆。

## 二、现状总结

当前版本为 v8.3.2，已经具备实现 v9 CLI 的数据和功能基础：

- 单一可执行文件已经能够根据参数区分 GUI 与 CLI；现有 CLI 支持 `--help`、`-h` 和 `notify`。
- `--help` 和参数错误会直接退出，但 `notify` 仍会启动 Tauri 事件循环，依赖通知窗口关闭事件结束进程，存在命令执行完毕后不能稳定退出的问题。
- 桌面端数据默认位于可执行文件同级的 `todo-note-data/`：
  - `data.json`：节点、具体任务和背景设置，当前 `schemaVersion` 为 4。
  - `settings.json`：用户、外观、生命周期、通知和快捷键等设置。
  - `tasks.json`：定时任务及脚本运行时路径。
- 前端加载时会将 `data.json` 和 `tasks.json` 组合为统一的内存 `AppState`，但持久化时仍分别写回对应文件。
- 左栏节点分为三类：
  - `system`：我的一天、计划内、收藏、定时任务等内置视图，不允许作为普通数据增删。
  - `category`：可嵌套的分类文件夹。
  - `entry`：左栏中的具体条目，可归属某个 `category`。
- 条目内的具体任务包含 Markdown 正文、完成状态、重要标记、我的一天、计划日期、截止日期、完成时间、标签、emoji、创建时间和修改时间。具体任务只能归属 `entry`，不能直接归属 `category`。
- 定时任务已有一次执行、固定间隔、Cron 日历和条件探测四种触发方式，以及脚本、可执行文件和通知三种动作；但调度循环目前运行在 Svelte 前端，只有应用前端存活时才能工作。
- GUI 当前将内存中的完整状态防抖后写回 JSON。如果 CLI 同时直接修改文件，GUI 的旧快照可能在之后覆盖 CLI 修改，因此 v9 必须统一读写入口并引入跨进程并发控制。
- 现有数据规范化、默认值和部分业务规则主要位于 TypeScript 前端。若 CLI 在 Rust 中另写一套规则，将产生双重实现和长期漂移风险。

真实使用数据进一步验证并补充了以下边界：

- `plannedDate`、`dueDate` 等可选字段在未设置时通常直接缺省，而不是写成 `null`；加载、输出和 patch 必须区分“缺省”与“清空”。
- 同类节点的可选字段并不完全一致，例如 `collapsed` 可能只出现在部分 category/entry；Rust 模型和迁移不得因整对象重写而丢弃未修改字段。
- `backgrounds` 真实以各类 node ID 为动态键，`settings.appearance.uiColors` 也真实使用 entry ID 作为 map key，证明动态 key 不能按点路径拆解。
- `img/data/<entry-id>/` 只为部分 entry 存在，目录内可能保留当前 Markdown 未引用的文件；迁移和 `doctor` 只能报告孤立资源，不得自动删除。
- 真实 `tasks.json` 同时使用 inline/path 脚本，但旧模型会为每种 trigger/action 持久化整套无关字段；path 模式甚至可能残留无效 code，interval 也带有无关的 cron/runAt/probeAction。这些字段不能直接成为 v9 CLI 输入，更不能在迁移后被误执行。

v9 的重点不是再包装一层参数解析，而是建立 GUI、CLI 和调度器共用的可靠业务核心，并让命令、JSON Schema、持久化模型和 GUI 校验共享同一事实来源。

## 三、命令设计

### 3.1 总体命令结构

```text
kxtodo [全局选项] <领域> <动作> [参数]
```

顶层命令如下：

```text
Core domains:
  notify       发送桌面通知
  task         管理分类、左栏条目和具体任务
  schedule     管理和运行定时任务
  config       查询和修改 KXTodo 设置

Agent tooling:
  schema       查看命令的机器可读输入结构
  skills       列出或读取随当前版本发布的 Agent SKILL
  doctor       检查数据目录、数据完整性、运行主机和脚本环境

Additional commands:
  help         显示任意层级的帮助
  version      显示版本和数据 schema 版本
```

帮助必须支持逐级查看。顶层和领域帮助保持简洁，只列出命令与一句话用途；动作级帮助完整列出 Usage、参数类型与默认值、互斥关系、输入示例、输出格式和风险等级：

```bash
kxtodo --help
kxtodo task --help
kxtodo task add --help
kxtodo help task add
```

无参数启动 GUI 的既有行为必须保留。除无参数 GUI 模式外，任何 CLI 命令均不得意外打开或切换主窗口。

### 3.2 全局约定

#### 全局选项

| 选项 | 功能 |
|---|---|
| `--data-dir <path>` | 本次命令使用指定数据目录；默认仍为可执行文件同级 `todo-note-data/` |
| `--format <json\|pretty\|table\|ndjson>` | 指定输出格式；默认 `json` |
| `--json` | `--format json` 的简写 |
| `-q, --jq <expr>` | 对成功 JSON 结果应用 jq 表达式；错误信封不被过滤 |
| `--dry-run` | 校验并展示将执行的操作、目标和影响范围，但不写数据、不执行动作 |
| `--yes` | 对要求确认的高风险操作表示已获得用户明确确认 |
| `--idempotency-key <key>` | 为新增等写操作提供幂等键，Agent 重试时避免重复创建 |
| `--if-revision <revision>` | 仅在目标数据域的版本仍匹配时写入，用于需要严格并发控制的调用 |
| `--no-color` | 禁止终端颜色 |
| `-h, --help` | 显示当前层级帮助 |
| `-v, --version` | 显示版本 |

`--data-dir` 只影响当前调用，不修改永久配置。读取或写入持久化数据的响应，其 `meta` 必须标明目标域及该域数据版本；通过非默认目录执行时还应标明所用数据目录。

#### 输入规则

- 简单资源优先提供独立参数；通用复杂对象支持 `--data '<json>'`、`--data @file.json` 和 `--data -`。三种形式分别表示内联 JSON、读取 UTF-8 JSON 文件、从标准输入读取 JSON。
- schedule 是例外：完整定义只能通过类型明确的 `--spec`/`--patch` JSON 输入，不再并列维护大量 trigger/action flags，也不接受同义的 `--data`。
- 同一字段不得同时从独立参数和 JSON payload 提供；冲突时返回参数错误，不猜测优先级。
- 所有 patch 使用统一语义：字段缺省表示不变；显式 `null` 表示清空可选字段并在持久化 JSON 中移除该键；`false`、`0` 和空数组均是有效新值；数组一旦出现即整体替换。必填字段不可设为 `null`，未知字段必须报错。`--clear-*` 与对应 JSON `null` 完全等价。
- 已知可选字段未设置时，持久化和默认 JSON 输出均省略该键，不写 `null`；读取旧数据时同时接受缺省和 `null`，规范化后按缺省落盘。
- Markdown 正文支持 `--markdown <text>` / `--markdown-file <path|->`；schedule 脚本不另设正文 flags，只能在 ScheduleSpec 的 discriminated `source` 中选择 inline 或 file。
- 日期使用 `YYYY-MM-DD`；时间使用带时区的 ISO 8601。允许帮助中明确列出的相对时间写法，例如 `+2d`、`+1h`，响应中始终返回标准化后的 ISO 值。
- 资源名称用于展示和搜索，资源 ID 用于精确读取、修改、移动和删除。名称重复时不得静默选择其中一个。
- 写操作应在响应中返回写入后的完整资源，而不只返回“成功”。

#### 输出协议

默认输出为 UTF-8 JSON。成功结果写入 `stdout`，退出码为 0：

```json
{
  "ok": true,
  "command": "task.list",
  "data": {
    "items": []
  },
  "meta": {
    "count": 0,
    "nextCursor": null,
    "revisionDomain": "data",
    "revision": 42,
    "requestId": "req-...",
    "replayed": false
  }
}
```

错误结果写入 `stderr`，退出码非 0：

```json
{
  "ok": false,
  "command": "task.modify",
  "error": {
    "type": "not_found",
    "code": "TASK_NOT_FOUND",
    "message": "未找到任务 task-c3d4e5f6",
    "hint": "先运行 kxtodo task find --type item --query ..."
  },
  "meta": {
    "requestId": "req-..."
  }
}
```

约定如下：

- 判断成功以退出码 0 或 `ok == true` 为准。
- `pretty` 面向人类阅读；`table` 主要用于 list/find，允许截断长 Markdown；Agent 获取完整数据应使用默认 JSON。
- `ndjson` 用于流式输出大量列表项。
- `--jq` 由 CLI 内置、版本固定的 jq 兼容子集执行，不依赖系统安装 `jq`；支持范围由 `kxtodo schema jq` 明确列出。
- 所有时间字段返回 ISO 8601；同时可在 `pretty`/`table` 中按本地时区展示。
- 空列表是成功结果，返回 `items: []`，不是“未找到”错误。
- 列表支持 `--limit`、`--cursor` 和 `--all`。默认限制应足以交互使用，同时避免意外输出全部大数据。

稳定退出码：

| 退出码 | 含义 |
|---|---|
| `0` | 成功 |
| `1` | 未分类内部错误 |
| `2` | 参数或数据校验错误 |
| `3` | 指定资源不存在 |
| `4` | 名称歧义、版本冲突或资源状态冲突 |
| `5` | 数据文件损坏、锁或文件系统错误 |
| `10` | 高风险操作需要确认 |
| `20` | 脚本、可执行文件或调度动作执行失败 |

#### 风险等级

每个动作的帮助顶部显示 `Risk: read | write | high-risk-write`：

- `read`：只读查询。
- `write`：普通新增和修改，可直接执行，也支持 `--dry-run`。
- `high-risk-write`：级联删除、重置配置、运行脚本、启用含代码的定时任务等。未带 `--yes` 时不得执行，返回退出码 10 和结构化确认信息。
- `--dry-run` 永远不触发确认门禁，便于 Agent 先向用户展示影响范围。

### 3.3 通知：`kxtodo notify`

```text
kxtodo notify <message> [options]
```

| 参数 | 功能 |
|---|---|
| `<message>` / `--message <text>` | 通知正文，二选一且必填 |
| `--title <text>` | 通知标题，默认 `KXToDo` |
| `--duration <duration>` | 显示时长，如 `5s`、`5200ms` |
| `--tone <info\|success\|warning\|error>` | 通知样式 |
| `--position <bottom-right\|top-right\|bottom-left\|top-left>` | 弹窗位置 |
| `--wait` | 等待通知窗口关闭后再结束；默认只等待通知被可靠接收 |

示例：

```bash
kxtodo notify "构建完成" --title "CI" --tone success --duration 5s
```

通知一律路由到与目标数据目录匹配的 Background Host；Host 不存在时先按需启动，由 Host 持有通知窗口和计时器生命周期。默认情况下，CLI 在 Host 确认通知已创建后立即返回；响应包含通知 ID 和接收状态。只有 `--wait` 才等待窗口关闭，CLI 自身不得通过进入 GUI 事件循环来维持弹窗。

Notification payload 与 `settings.notifications` 不是同一对象：前者只描述单次通知的 title/message/duration/tone/position，后者提供全局默认 duration/position 以及窗口 width/height/titleFontSize/bodyFontSize。单次 payload 未给 duration/position 时在实际派发时读取当前 settings；窗口尺寸和字号始终来自 settings。title 缺省为 `KXToDo`、tone 缺省为 info。schedule 中的 Notification 也遵循同一动态继承规则；显式字段覆盖全局默认。

### 3.4 任务管理：`kxtodo task`

#### 3.4.1 对象命名与 ID

`task` 是管理待办数据的领域名；其中有三种可编辑对象，另有一种只读系统节点：

| `--type` | 中文含义 | 归属规则 |
|---|---|---|
| `category` | 分类文件夹，可嵌套 | 只能位于根级或另一个 category 下 |
| `entry` | GUI 左栏中的具体条目 | 只能位于根级或 category 下 |
| `item` | 条目内的具体 Todo/Markdown 任务 | 必须归属一个 entry |
| `system` | 我的一天、计划内、收藏等内置视图 | 只允许 get/list/find，不允许增删改 |

所有 ID 均由系统生成并视为不透明字符串。示例中的 `category-a1b2c3d4`、`entry-b2c3d4e5` 等前缀只为便于阅读，不构成协议；旧数据或导入数据可能使用完全不同的 ID。Agent 不得拼造 ID，也不得根据前缀推断类型，必须先通过 get/list/find/tree 获取 ID。正因 ID 前缀不可靠，`get/modify/remove` 的 `--type` 与 `--id` 均保持必填，并由 CLI 校验真实对象类型。具体 item 在历史数据中通常使用 `task-` 前缀，因此错误码继续沿用 `TASK_*`；这只是兼容命名，不代表 item 可由前缀识别。

命令输出必须提供类型、稳定 ID、名称和可读层级路径。具体 item 还应提供其 entry、祖先 category 和完整路径等位置上下文，使 Agent 无需自行拼接多次查询结果。

例如“2026”可以是 category，“0727-0731”可以是其下的 entry，具体“完成 XXX 需求”则是 item。

#### 3.4.2 动作总览

```text
kxtodo task <action> [options]

Actions:
  add       新增 category、entry 或 item
  get       按稳定 ID 获取一个完整对象
  list      按层级和结构化条件列出对象
  find      按关键词搜索，并可叠加结构化条件
  modify    修改或移动对象
  remove    删除对象
  tree      查看 category/entry 层级树及任务计数
```

兼容英文常用词别名：`create -> add`、`update -> modify`、`delete -> remove`、`search -> find`。帮助和 SKILL 统一使用上表中的主命令，避免文档出现两套写法。

#### 3.4.3 新增：`task add`

新增分类：

```bash
kxtodo task add --type category --name "2026"
kxtodo task add --type category --name "下半年" --parent-id category-a1b2c3d4
```

主要参数：`--name`、`--parent-id <category-id|root>`、`--icon`、`--collapsed`。未指定 `--parent-id` 时等同于 `root`。`root` 是 CLI 保留字，系统不得生成值为 `root` 的资源 ID。

新增左栏条目：

```bash
kxtodo task add --type entry --name "0727-0731" --parent-id category-a1b2c3d4 --icon calendar
```

主要参数：`--name`、`--parent-id <category-id|root>`、`--icon`。未指定 `--parent-id` 时等同于 `root`。

新增具体任务：

```bash
kxtodo task add --type item \
  --entry-id entry-b2c3d4e5 \
  --markdown "完成 XXX 需求" \
  --important true \
  --due-date 2026-07-31 \
  --tag "blue:需求"
```

主要参数：

- 必填：`--entry-id`，以及 `--markdown` / `--markdown-file` 二选一。`--entry-id` 持久化为 item.nodeId，目标必须真实存在且 kind=entry。
- 状态：`--completed <true|false>`、`--important <true|false>`、`--my-day <true|false>`。
- 日期：`--planned-date`、`--due-date`。
- 标签：可重复的 `--tag "<color>:<text>"`；颜色支持现有 red/yellow/blue/green/gray。创建结果会为每个标签返回系统生成的 tag ID。
- Emoji：可重复的 `--emoji`。

新增命令支持 `--idempotency-key`。相同数据目录内使用同一幂等键重试时，返回第一次创建的资源，并在 `meta.replayed` 标记重放，不能重复创建。

#### 3.4.4 精确读取：`task get`

```bash
kxtodo task get --type item --id task-c3d4e5f6
kxtodo task get --type entry --id entry-b2c3d4e5
```

`--type <system|category|entry|item>` 与 `--id` 必填。结果返回完整资源；item 包含 Markdown 全文、全部状态和日期、标签、emoji、创建/修改/完成时间，以及归属 entry 和祖先 category 信息。category/entry 返回自身信息、路径和直接/递归计数摘要。

#### 3.4.5 列表：`task list`

`list` 用于无关键词的结构化查询：

```bash
# 列出根级分类
kxtodo task list --type category --parent-id root

# 列出 2026 分类下的条目
kxtodo task list --type entry --parent-id category-a1b2c3d4

# 列出某条目中的全部具体任务
kxtodo task list --type item --entry-id entry-b2c3d4e5 --status all

# 查询最近一周创建或修改过的具体任务
kxtodo task list --type item \
  --changed-from 2026-07-20T00:00:00+08:00 \
  --changed-to 2026-07-27T23:59:59+08:00 \
  --all
```

category/entry 层级过滤统一使用：

- `--parent-id <category-id|root>`：指定父级的直接子级；`root` 表示根级。
- 叠加 `--recursive`：返回该父级下符合 `--type` 的全部后代。category/entry 列表不再提供语义重叠的 `--parent` 或 `--category-id`。

item 过滤：

- 位置：`--entry-id`、`--category-id [--recursive]`；item 列表中的 `--recursive` 只能与 `--category-id` 搭配，表示包含该分类全部后代 entry 中的 item。
- 状态：`--status <open|completed|all>`，默认 `all`；`--important <true|false>`、`--my-day <true|false>`。
- 标签：`--tag <text-or-id>`，可重复。按 ID 时精确匹配；按文本时匹配所有同文本标签，不因重复文本报歧义。
- 时间范围：`--created-from/to`、`--updated-from/to`、`--changed-from/to`、`--planned-from/to`、`--due-from/to`、`--completed-from/to`。
- 排序：`--sort <createdAt|updatedAt|dueDate|completedAt|name|position>` 与 `--order <asc|desc>`。
- 分页：`--limit`、`--cursor`、`--all`。

`changed` 表示创建时间或修改时间落入范围，适合“本周做过什么”这类 Agent 查询。默认 JSON 中每个 item 返回完整信息；`table` 才可将 Markdown 缩略显示。

#### 3.4.6 搜索：`task find`

`find` 用于包含真实关键词的全文搜索：

```bash
kxtodo task find --type all --query "0727"
kxtodo task find --type item --query "XXX 需求" --status completed
```

- `--query` 必填。
- `--type <system|category|entry|item|all>`，默认 `all`。
- 搜索范围包括 system/category/entry 名称、层级路径、item Markdown 和标签文本。
- 支持叠加 `list` 中适用于相同类型的状态、位置、时间、排序和分页条件。
- 结果必须返回匹配类型和稳定 ID；item 仍返回完整信息和位置上下文。

如果用户只给出时间、状态或位置范围而没有关键词，SKILL 应优先调用 `list`，不应把“本周”“已完成”等范围词机械地当成全文关键词。

#### 3.4.7 修改：`task modify`

修改必须使用稳定 ID：

```bash
kxtodo task modify --type entry --id entry-b2c3d4e5 --name "0727-0731 周报"
kxtodo task modify --type item --id task-c3d4e5f6 --completed true
kxtodo task modify --type item --id task-c3d4e5f6 --entry-id entry-d4e5f6a7
```

category/entry 支持：

- `--name`、`--icon`。
- `--parent-id <category-id|root>` 移动到另一个分类或根级。
- category 额外支持 `--collapsed <true|false>`。
- 禁止形成分类循环，禁止将节点放入 entry 或 system 节点下。

item 支持：

- `--entry-id` 移动归属。
- 替换 Markdown：`--markdown` / `--markdown-file`。
- 修改布尔状态：`--completed`、`--important`、`--my-day`。
- 设置日期：`--planned-date`、`--due-date`；对应 `--clear-planned-date`、`--clear-due-date`。
- 标签：`--add-tag "<color>:<text>"`；`--remove-tag <tag-id>` 只按精确 ID 删除；`--replace-tags` 接受完整的 `color:text` 列表并整体替换。
- Emoji：`--add-emoji`、`--remove-emoji`、`--replace-emojis`。

完成状态必须维护一致语义：从未完成改为完成时记录 `completedAt`，重新打开时清除；所有有效修改更新 `updatedAt`。节点也应在 v9 数据模型中具有 `updatedAt`，用于审计和增量查询。

#### 3.4.8 删除：`task remove`

```bash
kxtodo task remove --type item --id task-c3d4e5f6 --yes
kxtodo task remove --type entry --id entry-b2c3d4e5 --cascade --yes
```

- `--type` 和 `--id` 必填。
- 删除非空 entry 或 category 必须显式提供 `--cascade`；否则返回资源状态冲突并说明其子级和任务数量。
- 级联删除必须一并处理后代节点、具体任务、节点背景和关联 Markdown 图片。
- 所有删除均为 `high-risk-write`。未带 `--yes` 时返回退出码 10。
- `--dry-run` 返回将删除的节点 ID、任务 ID、图片数量和总计，不做实际删除。

#### 3.4.9 树视图：`task tree`

```bash
kxtodo task tree
kxtodo task tree --root-id category-a1b2c3d4 --depth 3 --include-counts
```

返回 category 与 entry 的层级树、稳定 ID、可读路径和直接/递归任务计数。默认不把 item 全部嵌入树中；需要任务详情时继续调用 `task list --type item --entry-id ...`。该命令用于人类和 Agent 快速理解数据组织结构。

#### 3.4.10 “本周做了什么”的标准调用流程

CLI 不负责替 Agent 总结自然语言结论，但必须提供完整、可组合的数据：

1. `task tree` 或 `task find --type entry --query "0727-0731"` 获取可能相关的分类、条目和稳定 ID。
2. `task list --type item --changed-from ... --changed-to ...` 获取本周创建或修改的任务。
3. `task list --type item --entry-id ... --all` 获取目标条目下的完整任务。
4. Agent 根据返回的 Markdown、状态、标签、时间和层级位置进行汇总。

创建本周任务的标准流程：

1. 查找或列出目标 category 和 entry。
2. 不存在时分别用 `task add --type category/entry` 创建，并记录返回 ID。
3. 用 `task add --type item --entry-id ... --markdown ...` 创建具体任务。
4. 每个新增步骤使用独立幂等键，避免 Agent 重试产生重复数据。

### 3.5 定时任务：`kxtodo schedule`

#### 3.5.1 动作与输入原则

```text
kxtodo schedule <action> [options]

Definition:
  add       使用完整 ScheduleSpec 新增定时任务
  validate  校验 spec 或 patch，不写入、不执行
  get       获取完整 spec 和运行状态
  list      按结构化条件列出定时任务
  find      按关键词搜索定时任务
  modify    使用 SchedulePatch 修改定时任务
  remove    删除定时任务

Runtime:
  enable    启用任务
  disable   禁用任务
  run       立即执行一次
  stop      终止正在运行的任务
  logs      查看运行历史、stdout 和 stderr
  status    查看 Host、运行任务和下次执行时间

Environment:
  runtime list                 查看脚本运行时
  runtime detect               重新探测可用运行时
  runtime set <name> <path>    设置 python/node/pwsh/bash/make 路径
```

`search` 仅作为 `find` 的隐藏兼容别名；帮助、schema 和 SKILL 统一只展示 `find`。`runtime` 是 schedule 下唯一的子命名空间，因为解释器路径属于调度运行环境。

schedule 定义字段较多且具有明显的类型互斥关系，v9 不再为 `add/modify` 提供 `--trigger`、`--every`、`--action`、`--script-file` 等完整字段 flags。唯一完整输入为：

```bash
kxtodo schedule add --spec @schedule.json
kxtodo schedule add --spec -
kxtodo schedule modify --id schedule-e5f6a7b8 --patch @patch.json
kxtodo schedule validate --spec @schedule.json
kxtodo schedule validate --id schedule-e5f6a7b8 --patch @patch.json
```

- `--spec '<json>' | @file | -`：完整 `ScheduleSpec`，用于 add/validate。
- `--patch '<json>' | @file | -`：`SchedulePatch`，用于 modify；validate patch 时同时提供 `--id`，以便结合当前 spec 校验 discriminator 和最终完整对象。
- `--id`、`--dry-run`、`--yes`、`--idempotency-key`、`--if-revision` 等资源定位和全局控制参数仍使用 flags。
- payload 与 `kxtodo schema schedule.spec/patch` 不符、包含未知字段或包含 id/runCount/lastStatus 等运行时字段时直接报错，不能静默忽略。
- `schedule add` 默认创建为 disabled；spec 中显式 `"enabled": true` 才启用。启用脚本或可执行程序仍属于 `high-risk-write`，需要 `--yes`。

这样，字段、必填关系、默认值和互斥关系只存在于 ScheduleSpec JSON Schema；CLI 参数解析器不再复制一套相同规则。

#### 3.5.2 ScheduleSpec

ScheduleSpec 顶层保持最小且稳定：

```json
{
  "name": "提交周报提醒",
  "enabled": false,
  "trigger": { "type": "once", "at": "2026-07-31T17:30:00+08:00" },
  "action": {
    "type": "notification",
    "notification": { "title": "KXToDo", "message": "记得提交周报" }
  }
}
```

- `name` 必填且非空。
- `enabled` 可选，默认 `false`。
- `trigger` 和 `action` 必填，均是以 `type` 为 discriminator 的联合类型。
- 未属于当前 discriminator 分支的字段一律非法。例如 interval 不接受 `cron`，file script 不接受 `code`。

Trigger 只允许以下四种最小结构：

```json
{ "type": "once", "at": "2026-07-31T17:30:00+08:00", "missedPolicy": "run-once" }
```

```json
{
  "type": "interval",
  "every": "1h",
  "maxRuns": 10,
  "stopWhen": { "stream": "stdout", "mode": "contains", "pattern": "DOWNLOAD_DONE" },
  "missedPolicy": "run-once"
}
```

```json
{
  "type": "calendar",
  "cron": "0 9 * * *",
  "timezone": "Asia/Shanghai",
  "missedPolicy": "skip"
}
```

```json
{
  "type": "condition",
  "every": "1m",
  "probe": {
    "type": "script",
    "language": "python",
    "source": { "type": "inline", "code": "print('READY')" },
    "args": [],
    "timeout": "30s"
  },
  "when": { "stream": "stdout", "mode": "contains", "pattern": "READY" },
  "missedPolicy": "run-once"
}
```

规则：

- duration 统一使用 `<正整数><ms|s|m|h|d>`，由 Domain Core 的唯一解析器处理；不在 CLI、GUI 和 scheduler 中各写一套解析规则。
- interval 的 `maxRuns` 缺省表示不限次数；提供时必须为正整数，表示主动作最多启动的总次数，每次启动即计入 runCount，不因退出码成败而改变计数。
- `stopWhen`、`when` 使用统一 Match 结构；`stream` 当前只支持 stdout/stderr，`mode` 支持 contains/regex。
- condition 的 `probe` 只允许不带 notifications 的 script 或 executable。probe 超时、启动失败或非零退出均视为本次探测失败：不执行主 action、不增加主 action 的 runCount、记录独立 probe 日志和 failed 状态，并在下一个 every 周期继续探测；主 action 的 onComplete/onOutput 不因 probe 失败触发。
- `missedPolicy` 为 skip/run-once；缺省值由 trigger schema 定义：once/interval/condition 为 run-once，calendar 为 skip。恢复时最多补一次，绝不批量回放。

Action 只允许以下三种结构：

```json
{
  "type": "script",
  "language": "python",
  "source": { "type": "file", "path": "./download_when_ready.py" },
  "args": [],
  "workingDirectory": "./downloads",
  "timeout": "10m"
}
```

script 的 `source` 必须二选一：

```json
{ "type": "file", "path": "./script.py" }
```

```json
{ "type": "inline", "code": "print('hello')" }
```

```json
{
  "type": "executable",
  "program": "./tool.exe",
  "args": ["--mode", "check"],
  "workingDirectory": "./work",
  "timeout": "5m"
}
```

```json
{
  "type": "notification",
  "notification": {
    "title": "KXToDo",
    "message": "记得提交周报",
    "duration": "5s",
    "tone": "info",
    "position": "bottom-right"
  }
}
```

script.language 只支持 python/javascript/powershell/bash/makefile；可选 `interpreter` 用于覆盖该内置语言的 runtime 路径，缺省时使用 `schedule runtime` 中对应配置。新 spec 不提供 `language:custom`：需要任意解释器或调用约定时使用 executable action，避免再维护一套“custom script 如何传源码”的隐式规则。

script/executable 的 `args` 始终是字符串数组，不接受待二次拆词的命令行字符串；进程直接按 argv 启动。相对的 source/program/interpreter/workingDirectory 路径在创建时以 CLI 当前工作目录解析并规范化后保存，不能依赖未来 scheduler 的启动目录。主 action 的 timeout 可选，缺省表示不设置自动超时；condition probe 应显式设置有限 timeout，validate 对缺省给出 warning。通知字段与 `kxtodo notify` 复用同一 Notification schema，并按 §3.3 继承当前通知外观设置。

script/executable 可选的 notifications 结构为：

```json
{
  "onComplete": { "title": "KXToDo", "message": "{taskName} 完成：{stdout}", "tone": "success" },
  "onOutput": {
    "when": { "stream": "stdout", "mode": "contains", "pattern": "READY" },
    "notification": { "title": "KXToDo", "message": "{stdout}", "tone": "info" }
  }
}
```

缺省 notifications 表示不发送附加通知。模板变量只允许 schema 列出的 `{taskName}`、`{stdout}`、`{stderr}`、`{exitCode}`；未知变量报错。

#### 3.5.3 典型创建流程

每小时尝试下载，成功后停止的完整 spec：

```json
{
  "name": "等待并下载 XXX",
  "enabled": true,
  "trigger": {
    "type": "interval",
    "every": "1h",
    "stopWhen": { "stream": "stdout", "mode": "contains", "pattern": "DOWNLOAD_DONE" }
  },
  "action": {
    "type": "script",
    "language": "python",
    "source": { "type": "file", "path": "./download_when_ready.py" },
    "args": [],
    "workingDirectory": "./downloads",
    "timeout": "10m"
  }
}
```

Agent 标准流程：

```bash
kxtodo schema schedule.spec --example interval-script --jq '.data.example' > schedule.json
# 编辑 schedule.json 中的业务值和脚本路径
kxtodo schedule validate --spec @schedule.json
kxtodo schedule add --spec @schedule.json --dry-run
kxtodo schedule add --spec @schedule.json --yes --idempotency-key weekly-download-v1
```

脚本自行访问 URL，在下载成功时输出 `DOWNLOAD_DONE`；调度器匹配 stopWhen 后禁用任务。`validate` 只做 schema、路径、runtime、Cron/时区和模板检查，不写入、不执行。`--dry-run` 进一步返回规范化后的 spec、目标路径、风险和预计副作用。

#### 3.5.4 修改语义

SchedulePatch 从 ScheduleSpec schema 自动派生，使用 §3.2 的统一 patch 规则：除 name/trigger/action 等被实际提供的路径外，其余字段均可缺省；nested object 递归支持 partial patch。discriminator 本身不要求每次出现，但一旦改变 trigger.type、action.type 或 source.type，对应对象立即按新分支 schema 校验并要求完整。派生规则由 schema 生成器实现，不维护手写 Patch 类型。

```json
{ "trigger": { "every": "30m" } }
```

```bash
kxtodo schedule modify --id schedule-e5f6a7b8 --patch '{"trigger":{"every":"30m"}}'
```

- 缺省字段不变，null 清除可选字段。
- 修改 trigger.type 或 action.type 时，该 trigger/action 必须提供新分支的完整对象，不能把旧分支字段带入新类型。
- 修改 script source.type 时必须提供完整 source；file→inline 不保留 path，inline→file 不保留 code。
- id、createdAt、updatedAt、runCount、lastStatus、lastRunAt、nextRunAt、stdout/stderr 等均为服务端管理字段，patch 不允许出现。
- 修改触发器后由 Core 重算 nextRunAt；修改定义不清空历史，除非未来提供单独且高风险的日志清理命令。

#### 3.5.5 查询、删除和运行控制

```bash
kxtodo schedule get --id schedule-e5f6a7b8
kxtodo schedule list --enabled true --status all --sort nextRunAt
kxtodo schedule find --query "下载" --all
kxtodo schedule remove --id schedule-e5f6a7b8 --yes
kxtodo schedule enable --id schedule-e5f6a7b8 --yes
kxtodo schedule disable --id schedule-e5f6a7b8
kxtodo schedule run --id schedule-e5f6a7b8 --wait --yes
kxtodo schedule stop --id schedule-e5f6a7b8
kxtodo schedule logs --id schedule-e5f6a7b8 --limit 20
kxtodo schedule status
```

`get` 返回 `{ id, spec, state, createdAt, updatedAt }`；spec 与 add 输入使用完全相同的结构，enabled 只存在于 spec。state 包含 runCount、是否正在执行、last/next run、lastStatus 和截断摘要。list/find 的默认 JSON 同样返回完整 spec 和 state；table 只显示摘要。

list 支持 enabled、status、trigger type、创建/修改/上次/下次运行时间、排序和分页过滤。find 搜索名称、脚本路径和通知文本，但向用户转述时避免无必要地粘贴 inline code、路径或历史输出。

remove 为 high-risk-write；正在运行时先停止并回收子进程。enable/disable 修改同一个 `spec.enabled` 字段、递增 schedule revision 并返回更新后的对象；enable 在启用前重新校验 spec/runtime/path，script/executable 需要 `--yes`。disable 阻止后续运行但不终止当前实例，立即终止使用 stop。run 默认入队，`--wait` 才返回最终退出码和输出；执行代码需要 `--yes`。

logs 返回有界运行历史，包括计划/实际时间、状态、退出码、stdout/stderr、停止原因、missedCount 和截断标记。status 返回 Host、运行任务、下一次唤醒、runtime 可用性及 lastMissedAt。后台运行与 Host 生命周期遵循 §4.4。

### 3.6 配置管理：`kxtodo config`

```text
kxtodo config <action>

Actions:
  list       列出配置，可按前缀过滤
  get        获取一个配置项
  set        设置一个配置项
  unset      清除可选配置，恢复该项默认继承行为
  reset      将一个配置分支或全部设置恢复默认值
  path       显示数据目录及各数据文件的实际路径
  validate   校验配置和引用的本地资源
```

配置的静态字段使用与 `settings.json` 一致的点路径，CLI 必须依据已知 schema 校验类型和范围并拒绝未知键。`set` 的位置值、`--json-value`、`--value-file` 三者必须且只能提供一个，不设优先级：

```bash
kxtodo config get appearance.uiScale
kxtodo config list --prefix appearance
kxtodo config set appearance.uiScale 0.85
kxtodo config set notifications.position top-right
```

`appearance.uiColors` 等动态 map 不把资源 ID 解析成点路径片段，统一通过 `--map-key` 精确指定；因此旧数据中的 ID 即使包含 `.` 也无歧义。配置 schema 必须将此类字段标记为 map，并声明 key/value 类型，CLI 不得靠硬编码字段名猜测：

```bash
kxtodo config get appearance.uiColors --map-key "entry-b2c3d4e5"
kxtodo config set appearance.uiColors "#dfe8df" --map-key "entry-b2c3d4e5"
kxtodo config unset appearance.uiColors --map-key "entry-b2c3d4e5"
```

数组和对象支持 JSON 或文件输入，适合修改配色盘：

```bash
kxtodo config set appearance.themePresets \
  --json-value '[{"name":"项目蓝","color":"#dbeafe"},{"name":"柔和绿","color":"#dcfce7"}]'

kxtodo config set appearance.themePresets --value-file ./palette.json
```

要求：

- `get/list` 返回规范化后的有效值，并在适用时标记该值来自用户配置还是默认值。
- `set` 先完成完整配置校验，再原子写入；涉及开机启动、全局快捷键等原生副作用时，运行主机同步应用并返回结果。
- `unset` 只适用于可选映射项或有明确默认继承语义的字段。
- `reset [prefix]` 为高风险操作，必须 `--yes`；`--dry-run` 返回将恢复的字段及前后值。
- `path` 是只读诊断命令，必须返回 §4.2.3 中全部持久化、备份、运行时和 SKILL 路径，并标记不存在、只读或正在使用的路径。
- `validate` 不自动修复；发现损坏或无效值时返回字段路径、原因和建议。未来若增加 secret，`list/get` 默认脱敏，只有专门的安全流程可以读取。

### 3.7 Agent 工具

#### `schema`

```bash
kxtodo schema task.add
kxtodo schema task.list --format json
kxtodo schema schedule.spec
kxtodo schema schedule.patch
kxtodo schema schedule.spec --example interval-script
kxtodo schema config.set
```

`schema` 返回由 Rust 权威模型生成的 JSON Schema、命令参数、必填关系、discriminator、枚举、默认值、互斥条件、风险等级和经过校验的示例。它是 Agent 生成 payload 前的机器权威依据；`schedule.spec` 与 `schedule.patch` 必须直接来自实际验证/持久化所用模型，不能维护手写副本。

#### `skills` 与自包含 SKILL 交付

```bash
kxtodo skills list
kxtodo skills read kxtodo
kxtodo skills path
kxtodo skills validate
kxtodo skills persist kxtodo <path>
kxtodo skills echo kxtodo
```

v9 只维护一份官方 SKILL 源文件：

```text
skills/kxtodo/SKILL.md                         # 仓库中的唯一编辑源
```

构建时通过 `include_str!` 将该源文件编译期嵌入二进制；发布的 exe 完全自包含，不依赖外部 SKILL 文件。`skills list/read/path/validate/echo` 均读取同一份嵌入正文。`skills echo kxtodo` 将完整 Markdown 原文直接写到 stdout，不包 JSON 信封；未知名称仍返回标准错误信封。

`skills persist <skill-name> <path>` 用于按需落地可编辑副本：若 path 末段已是 `skills`，则生成 `<path>/<skill-name>/SKILL.md`；否则生成 `<path>/skills/<skill-name>/SKILL.md`。命令递归建目录并原子覆盖，成功 envelope 返回最终绝对路径。外部副本只是导出物，不是运行时读取源。

单文件 SKILL 必须以简洁章节覆盖：共享输出/退出码/风险规则；category/entry/item 术语；list 与 find 的选择；按 ID 修改；定时任务 JSON 工作流；配置与配色盘；典型查询和创建流程。SKILL 只说明“何时调用、按什么步骤调用、如何处理风险/错误”，不得复制 ScheduleSpec 字段表、枚举、默认值或完整 help；这些内容一律引导 Agent 读取当前版本的 `--help`/`schema`。它必须强调：写操作使用幂等键；删除和代码执行先 dry-run 再确认；不猜测 ID 或类型；不把范围条件误当全文关键词。

`skills validate` 校验 frontmatter、SKILL 版本、嵌入正文中出现的命令和参数是否仍被当前 CLI/schema 支持。帮助与 schema 仍由同一命令元数据产生；SKILL 源文件独立维护、编译期嵌入，并必须在 CI 中通过该校验。

#### `doctor`

```bash
kxtodo doctor
kxtodo doctor --check runtimes
```

检查项目包括：数据目录可读写、JSON schema 与迁移状态、备份可用性、文件锁、后台运行主机、调度器状态、脚本运行时及配置引用路径。默认只诊断，不修改数据；任何修复必须另行显式确认。

## 四、设计架构

### 4.1 总体分层

v9 采用“单一可执行文件、多入口、共享业务核心”的架构：

```text
                  ┌─ GUI / Tauri commands ─┐
Executable Router ├─ CLI parser & renderer ─┼─ Application / Domain Core
                  └─ Background host ───────┘             │
                                      ┌───────────────────┼───────────────────┐
                                      │                   │                   │
                              Data Repository      Scheduler Engine    Notification Adapter
                                      │                   │                   │
                               todo-note-data       Process Runtime       Desktop Window
```

- **Executable Router**：只负责根据参数选择 GUI、短生命周期 CLI 或内部后台主机模式。普通 CLI 命令不进入 Tauri GUI 事件循环。
- **CLI 层**：使用 Rust 的成熟参数解析体系实现领域、动作、逐级帮助和参数校验；负责输入适配与输出渲染，不直接操作 JSON。
- **Application / Domain Core**：统一实现 category/entry/item、配置、调度定义、风险和业务校验。GUI、CLI 和后台主机调用完全相同的用例，避免规则分叉。
- **Repository 层**：统一负责数据加载、迁移、锁、版本、原子写入、备份和查询。
- **Background Host**：长生命周期地承载调度器、通知窗口和运行中子进程管理；主窗口是否显示不影响它工作。
- **Adapter 层**：隔离文件系统、进程执行、桌面通知、开机启动等平台差异，保留 Windows 主目标和后续 Linux/macOS 扩展能力。

CLI 主体使用 Rust 是合适的：可直接复用现有 Tauri 后端和本地文件能力，启动快、易发布为同一可执行文件，也便于提供可靠退出码、文件锁和进程管理。性能不是首要理由，单一业务核心和生命周期可控才是主要收益。

### 4.2 权威模型、v9 边界与迁移

Rust Domain Core 是 v9 的业务权威：category、entry、item、settings 和 schedule 的默认值、关系校验、时间语义、持久化修改与迁移只能由它决定。GUI 视觉、布局和既有交互保持不变，但所有持久化写入必须改为提交业务命令，不能再直接保存前端全量快照。

v9 必须完成：Rust Repository（锁、原子写、per-domain revision、迁移、备份）、四个 CLI 领域、Rust Background Host 与调度器、GUI 写入命令化、事件刷新、schema/doctor，以及独立单文件 SKILL。TypeScript 在 v9 可保留类型声明、只读展示转换和首屏兜底，但不得继续拥有独立的持久化写入或相互矛盾的业务校验；彻底删除已无用途的 TS 规范化兼容代码可在 v9 后继续收敛，不能阻塞本版交付。

其它统一规则：

- system 节点、完成时间、移动约束、标签颜色和调度触发器均以 Rust Core 为准。
- 输出层可添加 path、祖先和计数等派生字段，但不得把展示字段写回存储模型。
- Repository 对 data/settings 的未识别字段采用保留式读写：修改一个字段不能丢弃同对象、其它对象或顶层的未知字段。迁移只有在本版本明确声明转换时才可删除旧字段。
- 已知可选值统一“缺省即未设置”；patch null 删除键。`collapsed` 等非均匀可选字段和动态 backgrounds/uiColors key 必须原样保留。
- item.nodeId 必须指向真实存在的 entry，不能指向 category/system。新增、移动、迁移和每次写事务都校验该约束；孤儿 item 保留原始 JSON，由 doctor 和查询结果的 integrityIssues 报告，不得像现有前端 normalizeState 那样静默过滤。允许通过 `task modify --type item --id ... --entry-id <valid-entry>` 修复。
- `expanded`、`editing` 等纯 GUI 临时字段不属于 CLI 可编辑业务数据；迁移时保留以兼容 GUI，CLI 读写不得主动改变，后续版本可迁出持久化模型。
- 图片目录不存在是正常状态；未被当前 Markdown 引用的文件不等于可安全删除。只有删除整个 entry 的显式级联事务可删除其图片目录，其它孤立项仅由 doctor 报告。
- 所有可变资源保留稳定、不透明 ID；普通节点在 v9 增加 `updatedAt`。

#### 4.2.1 单一事实来源

| 事实 | 唯一权威来源 | 自动派生或消费方 |
|---|---|---|
| 命令名、层级、flags、风险等级 | Rust Command Catalog | 参数解析器、`--help`、命令 schema、补全、SKILL validator |
| 业务字段、类型、discriminator、枚举、默认值、约束 | Rust Domain Model | JSON Schema、payload 校验、Repository、GUI 生成类型、`kxtodo schema` |
| ScheduleSpec/SchedulePatch/Notification/Match | 同一 Rust 类型及其生成 schema | CLI add/modify、tasks.json、GUI 编辑器、scheduler executor |
| 数据目录及增长上限 | Repository Layout Descriptor | `config path`、doctor、备份/清理器、帮助 |
| schema 迁移顺序 | Migration Registry | Host/standalone 启动、doctor |
| Agent 工作流与选命令原则 | `skills/kxtodo/SKILL.md` 唯一源文件（编译期嵌入） | Agent、`skills read/echo/persist/validate`；只引用 help/schema，不复制字段表和默认值 |

生成的 JSON Schema、TypeScript 类型、help 和示例属于构建产物，不允许手工编辑。CI 必须：重新生成后检查工作区无差异；用真实验证器运行所有 help/SKILL/需求示例；检查 SKILL 中出现的命令和 flags 均存在。字段或默认值变更只改 Rust Domain Model；命令名称或风险变更只改 Command Catalog；数据路径变更只改 Layout Descriptor。

#### 4.2.2 v8 → v9 数据迁移

首次打开旧数据时，在获取仓库独占锁并完成备份后执行一次幂等迁移：

| 文件 | v9 格式 | 必须迁移的内容 |
|---|---|---|
| `data.json` | `schemaVersion: 5` | 为普通节点补 `updatedAt`（旧数据以 `createdAt` 回填）；增加 `_meta.revision = 0` 和空幂等台账；保留 GUI 临时字段但 CLI 不修改 |
| `settings.json` | `_meta.schemaVersion: 1` | 增加 settings 域 revision 和空幂等台账；其余设置按现有规范化规则迁移 |
| `tasks.json` | `_meta.schemaVersion: 2` | 每项拆为 `{ id, spec, state, ui, createdAt, updatedAt }`；spec 使用 ScheduleSpec，state 保存运行信息，ui 暂存 expanded/editing；保留 runtimes；初始化 schedule revision/幂等台账 |

schedule v8→v9 必须使用下表的字段映射，不得由实现者自行猜测：

| v8 字段 | v9 落点 | 转换规则 |
|---|---|---|
| `id` | wrapper.id | 原样保留，不重新生成 |
| `name`、`enabled` | spec.name、spec.enabled | 校验后保留；无效 name 使用“未命名定时任务”并写 warning |
| `expanded`、`editing` | wrapper.ui.expanded/editing | 保留 GUI 状态；不进入 ScheduleSpec，不允许 CLI 修改 |
| `createdAt`、`updatedAt` | wrapper 同名字段 | 有时区的合法 ISO 原样规范化；无效值按通用迁移错误处理并报告 |
| `runCount` | state.runCount | 非负整数原样保留 |
| `lastRunAt` | state.lastRunAt | 合法 instant 原样规范化 |
| `nextRunAt` | 不直接迁移 | 完成 spec 迁移后由 scheduler 重新计算；旧值只写迁移诊断 |
| `lastStatus` | state.lastStatus | idle/success/failed/stopped 保留；running 因旧进程不可能存活，转 stopped 并注明 migration interrupted |
| `lastExitCode` | state.lastExitCode | number/null 原样保留 |
| `lastStdout`、`lastStderr` | state 对应摘要 | 各截断至 64 KiB；不伪造历史运行记录 |
| `trigger.everySeconds` | interval/condition `every` | 正整数秒转 duration；按 d→h→m→s 选择最大可整除单位，例如 86400→1d、3600→1h、90→90s |
| `trigger.repeatCount` | interval `maxRuns` | 0→缺省（无限），正值→maxRuns；其它 trigger 丢弃 |
| `trigger.runAt` | once `at` | 仅 once 保留，按下述旧本地时间规则补全；其它 trigger 丢弃 |
| `trigger.cron` | calendar `cron` | 仅 calendar 保留，并补迁移时宿主的 IANA timezone；其它 trigger 丢弃 |
| `trigger.stopCondition` | interval `stopWhen` | enabled=true 且 pattern 非空时转 `{stream:"stdout",mode,pattern}`；否则缺省 |
| `trigger.probeAction` | condition `probe` | 仅 condition 按 script/executable 规则迁移并移除所有 notifications；旧 probe 为 notification 或缺少必填字段时禁用并报告；其它 trigger 丢弃 |
| `trigger.probeCondition` | condition `when` | enabled=true 且 pattern 非空时转 Match；不满足则禁用该 schedule 并报告 |
| `action.type=script` + `scriptMode` | spec.action.type/source | path 只取 filePath→`source:{type:"file",path}`；inline 只取 code→`source:{type:"inline",code}`；另一分支残留字段丢弃且不得执行 |
| 内置 `action.language` + `interpreter` | script.language/interpreter | python/javascript/powershell/bash/makefile 保留；非空 interpreter 作为显式 override |
| `language:custom` + `interpreter` | executable program/args | interpreter 必须非空；path→`program=interpreter,args=[filePath,...args]`；inline→`program=interpreter,args=["-c",code,...args]`，保持 v8 实际调用顺序 |
| `action.arguments` | script/executable `args` | 使用 v8 兼容分词；无法无损解析则禁用并报告 |
| `action.executablePath` | executable `program` | 仅 action.type=executable 保留 |
| `action.workingDirectory` | script/executable `workingDirectory` | 非空时规范化；目录失效则保留值、禁用并报告 |
| `action.notification` | notification action.notification | 仅 action.type=notification 保留并转 Notification |
| `notifyOnComplete` + `completionNotification` | `notifications.onComplete` | enabled=true 才保留；false 时缺省 |
| `stdoutNotification` | `notifications.onOutput` | enabled=true 且 condition 有效时转 `{when,notification}`；否则缺省 |
| AppNotification `durationMs` | Notification `duration` | 正整数毫秒按 d→h→m→s→ms 选择最大可整除单位；title/message/tone/position 按 schema 迁移 |

迁移 action 时先看 action.type，再看 scriptMode；迁移 trigger 时先看 trigger.type。表中标记丢弃的无关字段不得进入 spec、不得执行，但完整旧 tasks.json 必须保留在备份中，未知旧字段也写入迁移报告。

旧 `runAt` 可能是无时区、无秒的本地墙钟时间。若字符串已有 Z/offset，则按其 instant 规范化；否则仅在缺少秒时补 `:00`，并使用迁移时宿主 OS 解析出的本地 IANA timezone。DST 重叠时选择较早 instant，DST 空洞时顺延到第一个有效 instant，均写 warning；若无法取得 IANA timezone，则禁用受影响的 once/calendar schedule 并要求用户 patch，不使用猜测的固定偏移。迁移报告记录采用的 timezone。

旧 arguments 解析、路径规范化、正则和模板校验只使用 Domain Core 的同一实现。任一当前分支必填字段缺失、custom interpreter 为空或结构无法无损转换时，保留原始备份、将任务迁移为 disabled、写明字段级错误，不阻止其它合法任务迁移。迁移结束还要校验 spec/state：once 或 condition 已有主动作 runCount 时保持 disabled；interval 在 maxRuns 存在且 runCount>=maxRuns 时转 disabled/stopped；其它 enabled 任务校验通过后再计算 nextRunAt。

迁移中断后下次启动应从原文件或备份安全重试，不能产生半迁移状态。迁移成功前不删除备份；迁移过程不得清理未引用图片、孤儿 item 或动态 map key。

#### 4.2.3 v9 完整数据目录

```text
todo-note-data/
├── data.json                       # category/entry/item、背景、data revision/幂等记录
├── settings.json                   # 应用设置、settings revision/幂等记录
├── tasks.json                      # ScheduleSpec + state/ui、runtimes、schedule revision/幂等记录
├── img/
│   ├── avator/                     # 保持现有兼容目录名
│   ├── background/
│   └── data/<entry-id>/
├── history/
│   ├── schedule.ndjson             # 调度运行历史及截断后的 stdout/stderr
│   └── audit.ndjson                # 写事务摘要，不保存完整正文或密钥
├── backups/<timestamp>/            # 迁移和高影响写入前的完整 JSON 备份集
├── runtime/host.json               # 临时 Host 描述：协议版本、PID、数据目录、IPC 端点与认证 token
├── runtime/recovery.json           # 附属文件操作的持久恢复记录（存在未完成操作时）
├── runtime/host.owner.lock         # 按 data-dir 区分的 Host 生命周期所有权锁
└── .kxtodo.lock                    # 仓库事务锁载体
```

SKILL 正文编译期嵌入 exe；只有显式执行 `skills persist` 时才生成外部副本。

三个 JSON 的 `_meta` 分别保存本域 revision 与有界幂等记录；幂等记录默认保留 30 天且每域最多 1000 条。`history/schedule.ndjson` 默认每个 schedule 保留最近 100 次、全文件不超过 20 MiB，单次 stdout 和 stderr 各最多 64 KiB，并标记截断；audit 日志轮转后总量不超过 10 MiB；备份默认保留最近 5 组。原子写可能产生同目录临时文件，但成功后必须清理；启动和 `doctor` 要识别并处理崩溃遗留。`runtime/host.json` 在正常退出时删除，发现 PID/端点失效时按 stale descriptor 清理。

`config path` 必须逐项返回以上路径、实际限制和状态；`doctor` 必须检查格式版本、revision、幂等台账、历史上限、备份、临时文件、锁、Host 描述符、失效引用和孤立图片。doctor 默认只报告，任何清理必须是独立的 high-risk-write，并再次确认。

### 4.3 数据一致性、GUI 联动与冲突

GUI 与 CLI 双线并行的首要约束是禁止丢失更新。Repository 必须提供：

1. **跨进程文件锁**：一次写事务覆盖读取当前版本、校验、修改和落盘全过程。
2. **原子写入**：写临时文件、刷新并原子替换；损坏或无法迁移时停止写入，不能用默认空数据覆盖。
3. **per-domain revision**：data、settings、schedule 各自使用从 0 开始的持久化单调整数，存于对应 JSON 根 `_meta.revision`。任务命令只比较/递增 data，配置命令只比较/递增 settings，调度定义和状态命令只比较/递增 schedule，互不造成无关冲突。响应返回 `revisionDomain` 和该域 `revision`；`--if-revision` 只校验当前命令的目标域。
4. **事务级业务操作**：新增、移动、完成、级联删除等在一次事务中完成；CLI 和 GUI 都不得自行“读全量—改 JSON—写全量”。
5. **同域幂等**：幂等键与首次结果摘要写入同一域 `_meta`，与业务修改一同原子提交。
6. **备份边界**：迁移和高影响写入前保留有效备份；图片等附属文件操作失败时事务必须回滚或留下可由 `doctor` 完成的明确恢复记录。

GUI 运行时，GUI 和 CLI 的所有持久化写入都必须提交给 Background Host 的 Domain Core。当前 `commit/commitScheduler/commitSettings → save*` 的全量写盘路径必须被替换，不得仅依靠 revision 检测继续保留。Host 提交事务后发出包含目标域、revision、变更资源 ID 和必要新值的事件；前端 store 只是 Host 状态的视图缓存，并按事件增量更新或重新加载。CLI 成功响应只能在事务落盘且刷新事件已经发出后返回。

GUI 正在编辑同一 item 时采用固定冲突策略：

- 外部变更正常提交，但不覆盖编辑器中的本地未保存草稿；该资源标记为 pending conflict，其它资源仍即时刷新。
- GUI 保存草稿时携带开始编辑时读取到的 item `updatedAt`；若当前 `updatedAt` 已变化，Host 返回 conflict，GUI 保持编辑状态和草稿，并使用现有 toast 明确提示“外部版本已变化，本次未保存”。不得静默覆盖任一版本。
- 用户取消编辑或重新打开资源后加载远端最新版；用户若要保留草稿，可复制后基于最新版再次编辑。v9 不要求新增复杂合并界面。

可验收的即时联动标准：

- GUI/Host 已运行时，CLI 对 task、schedule 或 config 的成功修改应在命令返回后 1 秒内出现在 GUI；删除当前选中资源时 GUI 自动选择有效回退项。
- 唯一例外是上述正在编辑的同一 item，其草稿保留并在 1 秒内出现冲突提示。
- GUI 未运行时，CLI 在独占锁下直接调用同一 Core；GUI 下次启动必须读取最新 revision 和数据。
- GUI 产生的修改同样经 Host 落盘，CLI 紧随其后的读取必须看到最新值。

### 4.4 Background Host、单实例与 CLI 生命周期

Background Host 是单实例、主窗口、IPC、通知和调度器的所有者。现有只围绕 GUI 初始化的单实例逻辑必须提升为 Host 级状态机：

```text
Absent ──需要后台能力──> Hidden Host ──无参数启动──> GUI Visible
Absent ──无参数启动────> GUI Visible ──关闭到托盘──> GUI Hidden/Host Running
任一运行态 ──显式退出或崩溃──> Stopping ──释放资源──> Absent
```

规则如下：

- Host 身份按规范化后的数据目录确定，同一数据目录同一用户会话只允许一个 Host。默认数据目录的 Host 同时拥有唯一 GUI/托盘；自定义 `--data-dir` 如需调度可启动无主窗口的 headless Host，不得切换默认 GUI 的仓库。
- Host 启动时取得单实例所有权并发布 owner-only IPC 端点；`runtime/host.json` 只作发现描述，不能代替 OS 级单实例锁。
- 无参数启动发现默认 Hidden Host 时，通过 IPC 要求原 Host 创建或显示主窗口；发现 GUI Host 时只聚焦窗口。不得再启动第二套 Core。
- CLI 发现匹配 Host 时必须经 IPC 调用；协议不兼容、Host 无响应或锁状态矛盾时返回明确错误，不得回退为盲目直写。只有在取得同一 Host 启动互斥、再次确认没有 Host 后，纯数据 CRUD 才可在短进程中直接调用同一 Core，并持有该互斥直到仓库事务结束，避免与 GUI/Host 启动竞态。
- 需要通知、调度启用或立即运行时，Host 不存在则由一个带启动互斥的 launcher 拉起同一可执行文件的内部 Hidden Host，并等待 ready；内部模式不作为普通用户命令公开。
- 仅因 notify 启动的 Hidden Host 在最后一个通知关闭且没有 GUI、启用中的 schedule 或等待请求时应自动退出；承载启用任务的 Host 持续运行。
- 默认数据目录遵循 `settings.lifecycle.launchAtStartup`：为 true 时操作系统启动同一可执行文件的 Hidden Host，Host 在 WebView 之前加载并恢复 enabled schedules；为 false 时不擅自修改开机设置，任务仅在 Host 运行期间执行，下一次启动按 missedPolicy 处理错过次数。`schedule status` 必须报告 Host/开机恢复是否可用并给出修复提示。
- 自定义 `--data-dir` 的 headless Host 在 v9 不自动注册系统开机启动；启用任务时和 `schedule status` 必须明确返回该限制。它在当前会话可持续运行，机器重启后需显式针对该 data-dir 启动；不得让自定义仓库悄悄绑定默认 GUI 的 autostart。
- Host 崩溃后 OS 必须释放单实例锁和仓库锁；下一次启动验证 PID/IPC，清理 stale 描述并恢复。显式退出先停止接收请求、取消或回收子进程、刷新状态，再释放端点和锁。

CLI 生命周期必须保持短且可预测：help/version/schema/skills 直接执行；数据 CRUD 在原子事务完成后退出；`notify` 一律由 Host 持有弹窗，确认创建后退出；`schedule run` 默认入队后退出。只有显式 `notify --wait` 或 `schedule run --wait` 才等待最终状态。所有路径都要刷新 stdout/stderr 并返回稳定退出码，普通 CLI 不进入 Tauri GUI 事件循环。

本机 IPC 只接受当前用户会话，校验协议版本、数据目录、请求大小和请求 ID。Host 返回的 envelope 与 standalone CLI 完全一致，调用者不应感知执行位置差异。

### 4.5 调度器架构

现有前端调度循环应迁移到 Rust 后台主机，使其不依赖 WebView。调度器分为：

- **定义仓库**：保存任务定义、启用状态和下次运行时间。
- **计划器**：计算 once、interval、calendar、condition 的下次触发，处理时区和机器休眠恢复。
- **执行器**：按 argv 数组启动解释器或程序，不经过 shell 拼接；支持超时、取消、工作目录、输出上限和子进程回收。
- **状态与历史**：保存当前状态和有界运行历史，定义文件与大段日志分离，避免 `tasks.json` 无限增长。
- **通知器**：复用统一桌面通知能力。

运行策略必须明确：

- 同一个 schedule ID 同一时刻默认只运行一个实例，避免间隔任务重入；未来如需并行应增加显式策略。
- 机器休眠或主机停止期间按任务的 `missedPolicy` 执行 `skip` 或最多 `run-once`，记录 `lastMissedAt/missedCount`，不能瞬间回放历史次数。
- 启用、修改和主机恢复时重算 `nextRunAt`。
- condition 探测和主动作均受超时、取消和输出限制。
- 删除或停止任务必须回收其子进程。
- Android/iOS 不具备相同进程执行能力；v9 CLI 与脚本调度执行以桌面端为目标，移动端可继续读取同步后的状态，但不得伪装执行成功。

### 4.6 安全、可观测性与可扩展性

- 参数值始终按 argv 处理；脚本和可执行参数不通过 shell 字符串拼接，降低注入风险。
- inline 脚本、外部程序、网络下载和级联删除在帮助/schema 中显式标记风险，遵守 `--dry-run`、退出码 10 和 `--yes` 确认协议。
- stdout/stderr、运行日志和错误信封不得意外输出未来可能加入的密钥；配置读取预留脱敏机制。
- 每次 CLI 调用生成 request ID；写事务记录 command、目标 ID、时间和结果摘要，便于定位 Agent 重试和并发问题，但不默认记录完整 Markdown 或脚本正文。
- `--idempotency-key` 的记录应有作用域和保留上限，响应可区分首次执行与重放。
- 命令 catalog 生成 parser/help/命令 schema；Domain Model 生成 payload schema/GUI 类型；Layout Descriptor 生成路径查询/诊断。独立 SKILL 不复制这些事实，只通过 `skills validate` 和 CI 校验引用，防止版本漂移。
- 后续扩展同步、导入导出或更多资源时，继续采用顶层领域 → 动作、统一 envelope、风险等级和共享 Domain Core，不把新逻辑直接堆入 CLI 参数分派或前端 store。

最终交付应保证：现有 GUI 布局、视觉和正常操作流程不变；全部 CLI 命令按契约退出；默认 JSON 可被 Agent 稳定解析；GUI/CLI 并行写入不丢数据且满足 1 秒联动验收；Host/单实例崩溃可恢复；定时任务在后台可靠运行；帮助、schema 与独立单文件 SKILL 足以让 Agent 在不了解内部 JSON 格式的情况下完成查询、创建、修改、删除、调度和配置操作。上述 v9 必须项不得以“后续优化”为由省略。
