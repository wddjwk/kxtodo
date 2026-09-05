---
name: kxtodo
version: 1
cliHelp: kxtodo-cli --help
description: 通过 KXToDo CLI 查询、创建、修改、删除待办数据，管理定时任务与设置。
---

# KXToDo Agent SKILL

KXToDo v9 提供脚本化 CLI。本 SKILL 说明**何时调用、按什么步骤调用、如何处理风险与错误**。
字段结构、枚举、默认值一律以当前版本的 `kxtodo-cli --help` 与 `kxtodo-cli schema <目标>` 为准，本文件不复制。

## 核心约定

- 数据目录：`--data-dir` > 系统默认数据目录（Windows `%LOCALAPPDATA%\kxtodo\todo-note-data`，Linux `$XDG_DATA_HOME`/`~/.local/share` 下的 `kxtodo/todo-note-data`）；目录中没有数据（缺 data.json）会报错退出码 3。GUI（Windows 为 `KXToDo.exe`，Linux 为 `KXToDo.AppImage`）与 CLI 同目录放置即可被 `find_gui_exe` 找到（Linux 也认软链为 `kxtodo` 的 GUI），使用同一默认目录。
- 输出协议：成功写 stdout，`{ "ok": true, "command", "data", "meta" }`；错误写 stderr，`{ "ok": false, "error": { "type", "code", "message", "hint?" } }`。判断成功以退出码 0 或 `ok == true` 为准。
- 退出码：0 成功；2 参数/校验错误；3 资源不存在；4 歧义或状态冲突；5 数据/锁/文件系统错误；10 高风险需确认；20 执行失败。
- `--dry-run` 永不触发确认门禁，适合先向用户展示影响范围。
- 高风险动作（删除、重置、启用/运行代码）必须 `--yes` 才执行，否则退出码 10。

## 术语

- `category`：分类文件夹，可嵌套，位于根级或另一 category 下。
- `entry`：左栏条目，位于根级或 category 下。
- `item`：条目内的具体任务（Markdown 正文 + 状态），只能归属 entry。
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

- 找服务器：`sync discover`（局域网 UDP 广播查询，返回 name/host/port/url，`url` 可直接当 `--server`）。该命令**不需要数据目录已存在**，是配对前的第一步。
- 配对：新账户 `sync register --server <url> --username --secret`；已有账户换设备用 `sync login`（同样三个参数）。账户 = **用户名 + 密码**，密码派生认证/加密密钥，丢失=数据不可恢复，别替用户编密码。
- 日常：`sync now` 立即同步；`sync status` 是**纯本地读**（配对信息 + 最近同步结果 + 缓存的在线状态，不联网，可随时调用）；`sync probe` 才真的联网探测（短超时 /healthz + /me）并刷新在线状态。判断服务器通不通看 status 的 `online` 字段（`null` = 还没探测过），要最新结论先 probe。
- 配置：`sync configure --interval-seconds N`（自动同步间隔，低于 5 按 5 生效）、`--reconnect-seconds N`（掉线后静默重连间隔）、范围三选 `--sync-data`（节点/任务/插图图片，默认开）/`--sync-settings`（配置/配色/背景与头像图片，默认开）/`--sync-schedules`（默认关）。图片文件本体没有独立开关：插图跟数据走，背景与头像跟设置走；改范围会自动全量重拉一次。
- 暂停与恢复：`sync configure --enabled false` 暂停同步（服务器地址/用户名/密码全部保留，此时 `sync now` 报 `SYNC_PAUSED`，status 的 `paused` 为 true），`--enabled true` 恢复。`sync unpair` 才是解除配对（清 token 与密码，服务器数据保留）。
- 历史：`sync history` 列出本机用过的「服务器地址 + 用户名 + 密码」（`runtime/sync-history.json`，0600，最多 8 条，最近使用在前），`sync history --remove <下标>` 删一条。给用户回填凭据前先问，别把密码写进日志或提交里。

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

## 错误处理

- 退出码 3：资源不存在 → 先 find 获取真实 ID。
- 退出码 4：类型不符/状态冲突 → 检查 --type 与对象真实类型；非空删除需 --cascade。
- 退出码 10：高风险未确认 → 向用户说明影响后加 --yes。
- 退出码 2：参数问题 → 看 error.hint，并对照 `kxtodo-cli <领域> <动作> --help`。
