---
name: kxtodo
version: 4
cliHelp: kxtodo-cli --help
description: >-
  用 KXToDo 的命令行管理本机的待办任务、日记、记账三类数据，外加定时任务、桌面通知、
  存储清理、导入导出备份与多端同步。能力包括分类/条目/任务的增删改查与关键词搜索、
  标签与截止日期、按日期区间检索日记、记一笔与转账、资金账户与两级分类管理、
  收支统计与大类占比、净资产与余额、Excel 与 Markdown 压缩包导入导出、
  定时跑脚本或弹通知、设置项读写。典型场景：用户说记一下、加个待办、提醒我、
  建个清单、写今天的日记、补记某天、记一笔账、转账、查余额、查净资产、
  今天花了多少、这个月开销、钱都花在哪、资产总览、这周或这个月或今年做了些什么、
  帮我写周报月报年度总结、导出备份、换机搬家、导入回来、占了多少空间、清理一下、
  配一下多设备同步、同步不动了——这些都应优先落到 KXToDo，
  而不是另建临时文件或散在对话里。
---

# KXToDo CLI

一份数据、三类内容：**task（待办）**、**diary（日记）**、**ledger（记账）**，各自独立存储、互不替代。
本 SKILL 讲**什么时候用哪个、怎么调、有哪些坑**。字段结构、枚举与默认值一律以
`kxtodo-cli <领域> <动作> --help` 与 `kxtodo-cli schema <目标>` 为准，本文不复制。

---

## 1. 一分钟上手

```bash
kxtodo-cli task tree                                  # 先看有哪些分类与条目
kxtodo-cli task add --type item --entry-id <id> --markdown "写周报" --idempotency-key w1
kxtodo-cli diary add --markdown "今天把同步分层重构完了" --mood 🙂
kxtodo-cli ledger add --amount 30 --account 微信 --category 午餐 --note 小面 --yes
kxtodo-cli ledger stats --month 2026-09 --jq '.data.totals'
```

**输出协议**：成功 → stdout `{"ok":true,"command","data","meta"}`；失败 → stderr `{"ok":false,"error":{"type","code","message","hint?"}}`。
判成功看**退出码 0**（或 `ok == true`）。`error.hint` 通常直接给出下一步该做什么，先读它。

**退出码**

| 码 | 含义 | 该怎么办 |
|---|---|---|
| 0 | 成功 | — |
| 1 | 内部错误 | 罕见；带上完整 stderr 报给用户 |
| 2 | 参数 / 校验错误 | 读 `error.hint`，再对照 `<动作> --help` |
| 3 | 资源不存在（含数据目录没初始化） | ID 猜错了 → 先 `task tree` / `list` 拿真 ID |
| 4 | 类型不符 / 状态冲突 | 检查 `--type`；非空节点删除要 `--cascade` |
| 5 | 数据 / 文件锁 / 文件系统错误 | 别重试写操作，先 `doctor` |
| 10 | 高风险动作未确认 | 向用户说明影响，得到同意后加 `--yes` |
| 20 | 执行失败（脚本 / 通知 / 同步等） | 看 `data` 里的原因 |

**只要字段，别自己解析 JSON**：`--jq` 内置一个 jq 子集（`--format` 会被忽略，并在 stderr 提示一行；stdout 保持纯 JSON）。

```bash
kxtodo-cli task list --type item --all --jq '.data.items | map({id, markdown, done: .completed})'
kxtodo-cli ledger stats --month 2026-09 --jq '.data.totals, .data.categories[0]'
kxtodo-cli diary list --from 2026-09-01 --to 2026-09-30 --jq '.data.total'
```

支持：`.`｜`.a.b`｜`."key"`｜`.a[0]`（可负）｜`.a[]` 与 `.[]`｜`a | b`｜`.a, .b`（逗号多输出）｜
`{a, b}` 与 `{"名字": .a.b}`（对象构造）｜`length`｜`keys`｜`first`｜`last`｜`map(expr)`｜`select(.a == 值)`。
完整清单与示例：`kxtodo-cli schema jq`。写错了错误里会内联同一份清单。

**给人看的输出**：`--format table|pretty|ndjson`（`--json` = `--format json`，是默认）。

---

## 2. 用哪个域

| 用户想干什么 | 用 | 关键差别 |
|---|---|---|
| 待办、提醒、要勾选完成的事、清单 | `task` | 有完成状态，必须挂在某个 entry 下 |
| 叙事记录、当天发生了什么、心情、补记某天 | `diary` | 无完成状态，不属于任何条目，按**归属日期**归档 |
| 花了多少、收入、转账、账户余额、资产、消费结构 | `ledger` | 金额是钱，写操作一律要 `--yes` |
| 到点自动跑脚本 / 弹通知 | `schedule` | 定义只能走 JSON spec |
| 弹一条桌面通知 | `notify` | 需要常驻的 GUI 承担 Host |

`task` 的层级：`category`（分类，可嵌套）→ `entry`（左栏条目）→ `item`（具体任务，Markdown 正文）。
`system`（我的一天 / 计划内 / 收藏）是只读内置视图。
**ID 全部不透明**：不得拼造、不得按前缀推断类型，一律先查再用。

---

## 3. 通用约定

- **数据目录**：`--data-dir` > 系统默认（Windows `%LOCALAPPDATA%\kxtodo\todo-note-data`，Linux `$XDG_DATA_HOME` 或 `~/.local/share` 下的 `kxtodo/todo-note-data`）。目录里没有数据 → 退出码 3，**CLI 永不静默创建数据**；让用户先打开一次 GUI（Windows `KXToDo.exe` / Linux `KXToDo.AppImage`）。
- **确认门**：删除、重置、启用/运行代码，以及**记账的一切写操作**都是 high-risk-write，未带 `--yes` → 退出码 10 且数据分毫不动。先 `--dry-run` 看影响范围（`--dry-run` 永不触发确认门）。
- **改与删都按稳定 ID**：`task modify --type item --id ...`。patch 语义 = 字段缺省不变、显式 `null` 或空串清空、数组整体替换。
- **幂等**：每个创建类写操作给一个自己的 `--idempotency-key`，重试安全（返回首次结果）。
- **乐观并发**：读响应里的 `meta.revision`，写时带 `--if-revision`。
- **时间**：日期 `YYYY-MM-DD`，时刻 `HH:MM`（秒会被舍掉），一律**本地时区**。
- **金额**：CLI 与 Excel 都是**两位小数的元**（第三位四舍五入到分），内部存整数分。

---

## 4. task 待办

- **选命令**：有真实关键词（标题、正文片段）→ `task find --query`；只有范围条件（「本周」「已完成」「某条目下」）→ `task list` + 结构化过滤，**不要把范围词当关键词**；不知道层级先 `task tree`。
- **分页**：`task list` / `task find` 默认每页 50（`--limit`），翻页用 `--cursor`（取上一页的 `meta.nextCursor`），`--all` 忽略分页一次拿全。**做汇总务必带 `--all`**，否则只统计到前 50 条。
- **写**：`task add --type category|entry|item`；item 必须给 `--entry-id`，正文用 `--markdown`（长文用 `--markdown-file <path|->`），空正文会被拒。
- **日期与时刻**：`--due-date` 配 `--due-time <HH:MM>`（可选，精确到分钟），`--due-time ""` 清除回到只精确到天。
- **提醒**（v0.8.3）：`--reminder` 可重复，两种写法——`due-<分钟>` = 截止前 N 分钟（`due-0` = 到点那一刻；这一类**必须同时给 `--due-date` 与 `--due-time`**）；`+30m` / `+1h` / `+2d` 或 RFC3339（`2026-09-20T09:00:00+08:00`）= 绝对时刻（相对写法从此刻起算，**已经过去的时刻会被拒**）。
  `task modify --reminder ...` 是**整体替换**（不给 = 不动；只写 `--reminder` 不带值 = 清空全部）；清掉日期或时刻会连带清掉「截止前」那一类。
  提醒只在**这台设备上 KXToDo 进程活着**时才可能弹（桌面常驻托盘、移动端要开着应用），错过的不补发——要「设备关着也响」得用 `schedule`。
- 条目视图里的 `plannedDate` / `dueDate` / `dueTime` **永远在场**（没有就是 `null` / 空串），可以按固定形状解析，不必先判键在不在。
- **删**：`task remove --type ... --id ... --yes`；非空节点要 `--cascade`（会连带子节点与任务，先 `--dry-run` 看数量）。
- **一般卡片的 Markdown 压缩包导出/导入只在 GUI 里**（条目的三点菜单），CLI 没有对应命令 —— 用户要这个就引导他到界面，别去找不存在的子命令。日记与记账的导入导出才有 CLI（见下两节）。

---

## 5. diary 日记

- **写**：`diary add --markdown "..."`，可带 `--title` / `--mood <emoji>` / `--weather <emoji>` / `--tag "color:text"`（可重复；只给 `color` 就是无文字标签）。颜色是十选一：`red` `orange` `yellow` `green` `cyan` `blue` `purple` `pink` `gray` `custom`，或**直接给 `#rrggbb`**（自动记成自定义色；`custom` 单独给会退回灰色）/ `--time <HH:MM>`。**标题与正文不能同时为空**。长正文用 `--markdown-file`。
- **`--date` 是「归属日期」不是创建时间**：补写昨天就传昨天的日期，`createdAt` 仍是现在（要连写作时刻一起补就加 `--time`）。同一天可以有多篇。
- **读**：`diary list [--date 某天 | --from 起 --to 止]`，按日期由近及远、同一天内按写作先后。
  **不传 `--limit` 就返回全部**（与 task 的默认 50 刻意不同：月度回顾要的是整月）；要分页给 `--limit` + `--cursor`，`--all` 强制全部。
  `data.total` = **过滤后命中的篇数**（同一个值也在 `meta.count`），`data.returned` = 本页篇数。
- **改**：`diary modify --id ... --date/--time/--title/--markdown/--mood/--weather`；心情/天气/标题传**空串 = 清除**；标签用 `--replace-tags` 整体替换。只改 `--date` 时写作时刻自动保留。
- **删**：`diary remove --id ... --yes`（写同步墓碑，删除会传播到其它设备）。
- **导出 / 导入**（给人看的 Markdown 树，也是搬家与备份的路径）：
  `diary export --out <path.zip> [--from --to]` → 包内 `年/月/YYYYMMDD[_序号][_标题].md`，元数据在 YAML front-matter，正文引用的插图进包内 `images/`，解压即可读。
  `diary import --zip <path> --yes` → 解析很宽容（没有 front-matter 的手写 md 也能进，日期退回文件名或目录；非法条目跳过并计入 `skipped`），插图落回本地且同名不覆盖。
  **同一天已有日记不算冲突**：直接追加成另一篇，不合并也不去重 —— 所以**同一个包导两遍会得到两份，别重试**。

---

## 6. ledger 记账

> **金融数据敏感**：`add` / `transfer` / `modify` / `remove` / `import` / `account-*` / `account-type-*` / `category-*`
> 全部是 high-risk-write。**先向用户说明这次增删改的金额、账户、日期并得到明确同意，再带 `--yes`**；
> 未带就报 `CONFIRMATION_REQUIRED`（退出码 10），数据不动。别把用户的沉默当同意。
> 读操作（`get` / `list` / `accounts` / `account-types` / `categories` / `stats` / `balance` / `export`）**永远不需要**确认。

**记一笔**

```bash
ledger add --amount 30 --account 微信 --category 午餐 --note 小面 --yes
ledger add --kind income --amount 18155 --account 储蓄卡 --category 工资薪金 --date 2026-09-01 --yes
ledger transfer --from 储蓄卡 --to 微信 --amount 2000 --yes      # 转账不计入收支统计
```

`--account` / `--category` **认名字也认 ID**（名字对人和 Agent 都更友好）。`--date` 缺省为本地今天，`--time` 可选到分钟，`--image <文件名>` 可重复传多张附图。返回体自带 `accountName` / `categoryName` / `categoryParentName` / `signed`，不必二次查表。

**查**

| 想知道 | 命令 |
|---|---|
| 某段时间的明细 | `ledger list [--date \| --from --to] [--kind] [--account] [--category]` |
| 各账户余额与净资产 | `ledger accounts` |
| 净资产 / 总资产 / 总负债 | `ledger balance`（信用卡负余额计入负债） |
| 两级分类有哪些 | `ledger categories [--side expense\|income]`（平铺数组按树序：大类后面紧跟它的子分类；每项带 `depth` 0/1 与 `parentId`） |
| 可用图标目录 | `ledger icon-list` |
| **收支统计与钱花在哪** | `ledger stats` |

`ledger list` 与日记同口径：**不传 `--limit` 返回全部**（记账是金融数据，静默截断会让月合计算出残值），要分页给 `--limit` + `--cursor`；`data.total` 是过滤后的总笔数。

**统计（周报 / 月报 / 年度总结的首选）**

```bash
ledger stats --month 2026-09
ledger stats --year 2026 --side expense
ledger stats --from 2026-09-14 --to 2026-09-21 --jq '.data.series'
```

三选一（都不给 = 全量），返回 `range`（from/to/grain/label）、`totals`（收入/支出/结余/转账）、`series`（时间序列）、`categories`（大类占比）。

- 回答「钱花在哪」**直接读 `categories`**（子分类金额已并进大类，带笔数与百分比），别自己逐笔加。
- **不带 `--side` 时 `categories` 里收入与支出混排**（先收入后支出），每项带 `side`，`percent` 是**该侧内部**的占比（两侧各自合计 100%）。算「支出占比」先按 `side` 过滤，别把两侧百分比相加。
- `series` 的横轴**严格跟着 `--from/--to`**：区间 ≤62 天按天逐日枚举（含两端、空档补零，周报就是这 7 天而不是整月），更长按月逐月枚举；不给边界的开区间按账目里出现过的日期/月份收表。门槛与 GUI 的统计视图同一个数。
- 转账不进 `totals` 的收支，只单独给一个 `transfer` 合计。

**账户、账户类型与分类**

- 账户：`ledger account-add --name <名> [--kind <类型>] [--initial <元>] [--icon <lucide 名>] --yes`、`account-modify`、`account-remove`。
  `--kind` 是**自由字符串**（现金/银行卡/公积金/医保…任意非空值），唯一有语义的是 `credit`（信用卡，负余额计入总负债）。
  `--initial` 可为负（透支/欠款开户）：`--initial -1500.00` 与 `--initial=-1500.00` 都认。
  `account-modify` 还接受 `--balance <元>` **直设当前余额**（自动反推期初 = 目标余额 − 流水净额；与 `--initial` 互斥，同给报 `LEDGER_PARAM_CONFLICT`）。
  **余额永远只有「期初 + 流水」一个数据源**，改历史账目不用回头修余额。**名下还有账目的账户删不掉**（`LEDGER_ACCOUNT_IN_USE`）。
- 自定义账户类型：`ledger account-type-add --name <名> [--icon] [--color <#rrggbb>] --yes`、`account-type-modify`、`account-type-remove`，只读 `account-types`。名字唯一；删掉仍被引用的类型是允许的（账户的 `kind` 存的是字符串，原样保留）。
- 分类：`ledger category-add --name <名> [--side expense|income] [--parent <大类名|ID>] [--icon] --yes`、`category-modify`、`category-remove`。
  **只有两级**（子分类下不能再挂）。删大类连带子分类，名下账目保留但变「未分类」。
  首跑自带一套种子账户与两级分类，**先 `ledger categories` 看现成的，别重复建同名分类**。
- **图标只认目录里的名字**：设计分类前先 `ledger icon-list`（分类目录 20 组两百多个 lucide 名，PascalCase 如 `UtensilsCrossed`；账户目录另有六组）。猜一个不存在的，界面画不出来只会退化成省略号。

**导出 / 导入**：`ledger export --out <path.zip> [--from --to]` → 包内一张 `kxtodo-ledger.xlsx`（说明 / 账户 / 分类 / 账目 四张表，金额是带符号的元）。
`ledger import --file <path.zip|.xlsx> --yes` → 只认这套表头，账户与分类按名字合并、缺的自动建，日期非法或金额为 0 的行跳过并计入 `skipped`。
**重复导入会产生重复账目，别重试**。搬家 = 全量导出 → 新目录导入。

---

## 7. schedule 定时任务

定义只走 JSON，五步：

```bash
kxtodo-cli schema schedule.spec --example interval-script --jq '.data.example' > schedule.json
# 编辑业务值（名称、脚本路径、时间）
kxtodo-cli schedule validate --spec @schedule.json
kxtodo-cli schedule add --spec @schedule.json --dry-run
kxtodo-cli schedule add --spec @schedule.json --yes --idempotency-key <唯一键>
```

改：`schedule modify --id ... --patch '<json>'`（patch 里 CLI 专属字段要原样带上）。
运行控制：`schedule enable|disable|run|stop`，历史 `schedule logs --id ... --limit 20`，状态 `schedule status`。
触发器有 once / interval / calendar / condition，动作有 脚本 / 可执行文件 / 通知。**启用与运行都是 high-risk-write，但是条件式的**：只有动作是脚本 / 可执行文件（会真的执行代码）才需要 `--yes`，纯通知动作不需要。拿不准就 `schema schedule.enable`（返回里的 `riskCondition` 写明何时要 `--yes`）。
定时任务只在桌面端有调度引擎（GUI 常驻 Host 才跑得起来），移动端没有。

---

## 8. 其余命令

- **`notify`** — 发桌面通知：`notify "构建完成" --title "CI" --tone success --duration 5s`，`--wait` 等窗口关闭。通知由与目标数据目录匹配的常驻 Host 持有；Host 不在会尝试拉起 GUI 同目录的程序，找不到报 `GUI_NOT_FOUND`。
- **`storage`** — 用户问「占了多少空间 / 清理一下」：先 `storage usage`（只读盘点：总体积 + 插图/背景/头像各自的总量与**无引用量** + **领域文件 / 历史记录 / 运行时 / 同步服务器（含 WAL）/ 服务器日志 / 备份**分项 + 临时残留），报数字给用户并得到同意后再 `storage clean --yes`（删无引用图片与空目录、临时残留、旧日志；**保留最新两份日志且绝不删今天的**）。返回 `freedBytes` 与逐项计数，单个文件删不动只进 `warnings`。
  **它永远不碰**：五个领域 JSON（数据本体）、`runtime/`（同步状态）、`history/`、`backups/`、服务器数据库与账户令牌。删除不可恢复。
- **`config`** — 读写设置：`config get appearance.uiScale`、`config list --prefix appearance`、`config set <路径> <值>`、`config unset`、`config reset`（高风险，先 `--dry-run`）、`config path`、`config validate`。动态 map（如 `appearance.uiColors`）必须带 `--map-key <entry-id>`。
  常用：字号 `appearance.uiFontSize`(14-22) / `markdownFontSize` / `ledgerFontSize` / `diaryFontSize`(14-26)；固定导航 `appearance.navItems`（字符串数组，可选 `my-day`/`planned`/`important`/`diary`/`ledger`/`scheduled`/`toolbox`，顺序即显示顺序）与 `appearance.navLayout`（`list|grid|icons`）；一周起始 `features.weekStart`（`monday`（默认）|`sunday`）、超链接渲染样式 `features.linkRender`（`off|title|card`，默认 `card`）、临期高亮 `features.dueHighlight`（`off|solid|gradient`，默认 `off`）；预置标签 `appearance.tagPresets`（标签数组，跟设置一起同步），数组用 `config set appearance.navItems --json-value '["diary","my-day"]'`。
  **视图偏好是本机状态不跨设备同步**（`diary.view` / `ledger.view`），**外观是同步的**（日记与记账的主题色与背景）。
- **`doctor`** — 体检：数据目录、五个领域文件的完整性与 schema 版本、常驻 Host、脚本环境。排查「数据好像不对」先跑它。
- **`schema`** — 机读的命令与结构定义：`schema task.add`、`schema schedule.spec [--example <名>]`、`schema jq`、`schema notification`、`schema match`。**要字段清单就查它，别猜**。
- **`skills`** — 本 SKILL 自身的分发：`skills list`、`skills read`、`skills path`、`skills validate`、`skills persist [--yes]`（落地到 `~/.agents/skills/kxtodo/SKILL.md`，已存在直接覆盖）、`skills echo`。
- **`version`** — 版本与五个领域文件的 schema 版本。

---

## 9. sync 多端同步

只在用户明确要配同步或排查同步时才用；日常记录数据不需要碰它。

- **三种通信方式**（`sync configure --mode lan|server|p2p`，共用同一套加密 / LWW / 墓碑内核，区别只在「连哪儿」）：
  `lan` 局域网（本机当主机，或选定局域网里的一台）｜`server` 自建常开服务器（手填 ip:port）｜`p2p` 无公网 IP 的跨网络直连（同账户设备靠账户派生密钥互相发现，零额外配置）。
- **配对**：`sync pair --username <名> --secret <密码>`（自建服务加 `--server <url>`，局域网加 `--lan-peer <名字>` 或先 `--lan-host true`，P2P 只要账户凭据）。不分注册与登录：账户不存在就当场创建。**用户名 ≥4 位、密码 ≥6 位**。密码派生加密密钥，**丢失 = 数据不可恢复**，别替用户编密码。
- **找主机**：`sync discover`（配对前的第一步，不需要数据目录已存在）。局域网方式把返回的 `name` 当 `--lan-peer`；自建服务方式把 `url` 当 `--server`。主机的身份是**名字**不是 ip:port（换 IP、端口上移都无感）；`--lan-peer` 也接受直接填 `ip` 或 `ip:port`。
- **日常**：`sync now` 立即同步；`sync status` 是**纯本地读**（不联网，可随时调）；`sync probe` 才真联网并刷新在线状态。判断通不通看 `status` 的 `online`（`null` = 还没探测过），要最新结论先 `probe`。凭据默认不输出，`sync status --show-secrets` 才带。
- **范围五选**：`sync configure --sync-data`（节点/任务/插图，默认开）/ `--sync-settings`（配置/配色/背景与头像，默认开）/ `--sync-schedules`（默认关）/ `--sync-diary`（默认开）/ `--sync-ledger`（默认开）。节奏：`--interval-seconds`（低于 5 按 5 生效，**这一项与其它设备共享**）与 `--reconnect-seconds`。改范围会自动全量重拉一次。
- **暂停 ≠ 解除**：`sync configure --enabled false` 暂停（地址与凭据全保留，`sync now` 报 `SYNC_PAUSED`），`--enabled true` 恢复；`sync unpair` 才清 token 与密码（对端数据保留）。
- **`sync peers`** 列出 P2P 目录里的在线设备与本机；报告里的 `peers` 是本轮真正对账过的名单 —— 「同步成功但数据没动」先看它。
- **排障速查**：`SYNC_HOST_NOT_RUNNING`（本机该当主机但内置服务器没起 —— **内置服务器由常驻的 GUI/APK 启停，只有 CLI 在跑时勾这个开关不会真的起服务**，让用户打开应用）｜`SYNC_LAN_HOST_NOT_SELECTED` / `SYNC_LAN_HOST_NOT_FOUND`｜`SYNC_P2P_NO_PEER`（目录里没有在线设备，**对方不在线是正常情况**，按 `--reconnect-seconds` 静默重试，不是故障）｜`SYNC_PAUSED`｜`SYNC_FEATURE_DISABLED`（用户在设置里关掉了同步总开关）｜`AUTH_FAILED`（密码错）。
  换过主机或它的库被重建时，客户端会自动清零水位全量重新对账并在报告里留一条 warning —— 某轮 `pushed` 突然等于本机全部实体数是**预期的重新播种**，不是故障。
- `sync history` 列出本机用过的配对信息（含密码，最多 8 条），`sync history --remove <下标>` 删一条。**给用户回填凭据前先问，别把密码写进日志或提交里。**

---

## 10. 典型流程

**「这周 / 这个月 / 今年做了些什么」**
1. `task tree` 定位相关条目（或 `task find --type entry --query "关键词"`）。
2. `task list --type item --changed-from <起> --changed-to <止> --all` 取这段时间创建/修改的。
3. 要某个条目的全量：`task list --type item --entry-id <id> --all`。
4. 同期日记：`diary list --from <起> --to <止>`；同期开销：`ledger stats --from <起> --to <止>`。
5. 汇总时注意 `--all`：不带就是前 50 条。

**「这个月花了多少 / 钱花在哪」**
1. `ledger stats --month <YYYY-MM>` 读 `totals` 与大类占比 `categories`（要趋势看 `series`）。
2. 需要明细再 `ledger list --from <月初> --to <月末>`（可加 `--category <名>` 只看一类）。
3. 年度用 `--year <YYYY>`；自定义区间用 `--from --to`（≤62 天给日粒度）。

**「记一笔」**
1. 不确定账户/分类叫什么 → `ledger accounts` 与 `ledger categories`（**先查再写，别凭猜建同名分类**）。
2. 向用户复述这笔的金额/账户/分类/日期，得到同意。
3. `ledger add ... --yes --idempotency-key <唯一键>`，读返回体里的 `signed` 与 `accountName` 回话。

**「导出备份 / 换机搬家」**
- 日记：`diary export --out diary.zip` → 新机 `diary import --zip diary.zip --yes`。
- 记账：`ledger export --out ledger.zip` → 新机 `ledger import --file ledger.zip --yes`（账户与分类跟着走）。
- 待办条目本身没有 CLI 导出（一般卡片的 Markdown 压缩包只在 GUI 的三点菜单里）；要跨机搬待办，正路是配好 `sync` 让两端自己收敛。
- 导入类命令**都不幂等**（重复导入产生重复数据），失败要看清 `skipped` 再决定，别直接重试。

---

## 11. 注意事项

- **不要重试写操作**，除非带了同一个 `--idempotency-key`；导入类命令重试会产生重复数据。
- **不要把范围词当关键词**：「本周」不是 `--query` 的值，用 `--changed-from/--changed-to`。
- **不要拼造 ID**，也不要按前缀推断类型。
- **不要替用户编同步密码**，也不要把密码写进日志、提交或回复里。
- **不要把 `--dry-run` 的结果当成已执行**：它只校验并展示影响。
- 汇总统计一律带 `--all`（task 默认每页 50）。
- 界面显示相关的设置（视图选择、字号、导航布局）多是**本机偏好、不跨设备同步**，改它不会影响别的设备，也不影响任何命令的输出。
- **这些能力只在 GUI 里，CLI 没有对应命令**（用户要就引导他到界面，别去找不存在的子命令）：工具箱里的「文件传输助手」（两台设备凭同一句 ≥8 位的口令互传文件与文件夹，打洞直连）与「草稿纸」、把工具固定进侧栏与拖动排序、临期高亮的四档配色、提醒的可视化编辑（CLI 这一侧只有 `task add/modify --reminder`）。
