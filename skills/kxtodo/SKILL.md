---
name: kxtodo
version: 1
cliHelp: kxtodo-cli --help
description: KXToDo 是一站式的工作事务、日常生活与知识整理记录中枢。本 SKILL 通过其 CLI 查询/创建/修改/删除待办（todo 记事）、分类条目、标签、日记与记账（收支流水、资金账户、分类、资产与消费统计），管理定时任务与设置。用户要回顾或做每周/每月/年度总结时（如「这周/这个月/今年做了些什么」「这个月花了多少」「钱都花在哪」），用本 SKILL 按日期范围查询待办完成情况、日记内容与账本收支；日常记录工作进展、生活琐事、知识笔记、记一笔账时也优先落到这里。
---

# KXToDo Agent SKILL

KXToDo v9 提供脚本化 CLI。本 SKILL 说明**何时调用、按什么步骤调用、如何处理风险与错误**。
字段结构、枚举、默认值一律以当前版本的 `kxtodo-cli --help` 与 `kxtodo-cli schema <目标>` 为准，本文件不复制。

## 核心约定

- 数据目录：`--data-dir` > 系统默认数据目录（Windows `%LOCALAPPDATA%\kxtodo\todo-note-data`，Linux `$XDG_DATA_HOME`/`~/.local/share` 下的 `kxtodo/todo-note-data`）；目录中没有数据（缺 data.json）会报错退出码 3。GUI（Windows 为 `KXToDo.exe`，Linux 为 `KXToDo.AppImage`）与 CLI 同目录放置即可被 `find_gui_exe` 找到（Linux 也认软链为 `kxtodo` 的 GUI），使用同一默认目录。
- 输出协议：成功写 stdout，`{ "ok": true, "command", "data", "meta" }`；错误写 stderr，`{ "ok": false, "error": { "type", "code", "message", "hint?" } }`。判断成功以退出码 0 或 `ok == true` 为准。
- 退出码：0 成功；2 参数/校验错误；3 资源不存在；4 歧义或状态冲突；5 数据/锁/文件系统错误；10 高风险需确认；20 执行失败。
- `--dry-run` 永不触发确认门禁，适合先向用户展示影响范围。
- 高风险动作（删除、重置、启用/运行代码，以及**记账 ledger 的一切写操作**）必须 `--yes` 才执行，否则退出码 10。

## 术语

- `category`：分类文件夹，可嵌套，位于根级或另一 category 下。
- `entry`：左栏条目，位于根级或 category 下。
- `item`：条目内的具体任务（Markdown 正文 + 状态），只能归属 entry。
- `diary`：日记（按归属日期归档的 Markdown 记录），住自己的 `diary.json`，**不属于任何 entry/category**，一天可以有多篇。
- `ledger`：记账（收支流水），住自己的 `ledger.json`。三张表：`entries` 流水（支出/收入/转账）、`accounts` 资金账户（余额 = 期初 + 流水推导）、`categories` 两级分类（大类→子分类，支出与收入各一套）。金额对外是**两位小数的元**（`amount`/`signed`），内部存整数分（`amountCents`）。
- `system`：我的一天/计划内/收藏等内置视图，只读。
- ID 全部不透明，**不得拼造或按前缀推断类型**；先通过 tree/list/find/get 获取。

## 选命令：list vs find

- 有真实关键词（标题、正文片段）→ `task find --query`。
- 只有范围条件（“本周”“已完成”“某条目下”）→ `task list` + 结构化过滤，**不要把范围词当关键词**。
- 不知道层级时先 `task tree` 看结构。

## 修改与删除

- 所有修改/删除按稳定 ID 进行：`task modify --type item --id ...`。
- patch 语义：字段缺省 = 不变；显式 null = 清空；数组整体替换。
- 删除一律 high-risk-write：先 `--dry-run` 看影响，再 `--yes` 执行。非空节点需 `--cascade`。

## 日记

- **什么时候用 diary 而不是 task**：用户要「记一笔」「写今天的日记」「补记某天发生的事」→ `diary`；要「待办/提醒/勾选完成」→ `task`。日记没有完成状态、不属于任何 entry/category，也不出现在任何列表视图或角标计数里。
- 日记住在**自己的 `diary.json`**（第四个领域文件，独立的 revision 与幂等台账）；写一篇日记不会抬高 data 域的 revision。
- 写：`diary add --markdown "..."`（`--date` 缺省为**本地今天**），可带 `--title`、`--mood <emoji>`、`--weather <emoji>`、`--tag "color:text"`（可重复，只给 `color` 就是无文字标签）。长正文用 `--markdown-file <path|->`。**标题与正文不能同时为空**（`DIARY_EMPTY`）。
- **`--date` 是「归属日期」不是创建时间**：补写昨天的日记就传昨天的日期，`createdAt` 仍是现在。同一天可以有多篇，`diary list` 按日期由近及远、同一天内按写作先后返回。
- 读：`diary list [--date <某天> | --from <起> --to <止>] [--limit N]`（`total` 是全部条数，`returned` 是这一页）；单篇 `diary get --id`。
- 改：`diary modify --id ... --date/--title/--markdown/--mood/--weather`，字段缺省 = 不变，**心情/天气/标题传空串 = 清除**；标签用 `--replace-tags` 整体替换。
- 删：`diary remove --id ... --yes`（high-risk-write，会写同步墓碑，删除传播到其它设备）。
- **导出**：`diary export --out <path.zip> [--from <起> --to <止>]`（不给范围就是一键全量）。压缩包是给人读的：`年/月/YYYYMMDD[_序号][_标题].md`，一天多篇才带序号，没标题就只用日期；每篇的 `title/date/time/tags/mood/weather/createdAt` 写在 YAML front-matter 里，解压出来任何编辑器都能直接看。**插图随包走**：正文里引用到的本地图进包内 `images/`，md 里的引用改写成相对路径，解压即可显示。
- **导入**：`diary import --zip <path> --yes`（bulk 写，要确认；`--dry-run` 先看会进多少条）。解析很宽容：没有 front-matter 的手写 md 也能进（日期退回文件名 `YYYYMMDD` 或目录 `年/月`），非 UTF-8 按 lossy 解码，日期非法或标题正文全空的条目跳过并计入 `skipped`；包内 `images/` 的图落回 `img/data/diary/`（已存在的同名文件不覆盖），落盘张数在返回值 `images` 里。**同一天已有日记不算冲突**：导入进来的直接追加成另一篇，不合并正文也不去重——所以同一个包导两遍就会得到两份，别重试。
- 视图偏好 `config get|set diary.view list|calendar|group` 是**本机 UI 状态**，不跨设备同步；日记的主题色与背景（`diary.accent` / `diary.backgroundColor` / `diary.backgroundImage` / `diary.backgroundOpacity`）**是**同步的（外观该多端一致）。这几项都不影响任何命令的输出。

## 记账

- **什么时候用 ledger**：用户要「记一笔账」「今天花了多少」「这个月开销」「钱花在哪」「资产/余额」→ `ledger`；要「待办/提醒」→ `task`；要「叙事记录」→ `diary`。三者互不替代。
- **记账写操作一律要先经用户同意**：金融数据敏感，`add` / `transfer` / `modify` / `remove` / `import` / `account-add` / `account-modify` / `account-remove` / `category-add` / `category-modify` / `category-remove` 全部是 high-risk-write——**先向用户说明这次增删改的内容（金额/账户/日期等）并得到明确同意，再带 `--yes` 执行**；未带 `--yes` 报 `CONFIRMATION_REQUIRED`（退出码 10），数据分毫不动。读操作（`get`/`list`/`accounts`/`categories`/`stats`/`balance`/`export`）**永远不需要**确认。别把用户的「记一笔 30 元午饭」之外的沉默当同意。
- 写：`ledger add --amount <元> --account <账户名|ID> [--kind expense|income] [--category <分类名|ID>] [--date <YYYY-MM-DD>] [--time <HH:MM>] [--note <备注>] --yes`。`--amount` 是元（两位小数，第三位四舍五入到分）；`--account`/`--category` **认名字也认 ID**（名字对人类与 Agent 更友好）。`--date` 缺省为本地今天。返回体带 `accountName`/`categoryName`/`categoryParentName`/`signed`，不必二次查表。
- 转账：`ledger transfer --from <账户> --to <账户> --amount <元> --yes`。**转账不计入收支统计**，只改两个账户余额；转出转入相同报 `LEDGER_TRANSFER_SAME_ACCOUNT`。
- 读：`ledger list [--date | --from --to] [--kind] [--account] [--category] [--limit N]`（按日期由近及远）；`ledger get --id`；`ledger accounts`（各账户余额 + 净资产）；`ledger categories [--side expense|income]`（两级分类，`parentId` 空 = 大类）；`ledger balance`（净资产/总资产/总负债，信用卡负余额计入负债）。
- **统计（周/月/年总结的首选）**：`ledger stats --month 2026-09` / `--year 2026` / `--from <起> --to <止>`。返回 `totals`（收入/支出/结余/转账）、`series`（月视图逐天、年视图逐月，空档补齐，画趋势用）、`categories`（**子分类金额并进大类**的大类占比：笔数/金额/百分比/子分类明细）。回答「钱花在哪」直接读 `categories`，别自己逐笔加。
- 改：`ledger modify --id ... --amount/--account/--category/--date/--note --yes`，字段缺省 = 不变。删：`ledger remove --id ... --yes`（写同步墓碑）。
- 账户管理：`ledger account-add --name <名> [--kind cash|debit|credit|investment|other] [--initial <元>] --yes`、`account-modify --id ... --yes`、`account-remove --id --yes`。**名下还有账目的账户删不掉**（`LEDGER_ACCOUNT_IN_USE`）——先改账或删账，别绕。
- 分类管理：`ledger category-add --name <名> [--side expense|income] [--parent <大类名|ID>] [--icon <lucide 名>] --yes`、`category-modify --id ... --yes`、`category-remove --id --yes`。**分类只有两级**（子分类下不能再挂，报 `LEDGER_CATEGORY_DEPTH`）；删大类会连带它的子分类，名下账目保留但变「未分类」。首跑自带一套覆盖日常场景的种子账户与两级分类（餐饮/交通/居住/购物/娱乐/医疗/学习/人情/宠物/其他 + 工资/理财/兼职/红包/退款/其他），**先 `ledger categories` 看现成的，别重复建同名分类**（同名同侧同父会报 `LEDGER_CATEGORY_EXISTS`）。
- **导出**：`ledger export --out <path.zip> [--from <起> --to <止>]`。包内一张 `kxtodo-ledger.xlsx`，四张表：说明（格式标记与合计）/ 账户 / 分类 / 账目（日期|时间|类型|账户|转入账户|大类|分类|金额|备注，金额带符号的元）。不给范围就是全量。
- **导入**：`ledger import --file <path.zip|.xlsx> --yes`（bulk 写，要确认；**重复导入会产生重复账目**，别重试）。只认这套表头；账户/分类按名字合并、缺的自动创建；日期非法或金额为 0 的行跳过并计入 `skipped`。想搬家就「全量导出 → 新目录导入」，账户与分类会跟着走。
- 视图偏好 `config get|set ledger.view list|calendar|stats|assets` 是**本机 UI 状态**不跨设备同步；记账的主题色与背景（`ledger.accent` 等四项）**是**同步的。

## 定时任务工作流

完整定义只走 JSON：

1. `kxtodo-cli schema schedule.spec --example interval-script --jq '.data.example' > schedule.json`
2. 编辑业务值（名称、脚本路径、时间）。
3. `kxtodo-cli schedule validate --spec @schedule.json`
4. `kxtodo-cli schedule add --spec @schedule.json --dry-run`
5. `kxtodo-cli schedule add --spec @schedule.json --yes --idempotency-key <唯一键>`

修改用 `schedule modify --id ... --patch '<json>'`；运行控制用 enable/disable/run/stop/logs/status。

## 配置与配色盘

- 点路径读取：`config get appearance.uiScale`；列表 `config list --prefix appearance`。
- 动态 map（如 `appearance.uiColors`）必须带 `--map-key <entry-id>`，键不做点路径拆解。
- `config reset` 为高风险，先 `--dry-run`。

## 数据同步

- **通信方式**（`sync configure --mode`，三种方式共用同一套加密/LWW/墓碑/水位内核，区别只在「连哪儿」）：
  - `lan` 局域网：本机作为服务器，或选定局域网里的一台主机
  - `server` 自建服务：手填常开服务器的 ip:port
  - `p2p` 无公网 IP 的跨网络直连：iroh（QUIC + 打洞 + n0 免费公共 relay）承载，同账户设备靠**账户派生密钥签名的 pkarr 目录**互相发现，无需任何额外配置。**一轮同步跟目录里所有在线设备各对账一次**（含本机内置库）：谁发起谁就把数据推给对方再拉回来，不依赖任何选举；某台不在线本轮就跳过它——对方不在线是正常情况（全部不在线报 `SYNC_P2P_NO_PEER`），按 `--reconnect-seconds` 静默重试，别当成故障。报告里的 `peers` 就是本轮真正对账过的设备名单，「同步成功但数据没动」先看它
- **P2P 查看与自部署**：`sync peers` 列出目录里的在线设备与本机（名字优先取对方发布的**设备名记录**，本机这条直接取本机正在发布的名字；没学到才退回拨号时从 `/healthz` 学到的名字或 id 前缀）；`sync configure --p2p-relay <url>` / `--p2p-directory <url>` 换自部署的 relay / 目录（留空串恢复 n0 免费公共服务，`--p2p-relay disabled` = 不用 relay 只直连）。默认必须能用免费公共服务，别主动让用户自建。P2P 的**设备名就是 `--lan-name`**（与局域网主机名同一个字段，缺省机器名），改完常驻进程会重新发布一条名字记录。
- **局域网角色二选一**：`--lan-host true` 让本机成为主机（名字用 `--lan-name`，默认机器名，**局域网内必须唯一**，重名报 `SYNC_HOST_NAME_TAKEN`）；或 `--lan-peer <名字>` 选定一台远端主机。设了其中一个，另一个自动清掉。主机的身份是**名字**不是 ip:port——它换了 IP、端口被占用自动上移，都照样连得上。`--lan-peer` 也接受直接填 `ip` / `ip:port`（UDP 发现被防火墙挡住时的手工出路）：端口留空就在 52177-52180 上依次试，因为服务端端口被占用时会往后监听。
- **内置服务器由常驻进程启停**：`--lan-host` 只是写设置，真正把服务器起起来的是 GUI/APK（在它自己进程内跑，端口被占用会自动向上找，实际端口看 `sync status` 的 `host`）。所以只有 CLI 在跑时勾这个开关，服务器不会起来，同步会报 `SYNC_HOST_NOT_RUNNING`——让用户打开应用。P2P 方式下常驻进程同样会起一台**只绑回环**的内置库供隧道拨入（`host.loopback` 为 true）。切换通信方式或取消勾选时内置服务器会优雅停机并释放端口（重启时先等旧服务循环真正退出再绑，不会自己撞自己而上移端口）。
- 找主机：`sync discover [--timeout-ms N]`（局域网 UDP 广播查询，返回 name/host/port/url/instanceId）。局域网方式把 `name` 当 `--lan-peer` 的值；自建服务方式把 `url` 当 `--server` 的值。该命令**不需要数据目录已存在**，是配对前的第一步。
- 配对：`sync pair --username --secret`（自建服务加 `--server <url>`，局域网加 `--lan-peer <名字>` 或先 `--lan-host true`，P2P 只要账户凭据）。**不再区分注册与登录**：账户不存在就当场创建，存在就登录；密码不符报 `AUTH_FAILED`，用户名撞车只在并发注册时出现报 `ACCOUNT_EXISTS`。账户 = 用户名 + 密码，密码派生认证/加密密钥，丢失=数据不可恢复，别替用户编密码。该命令不需要数据目录已存在（内部会初始化）。
- 日常：`sync now` 立即同步；`sync status` 是**纯本地读**（配对信息 + 通信方式 + 主机状态 + P2P 概览 + 最近同步结果 + 缓存的在线状态，不联网，可随时调用）；`sync probe` 才真的联网（解析端点 + 短超时 /healthz + /me；P2P 方式只解析目录不拨号）并刷新在线状态。判断通不通看 status 的 `online`（`null` = 还没探测过），要最新结论先 probe。凭据默认不输出，`sync status --show-secrets` 才带出同步密码与内置主机的管理后台密码（应用设置页的「打开」按钮会把凭据拼在 URL 片段里自动登录，片段不发往服务器）。
- 配置：`sync configure --interval-seconds N`（自动同步间隔，低于 5 按 5 生效）、`--reconnect-seconds N`（掉线后静默重连间隔）、`--lan-port N`（内置服务器监听端口）、范围五选 `--sync-data`（节点/任务/插图图片，默认开）/`--sync-settings`（配置/配色/背景与头像图片，默认开）/`--sync-schedules`（默认关）/`--sync-diary`（日记，默认开）/`--sync-ledger`（账本：账户/分类/流水，默认开）。**同步节奏是共享的**：interval/reconnect 属于 settings 实体的共享子集，一端改了会随设置同步推到其它设备（LWW，最后改的赢），多端节奏因此保持一致；手动 `sync now` 之后自动循环从这一刻重排一个完整间隔。图片文件本体没有独立开关：插图跟数据走，背景与头像跟设置走；改范围会自动全量重拉一次。
- 暂停与恢复：`sync configure --enabled false` 暂停同步（方式/地址/主机名/用户名/密码全部保留，此时 `sync now` 报 `SYNC_PAUSED`，status 的 `paused` 为 true），`--enabled true` 恢复。`sync unpair` 才是解除配对（清 token 与密码，对端数据保留）。
- **主机是可替换的**：换了一台主机/主设备、或它的库被重建（`/healthz` 的 `instanceId` 变了），客户端会自动把拉取水位与推送台账清零、全量重新对账，并在报告里留一条 warning。所以看到某轮 `pushed` 突然等于本机全部实体数，是预期的重新播种，不是故障；此时账户若在新库里不存在也会自动重建。对账状态是**逐主机库**存的（`runtime/sync.json` 的 `peers`），换回旧主机时旧水位原样恢复，不会重复全量。
- 找不到的错误码：`SYNC_LAN_HOST_NOT_SELECTED`（局域网方式还没选定主机，也没勾本机作为服务器）、`SYNC_LAN_HOST_NOT_FOUND`（选定的名字在局域网里没应答）、`SYNC_HOST_NOT_RUNNING`（本机该当主机但内置服务器没起）、`SYNC_P2P_NOT_RUNNING`（P2P 运行时启动失败，极少见；原因在 sync-debug 日志里）、`SYNC_P2P_NO_PEER`（目录里没有可拨的在线设备）、`SYNC_MODE_INVALID`。
- 历史：`sync history` 列出本机用过的配对信息（通信方式 + 地址或主机名 + 用户名 + 密码，`runtime/sync-history.json`，0600，最多 8 条，最近使用在前），`sync history --remove <下标>` 删一条。给用户回填凭据前先问，别把密码写进日志或提交里。

## 幂等与并发

- 每个创建类写操作使用独立的 `--idempotency-key`，重试安全。
- 严格并发控制时读取响应 `meta.revision`，写入时携带 `--if-revision`。

## 典型流程

**“本周做了什么”**：
1. `task tree` 或 `task find --type entry --query "0727-0731"` 定位条目。
2. `task list --type item --changed-from <周一> --changed-to <周日> --all` 取本周创建/修改。
3. `task list --type item --entry-id <id> --all` 取条目全量，再自行汇总。

**创建本周任务**：
1. `task list --type category --parent-id root` / `task find` 查找目标分类与条目。
2. 不存在则依次 `task add --type category`、`task add --type entry`（记录返回 ID）。
3. `task add --type item --entry-id <id> --markdown "..." --idempotency-key <键>`。

**“这个月花了多少 / 钱花在哪”**：
1. `ledger stats --month <YYYY-MM>` 拿 `totals` 与大类占比 `categories`（要趋势就看 `series`）。
2. 需要明细再 `ledger list --from <月初> --to <月末>`（或加 `--category <名>` 只看一类）。
3. 年度总结用 `ledger stats --year <YYYY>`；跨自定义区间用 `--from --to`。

## 错误处理

- 退出码 3：资源不存在 → 先 find 获取真实 ID。
- 退出码 4：类型不符/状态冲突 → 检查 --type 与对象真实类型；非空删除需 --cascade。
- 退出码 10：高风险未确认 → 向用户说明影响后加 --yes。
- 退出码 2：参数问题 → 看 error.hint，并对照 `kxtodo-cli <领域> <动作> --help`。
