# CLI 约定、命令面与 Agent 技能文档

> **这份文件是什么**：`kxtodo-cli` 这一侧的约定汇总——加/改命令要动哪里、core 命令名（camel）与 CLI 子命令名（kebab）的映射、确认门与退出码、CLI 的命令路由（有常驻 Host 时全部经 IPC）、哪些命令不要求数据目录已存在、`--jq` 与 `schema`、Windows 上的编码坑、以及 Agent 技能文档 `skills/kxtodo/SKILL.md` 的维护规则。
> **什么时候读它**：加/改任何 CLI 命令或参数、给某个动作加确认门、排查 CLI 退出码、CLI 中文参数乱码、要更新产品自带的 Agent 技能文档。
> **注意分工**：这份文件是**开发者视角**（怎么改 CLI）。`skills/kxtodo/SKILL.md` 是**产品自带的、给外部 Agent 用的 CLI 使用手册**（何时调用哪个命令），它编译期 `include_str!` 进 exe，是本仓库要维护的产物之一。本 skill（`develop-kxtodo`）不复制它的内容。

## 目录

- [加 / 改 CLI 命令要动哪里](#加--改-cli-命令要动哪里)
- [命令名口径：core 是 camel，CLI 是 kebab](#命令名口径core-是-camelcli-是-kebab)
- [确认门（high-risk-write）与退出码](#确认门high-risk-write与退出码)
- [CLI 的命令路由：有常驻 Host 时全部经 IPC](#cli-的命令路由有常驻-host-时全部经-ipc)
- [不要求数据目录已存在的命令](#不要求数据目录已存在的命令)
- [--jq 与 schema](#--jq-与-schema)
- [列表分页与合计（v0.8.0）](#列表分页与合计v080)
- [Windows 上的 CLI 编码坑](#windows-上的-cli-编码坑)
- [Agent 技能文档（skills/kxtodo/SKILL.md）](#agent-技能文档skillskxtodoskillmd)
- [相关但住在别处](#相关但住在别处)

## 加 / 改 CLI 命令要动哪里

路由表原文（同一行也在 SKILL.md 的路由表里）：

> **加/改 CLI 命令** → `crates/core/src/cli.rs`（clap 树）+ 对应 `ops_*.rs`；`schema.rs`/`skills.rs` 自动跟随

配套要知道的：

- 加 GUI 写操作走的是另一条：`crates/core/src/ops_gui.rs` 加命令 → `actions.ts` 加 coreDispatch 包装 → 组件调用 actions。CLI 与 GUI 共用同一条业务命令层（Domain Core 的 Invocation → 域分发 → envelope 输出），见 `architecture.md`「进程拓扑」。
- 加一类**要同步的**实体时，`core.rs` 与 `cli.rs` 的 `schemaVersions` 也要跟着改（完整清单见 SKILL.md 路由表那一行）。
- **help 里的「示例」是回归对象**：`cli_misc.rs::help_examples_parse` 会遍历所有层的 `long_about`，把 `kxtodo-cli ...` 开头的示例逐条 `try_parse_from`（按 shell 习惯切词、跳过带 `<`/`|` 的元语法、`#` 起头的词当行尾注释）。命令改名而示例没改会直接红——v0.8.1 之前 `ledger account-remove` 那三条写的是 core 内部名（`accountRemove`），照抄必失败。
- 调度触发/动作类型的白名单校验在 `ops_schedule.rs`，执行在 `plan.rs` / `scheduler.rs`。
- **`item_view` 的日期字段永远在场**（v0.8.1）：`plannedDate` / `dueDate` / `dueTime` 缺失时分别给 `null` / `null` / `""`。早先是条件插入，于是「未排期」的条目干脆没有这三个键——按固定形状解析的客户端（Agent 的 jq 表达式、脚本）一遇到没排期的条目就取到 undefined，还得写两套分支。
- **有真实默认值的分页参数要在 clap 上写 `default_value`**（v0.8.0 补齐：`task list`/`task find`/`schedule list`/`schedule find` 的 `--limit` = 50、`schedule logs` = 20——core 里本来就是这些值，`schedule logs` 是 `unwrap_or(20)`；写上后 `--help` 与 `schema` 都能机读默认值、行为不变）。**`ledger list` / `diary list` 的 `--limit` 刻意不加**：它们没有默认值 = 返回全部（见「列表分页与合计」）。`--cursor` 的帮助文案统一是「分页游标（取上一页 meta.nextCursor）」、`--all` 是「输出全部（忽略分页）」。

## 命令名口径：core 是 camel，CLI 是 kebab

原文（记账那一域的例子，全文在 `ledger.md` 的「CLI 子命令名与 core 命令名的映射」）：

> CLI 子命令是 kebab（`ledger account-add` 等；core 命令名仍 camel，`build_ledger_invocation` 做映射）。

## 确认门（high-risk-write）与退出码

- **一个动作只有一道门**：记账的分发层 `ledger_dispatch` 对 add/transfer/modify/accountAdd/accountModify/categoryAdd/categoryModify 统一 `require_confirmation`，而 remove/accountRemove/categoryRemove/import 保留各自信息量更大的内部门。未带 `--yes` 返回**退出码 10**，文案明确告诉 Agent「金融数据敏感，先向用户说明这次增删改并得到同意」；只读动作不设门。全文见 `ledger.md` 的「CLI 改账本必须先过确认门（v0.7.2）」。
- **GUI/Android 桥恒带 `controls.yes = true`**，确认门不适用于 GUI（GUI 操作即用户确认）——见 `invariants.md`。
- `schema.rs::risk_for` 是动作风险等级的唯一来源，这些写账动作全是 high-risk-write。**条件式风险**（`riskCondition`）：`schedule.enable` / `schedule.run` 报的是**最坏情况** high-risk-write，真正要 `--yes` 的只有 `action.type` 为 `script`/`executable`（会执行代码）那类——schema 与 help 都这么说，差别由 `schema.rs::risk_condition` 结构化给出（v0.8.1 之前三处说法不一致：schema 说 high-risk、help 说 write、实际是条件式）。
- **`-` 开头的文本值要能传进去**：纯文本/名称类参数（`--markdown` / `--name` / `--title` / `--note` / `--account` / `--category` / `--query` / `--initial` / `--balance` / `--amount`…）都带 `allow_hyphen_values = true`，否则 `--markdown "- 列表项"` 会被 clap 当成 flag 报错（v0.8.1 补齐 38 处）。**两条例外**：`--spec` / `--patch` / `--value-file` **不许加**（会破坏 `-` 表 stdin 的语义）；多值参数（`--tag` 这种可重复的）也不加——漏写一个值会被静默吞成下一个参数。`--amount` 加了是为了让「金额必须大于 0」这条**业务错误**浮上来，而不是 clap 的「未知参数」。
- 退出码散见各处，按错误查：
  - **3** = 资源不存在 / `DATA_DIR_NOT_FOUND`（CLI 最终没有 `data.json`，**CLI 永不静默创建数据**）→ `architecture.md`「数据目录解析」
  - **4** = 歧义或状态冲突，如 `sync now` 在暂停时报 `SYNC_PAUSED`、server `--daemon` 起不来告警 → `sync.md`
  - **10** = 高风险未确认（未带 `--yes`）→ 上面这条 + `ledger.md`
  - `skills persist` 的目标路径被同名文件/目录挡住且未加 `--yes` 也报 confirmation（退出码 10）→ 见本文「Agent 技能文档」

## CLI 的命令路由：有常驻 Host 时全部经 IPC

原文（在 `sync.md`「统一 pair（v0.6.0）」小节里）：

> CLI 路由注意：数据目录有常驻 Host 时 CLI 的**所有**命令都经 IPC 转给 Host 执行，所以 `sync configure --lan-host true` 会真的让正在运行的 GUI 起停内置服务器。

## 不要求数据目录已存在的命令

原文（在 `sync.md`「局域网自动发现（v0.5.0）」小节里）：

> CLI `sync discover [--timeout-ms]`——该命令**不要求数据目录已存在**（它是配对之前的动作，`command_needs_data` 里显式放行）。广播本身失败（容器禁 UDP 之类）按「没发现」处理并报错 `SYNC_LAN_HOST_NOT_FOUND`（Io 类，走静默重连），别把 socket 错误抛给用户。

`sync pair` 同样不需要数据目录已存在（内部会初始化）。放行名单在 core 的 `command_needs_data`——加「配对之前的动作」类命令时记得在那里显式放行。

（**核实补充**，2026-09 对着 `crates/core/src/cli.rs` 的 `command_needs_data` 核过）当前**不需要**数据目录的命令白名单就是这几条：`schema`、`skills`、`version`、`doctor`、`sync pair`（源码注释：「配对是显式的设备初始化动作：允许在空数据目录上执行（内部 ensure_initialized）」）、`sync discover`（源码注释：「发现是配对**之前**的动作：全新设备上数据目录还不存在也必须能跑」）；其余一律 `true`。缺 `data.json` 时报 `DATA_DIR_NOT_FOUND`，hint 是「用 `--data-dir` 显式指定数据目录；也可以先运行一次 GUI 创建数据」。

## --jq 与 schema

（**核实补充**：原 AGENTS.md 全文没有出现过 `jq`，下面这条是 2026-09 按源码核实后补的，避免以后又漏。）

- `crates/core/src/jq.rs` 的头注释原文是：`//! Built-in jq-compatible subset for `--jq` (§3.2).` / `//! Supported syntax is listed by `kxtodo-cli schema jq`.`——即 CLI 自带一个 jq 兼容子集，支持哪些语法由 `kxtodo-cli schema jq` 自己吐（`JQ_SUBSET_DOC` 常量），不在文档里另抄一份。
- **v0.8.0 扩了两块语法**：`{a, b}` 对象构造（`{a}`/`{.a.b}` 简写，键取路径最后一段；`{"名字": .a.b}` 显式键，裸标识符或单/双引号；`{}` 空对象；某字段产出多值时按 jq 语义做笛卡尔积；键顺序 = 写下的顺序）与 `.a, .b` 逗号多输出。程序模型从 `Vec<Stage>` 改成 `Vec<Segment>`（`Segment = Vec<Stage>`）：**先按 `|` 切段、段内再按 `,` 切分支**——jq 里 `,` 优先级高于 `|`；`split_top_level` 也跟踪 `{}`/`[]` 深度。顺带**修好 `.[]`**：`JQ_SUBSET_DOC` 一直写着支持 `.a[] 或 .[]`，实现却在 `.` 后遇到 `[` 就报「`.` 后必须是字段名」——文档与实现的漂移以文档为准修掉了。
- **语法错误的 hint 内联整条支持清单**（`SUPPORT_SUMMARY` 常量），不再让调用方跑一次 `schema jq`（那条往返在自动化里就是白烧一轮）；**`SUPPORT_SUMMARY` 与 `JQ_SUBSET_DOC` 必须同步改**——有测试同时断言两边都含新语法，防再漂。
- **`--jq` 与 `--format` 冲突时不静默丢**：`--jq` 生效时 `--format` 没有落点，非 json 时 `render.rs` 在 **stderr** 出一行提示（stdout 必须保持可被脚本解析的纯 JSON，多一个字符就炸）；jq **失败**路径刻意不拼提示（那时 stderr 是错误信封，调用方要解析它）。`Format::as_str()` 是提示文案用的。
- `schema` 命令面由 `crates/core/src/schema.rs` 驱动；`schema.rs` 与 `skills.rs` **自动跟随** `cli.rs` 的 clap 树（加了命令不用手工同步这两处）。
- `skills.rs` 负责把 `skills/kxtodo/SKILL.md` 编译期嵌入并提供 `skills persist`。

## 列表分页与合计（v0.8.0）

- `ledger list` / `diary list` 补了 `--cursor` 与 `--all`（复用 `ops_task::{Page, paginate, parse_cursor}`），**默认行为未变**：不传 `--limit` 仍返回全部。专用入口 `core.rs::unbounded_page_from`（与 `page_from` 唯一差别是 limit 缺省 = `usize::MAX` 而不是 `DEFAULT_PAGE_LIMIT`），注释写明理由：**记账是金融数据，静默截断会让调用方把一页当成全月合计**。`paginate` 的 `start + page.limit` 用 `saturating_add`——`--limit 18446744073709551615` 在 debug 构建下会溢出 panic。
- **`meta.count`（= `data.total`）与 `meta.nextCursor` 双写**；`render.rs::render_pretty` 的兜底分支原来是 `_ => return render_kv(data)` 提前返回，永远走不到合计行，改成键值输出后再补一行「共 N 条」（命中的只有 ledger.list 与 diary.list，其它命令都有专属渲染器）。
- **`diary list` 的 `data.total` 是过滤后的篇数**（原来是 `file.entries.len()`，即全库篇数）：`diary list --from 月初 --to 月末 --jq .data.total`（「这个月写了几篇」）原来报的是全库篇数；现在与 ledger list 同口径，`total` 与 `returned` 并排读作「命中 N 篇、本页 M 篇」。
- **`ledger stats` 的 grain 与 series**：day 粒度判据 = **闭区间 ≤ `DAY_GRAIN_MAX_DAYS`（62）天**（旧判据「from 与 to 同一个自然月」被新判据完全包含——`--month`/`--year`/长区间/开区间行为不变，唯一变化是跨月的短区间从两个月桶变成逐日）；**day 粒度的 `series` 严格跟着 `--from/--to`**（原来给的是 from 所在自然月的整月，做周报的调用方必须自己裁）——`enumerate_days`（chrono `NaiveDate` + `succ_opt`，跨月/跨年/闰月天然正确，倒挂区间返回空表）/ `enumerate_months` / `series_keys`（横轴单一出处）/ `span_days`，硬顶 `MAX_SERIES_KEYS = 1200`，序列按横轴取、缺档补零。**62 这个数与前端 `ledger.ts::bucketOf` 跨语言同口径，由 `include_str!` 测试钉住**（见 `ledger.md`）。stats 本体已改单趟聚合、JSON 最后组装，输出与旧实现逐字节一致并由 golden 测试钉死序列化文本。

## Windows 上的 CLI 编码坑

全文在 `pitfalls-windows.md`「另外三条与 Windows 有关的坑」（原始出处是 `sync.md` 的「踩坑记录」①–⑦）：

- **④ Windows CLI 中文参数乱码**（代码页 936）：`std::env::args()` 按系统 ANSI 代码页解码命令行，中文必乱码——cli 入口已改 `args_os` + `embed-manifest` crate 嵌入进程级 UTF-8 代码页 manifest（手写 `/MANIFESTINPUT` 链接参数会造成 side-by-side 启动错误，勿回退）。
- **② PowerShell 5.1 跑 CLI 测试脚本要 `[Console]::OutputEncoding=UTF8`**，否则 GBK 解码 UTF-8 JSON 炸。

## Agent 技能文档（skills/kxtodo/SKILL.md）

**`skills validate` 的域名单是从 clap 命令树现推的，不是手写白名单**（v0.8.1 修）：早先正则里硬编码了 `task|diary|schedule|config|skills|doctor|notify|schema|version`，后来加的 `ledger` / `storage` / `sync` 三个域压根不在里面——金融数据的命令名写错也不会有任何校验报错，校验等于漏了一半。现在凡顶层子命令都自动进正则，有子命令的域才把第二个词当动作名去比对 `command_catalog()`。

路由表原文：

> **Agent 技能文档** → 只编辑 `skills/kxtodo/SKILL.md`（编译期 include_str! 嵌入，发布 exe 自包含）。`skills persist` 不指定位置时默认写 `~/.agents/skills/kxtodo/SKILL.md`（v0.6.11）；已存在的 SKILL.md 直接覆盖，路径被同名文件/目录挡住时未加 `--yes` 报 confirmation（退出码 10）询问 y/N；结果里的 `data.path` 就是最终落地路径

维护规则（v0.8.0 起是硬约束）：

- **改这份文件必须重跑 `kxtodo-cli skills validate`**（有 core 测试盯着）：`cmd_validate` 用两条正则扫全文——任何 `task|diary|schedule|config|skills` 后面跟的**小写英文词**都会被当成命令名去比对命令目录（`task exportMarkdown` 这种写法会被解析成不存在的 `task.export` 而报错），任何 `--xxx` 都必须在 flag 目录里。**写文档时引用一个不存在的子命令或参数会直接让测试挂掉**。
- v0.8.0 把它整个重写过一次：frontmatter `version` 1→2（**`skills.rs::SKILL_VERSION` 要同步抬**，否则 `cmd_validate` 会拒）；description 补全完整能力与触发场景以提高命中率；正文按「一分钟上手 / 用哪个域 / 通用约定 / 各域怎么用 / 典型流程 / 注意事项」重排，补上此前完全没写的 `--jq`（含新语法）与 `--format`、退出码 1、新的分页语义、stats 的 grain/series 语义；砍掉同步的实现细节（P2P relay、instance epoch、端口生命周期）只留排障需要的错误码。

## 相关但住在别处

| 主题 | 住在哪 |
|---|---|
| CLI 不持有状态、需要常驻能力时经 IPC 找 Host、Host 不在就拉起隐藏 Host（`--kxtodo-host`）、找不到 GUI 报 `GUI_NOT_FOUND` | `architecture.md`「进程拓扑」 |
| Linux 上 `find_gui_exe` 认固定名 `KXToDo.AppImage` 与软链为 `kxtodo` 的 GUI | `architecture.md`「进程拓扑」最后一条 |
| CLI 的数据目录解析（`--data-dir` > 系统默认；缺 `data.json` 报 `DATA_DIR_NOT_FOUND`） | `architecture.md`「数据目录解析」 |
| 全部 sync 子命令（pair / now / status / probe / configure / discover / peers / history / unpair）的语义与错误码 | `sync.md` |
| 全部 ledger 子命令（add / transfer / list / stats / balance / account-* / category-* / account-type-* / export / import / icon-list）与确认门 | `ledger.md` |
| diary 子命令与 Markdown 压缩包导入导出 | `ui-patterns.md`「日记」的「导入导出是给人看的 Markdown 树」 |
| schedule 子命令（spec/validate/add/modify/enable/disable/run/stop/logs/status）与触发器/动作类型 | `ui-patterns.md`「定时任务」+ SKILL.md 路由表「加调度触发/动作类型」行 |
| task 的 Markdown 压缩包/文件夹导入导出（`cards_export_zip` / `cards_import_zip` / `cards_import_folder`） | `ui-patterns.md`「一般卡片的 Markdown 压缩包」 |
| `storage.usage` / `storage.clean`（释放空间） | `ui-patterns.md`「设置抽屉」 |
| server 的进程级动作（`--daemon` / `--stop` / `--update` / pidfile） | `sync.md`「server 运维」+ `build-and-release.md` |
