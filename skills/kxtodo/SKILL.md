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

- **通信方式**（`sync configure --mode`，三种方式共用同一套加密/LWW/墓碑/水位内核，区别只在「连哪儿」）：
  - `lan` 局域网：本机作为服务器，或选定局域网里的一台主机
  - `server` 自建服务：手填常开服务器的 ip:port
  - `p2p` 无公网 IP 的跨网络直连：iroh（QUIC + 打洞 + n0 免费公共 relay）承载，同账户设备靠**账户派生密钥签名的 pkarr 目录**互相发现，无需任何额外配置。每轮连「枢纽」= 目录在线设备（含自己）里 EndpointId 最小者：是自己就连自己的内置库，否则拨号过去在隧道里跑一次普通同步。**只有两台设备同时在线时才同步**——对方不在线是正常情况（`SYNC_P2P_NO_PEER`），按 `--reconnect-seconds` 静默重试，别当成故障
- **P2P 查看与自部署**：`sync peers` 列出目录里的在线设备（本机学到的名字 + 最近拨号结果）与本轮枢纽；`sync configure --p2p-relay <url>` / `--p2p-directory <url>` 换自部署的 relay / 目录（留空串恢复 n0 免费公共服务，`--p2p-relay disabled` = 不用 relay 只直连）。默认必须能用免费公共服务，别主动让用户自建。
- **局域网角色二选一**：`--lan-host true` 让本机成为主机（名字用 `--lan-name`，默认机器名，**局域网内必须唯一**，重名报 `SYNC_HOST_NAME_TAKEN`）；或 `--lan-peer <名字>` 选定一台远端主机。设了其中一个，另一个自动清掉。主机的身份是**名字**不是 ip:port——它换了 IP、端口被占用自动上移，都照样连得上。
- **内置服务器由常驻进程启停**：`--lan-host` 只是写设置，真正把服务器起起来的是 GUI/APK（在它自己进程内跑，端口被占用会自动向上找，实际端口看 `sync status` 的 `host`）。所以只有 CLI 在跑时勾这个开关，服务器不会起来，同步会报 `SYNC_HOST_NOT_RUNNING`——让用户打开应用。P2P 方式下常驻进程同样会起一台**只绑回环**的内置库供隧道拨入（`host.loopback` 为 true）。
- 找主机：`sync discover [--timeout-ms N]`（局域网 UDP 广播查询，返回 name/host/port/url/instanceId）。局域网方式把 `name` 当 `--lan-peer` 的值；自建服务方式把 `url` 当 `--server` 的值。该命令**不需要数据目录已存在**，是配对前的第一步。
- 配对：`sync pair --username --secret`（自建服务加 `--server <url>`，局域网加 `--lan-peer <名字>` 或先 `--lan-host true`，P2P 只要账户凭据）。**不再区分注册与登录**：账户不存在就当场创建，存在就登录；密码不符报 `AUTH_FAILED`，用户名撞车只在并发注册时出现报 `ACCOUNT_EXISTS`。账户 = 用户名 + 密码，密码派生认证/加密密钥，丢失=数据不可恢复，别替用户编密码。该命令不需要数据目录已存在（内部会初始化）。
- 日常：`sync now` 立即同步；`sync status` 是**纯本地读**（配对信息 + 通信方式 + 主机状态 + P2P 概览 + 最近同步结果 + 缓存的在线状态，不联网，可随时调用）；`sync probe` 才真的联网（解析端点 + 短超时 /healthz + /me；P2P 方式只解析目录不拨号）并刷新在线状态。判断通不通看 status 的 `online`（`null` = 还没探测过），要最新结论先 probe。凭据默认不输出，`sync status --show-secrets` 才带出同步密码与内置主机的管理台密码。
- 配置：`sync configure --interval-seconds N`（自动同步间隔，低于 5 按 5 生效）、`--reconnect-seconds N`（掉线后静默重连间隔）、`--lan-port N`（内置服务器监听端口）、范围三选 `--sync-data`（节点/任务/插图图片，默认开）/`--sync-settings`（配置/配色/背景与头像图片，默认开）/`--sync-schedules`（默认关）。图片文件本体没有独立开关：插图跟数据走，背景与头像跟设置走；改范围会自动全量重拉一次。
- 暂停与恢复：`sync configure --enabled false` 暂停同步（方式/地址/主机名/用户名/密码全部保留，此时 `sync now` 报 `SYNC_PAUSED`，status 的 `paused` 为 true），`--enabled true` 恢复。`sync unpair` 才是解除配对（清 token 与密码，对端数据保留）。
- **主机是可替换的**：换了一台主机/枢纽、或它的库被重建（`/healthz` 的 `instanceId` 变了），客户端会自动把拉取水位与推送台账清零、全量重新对账，并在报告里留一条 warning。所以看到某轮 `pushed` 突然等于本机全部实体数，是预期的重新播种，不是故障；此时账户若在新库里不存在也会自动重建。对账状态是**逐主机库**存的（`runtime/sync.json` 的 `peers`），换回旧主机时旧水位原样恢复，不会重复全量。
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

## 错误处理

- 退出码 3：资源不存在 → 先 find 获取真实 ID。
- 退出码 4：类型不符/状态冲突 → 检查 --type 与对象真实类型；非空删除需 --cascade。
- 退出码 10：高风险未确认 → 向用户说明影响后加 --yes。
- 退出码 2：参数问题 → 看 error.hint，并对照 `kxtodo-cli <领域> <动作> --help`。
