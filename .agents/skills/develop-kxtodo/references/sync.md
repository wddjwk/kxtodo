# 同步专项：安全模型 / 实体与范围 / 传输分层 / 内置主机 / P2P / 图片 / server 运维

> **这份文件是什么**：数据多端同步的**全部**细节——安全模型、同步语义、实体与范围、配对、自动同步节奏、局域网发现、图片 blob 通道、掉线处理、传输分层（transport / endpoint / engine / merge / crypto）、内置主机、instance epoch、端口生命周期、P2P、server 运维与管理控制台、同步功能总开关。
> **什么时候读它**：改 `crates/core/src/sync/` 任何模块、改 `crates/server/`、改设置页「数据同步」卡片、排查「同步成功但数据没动」/「409 一路重试」/「换主机拉不到东西」、加一类要同步的实体、部署 kxtodo-server。
> **分层图**（设备 ↔ 主机 ↔ SQLite）见 `architecture.md` 的「同步分层（一图）」。
> **改「连哪儿」不许动 merge/crypto**——这是分层的全部意义，见 `invariants.md`。

## 目录

- [安全模型（Etebase 式）](#安全模型etebase-式)
- [同步语义](#同步语义)
- [实体与范围](#实体与范围)
- [配对流程](#配对流程)
- [踩坑记录](#踩坑记录)
- [跨机验证记录（v0.4.0，已通过）](#跨机验证记录v040已通过)
- [server 运维（v0.4.1）](#server-运维v041)
- [自动同步（v0.4.1，已被 v0.5.0 重写）](#自动同步v041已被-v050-重写)
- [局域网自动发现（v0.5.0）](#局域网自动发现v050)
- [图片同步（v0.5.0）](#图片同步v050)
- [自动同步真正生效（v0.5.0，修的是 v0.4.1 的死代码）](#自动同步真正生效v050修的是-v041-的死代码)
- [掉线处理与 UI 不阻塞（v0.5.0）](#掉线处理与-ui-不阻塞v050)
- [server 运维（v0.5.0 追加）](#server-运维v050-追加)
- [账户模型简化（v0.5.1）](#账户模型简化v051)
- [同步范围三勾选框（v0.5.1）](#同步范围三勾选框v051)
- [暂停/恢复同步（v0.5.1）](#暂停恢复同步v051)
- [配对历史（v0.5.1，v0.6.0 扩展）](#配对历史v051v060-扩展)
- [设置面板同步卡片重排（v0.5.1 → v0.6.2）](#设置面板同步卡片重排v051--v062)
- [管理控制台重做（v0.5.1）](#管理控制台重做v051)
- [传输分层（v0.6.0，v0.6.1 补完 P2P）](#传输分层v060v061-补完-p2p)
- [内置主机（v0.6.0「本机作为服务器」）](#内置主机v060本机作为服务器)
- [主机身份是名字（v0.6.0）](#主机身份是名字v060)
- [runtime/sync-host.json（v0.6.0）](#runtimesync-hostjsonv060)
- [instance epoch（v0.6.0，换主机/库重建的自愈）](#instance-epochv060换主机库重建的自愈)
- [统一 pair（v0.6.0）](#统一-pairv060)
- [端口生命周期（v0.6.0 上移，v0.6.2 清理）](#端口生命周期v060-上移v062-清理)
- [P2P 同步（v0.6.1 落地，v0.6.2 补名字，v0.6.3 改逐台对账）](#p2p-同步v061-落地v062-补名字v063-改逐台对账)
- [同步功能总开关（v0.6.10，`features.sync`，默认开）](#同步功能总开关v0610featuressync默认开)
- [设置共享子集的三处清单必须一致（v0.8.3 修的两个洞）](#设置共享子集的三处清单必须一致v083-修的两个洞)
- [文件传输助手复用 P2P 那套（v0.8.3，`transfer.rs`）](#文件传输助手复用-p2p-那套v083transferrs)
- [相关但住在别处的同步内容](#相关但住在别处的同步内容)

## 同步总纲

（原 AGENTS.md 的这一章标题原文是：`### 数据多端同步（v0.4.0，crates/server + core sync 模块）`。下面是该章的**全部条目，保持原序**；每条从列表项提升为小节，文字一字未改。章首那张分层图搬到了 `architecture.md`「同步分层（一图）」，「数据地基（同步前置，schema v6）」搬到了 `architecture.md`「五个领域文件与数据地基」，「发布：kxtodo-server 双平台发布」与「更新下载多通道测速选路」搬到了 `build-and-release.md`。）

### 安全模型（Etebase 式）

`Argon2id(secret, 确定性盐=kxtodo|username)` 派生 master，HKDF 分离 `auth_key`（登录证明，HMAC 挑战应答，服务器持有同值）与 `enc_key`（加密密钥，永不出设备）。抓包/拖库只见密文；纯 HTTP 的残留风险仅 token 被嗅探后**冒写密文**（无明文泄露）。密钥丢失=数据不可恢复。
（**盐的构成变过一次**：v0.5.1 之前是 `kxtodo|username|email`，账户模型简化后 email 整个删掉，见下面「账户模型简化」那条。旧文档里若还看到 `username|email` 是没跟上的旧文。）

### 同步语义

逐实体 LWW（比较键 `(updatedAt, updatedBy=deviceId)`）+ 墓碑传播删除（`_meta.tombstones`，本地更新可复活）。服务器只做密文保管员 + OCC 版本号（PUT 带 base，409 客户端自愈重试：拉单实体重合并，本地仍胜则重推）。合并永远在客户端。

### 实体与范围

node（含背景）/task/**diary（日记）**/**ledger（记账：流水 `ledger` + 账户 `ledgerAccount` + 分类 `ledgerCategory` + 自定义账户类型 `ledgerAccountType`（v0.7.5）四种实体_kind_ 共用一个勾选框）**/schedule(仅 spec+enabled)/settings(共享子集：profile/配色/特性开关/**日记与记账的主题色与背景**；lifecycle/shortcuts/字号/sync 配置/日记与记账的视图选择均为本机偏好)。v0.7.0 起同步内容是**五个勾选框**：数据/设置/任务/日记/账本（`Scopes` 五个 bool，scopeSignature 五段，`sync configure --sync-diary/--sync-ledger`；日记与账本默认开——日记此前搭 `scopes.data` 的车，默认开是为了升级上来的用户行为不变）。diary 与 ledger 各住**自己的领域文件**（`diary.json` / `ledger.json`），engine 里各是一个独立合并事务（拉取分桶 / `repo.write_*` / 对账水位 / `resolve_conflict` 各一处 kind 判定），全新设备的推送抑制按各自文件存不存在单独判。**sync configure 开启 scope 会重置拉取水位全量重拉**（增量流曾按旧 scope 过滤）。**记账种子 id 必须确定性**（`lacc-01`/`lcat-exp-01-01` 这类，core 的 `LedgerFile::seed_defaults` 与前端 `defaults.ts::seedLedgerBook` 同一套）：账本没落盘时 `load_ledger` 会在内存里种一份供记第一笔账按名字解析账户/分类，随机 id 会让「加载两次 = 两本不同的账」，流水指向不存在的账户。

### 配对流程

v0.6.0 起 `sync register` / `sync login` 合并为一个 `sync pair`（账户不存在当场注册，存在就登录；全新设备先播种默认数据，首拉直接落服务端内容，**不与本地默认数据并集**）→ `sync now` / `sync status` / `sync configure` / `sync unpair`（清 token 关同步并清密码，服务器数据保留）。同步互斥锁 `runtime/sync.lock`（CLI/Host 并发只有一个同步在跑）。

### 踩坑记录

①拉取水位更新必须在合并事务**之后**读最新文件对账（早前先更新水位导致刚拉下的实体被误判脏又推回，服务端 seq 暴涨）；②PowerShell 5.1 跑 CLI 测试脚本要 `[Console]::OutputEncoding=UTF8`，否则 GBK 解码 UTF-8 JSON 炸；③WSL 镜像网络下 Windows↔WSL 的 localhost/LAN 直连默认被 Hyper-V 防火墙拦（需管理员开规则），跨机验证用真实 LAN 机器；④**Windows CLI 中文参数乱码**（代码页 936）：`std::env::args()` 按系统 ANSI 代码页解码命令行，中文必乱码——cli 入口已改 `args_os` + `embed-manifest` crate 嵌入进程级 UTF-8 代码页 manifest（手写 `/MANIFESTINPUT` 链接参数会造成 side-by-side 启动错误，勿回退）；⑤PowerShell 5.1 脚本文件本身必须带 UTF-8 BOM 否则中文按 GBK 解析直接语法错误；⑥**wmi 版本钉死**：iroh→netwatch→wmi，而 wmi 0.18.2+ 自己把 `windows` 与 `windows-core` 的版本范围写错配（一个 0.61 一个 0.62），Windows 上必编不过（`IWbemObjectSink: windows_core::Interface` 不满足）——`Cargo.toml.lock` 里钉 `wmi 0.18.1`（两个范围都是 ^0.62，内部自洽）。升级 iroh 或重新生成 lock 后若见这类错，先 `cargo update -p wmi --precise 0.18.1`；⑦iroh 1.1 要求 rust-version 1.91：四个 manifest 的 `rust-version` 已抬到 1.91（自己的 edition 仍 2021，依赖的 edition 2024 不影响我们），本地与 CI 都是 stable 无碍。

### 跨机验证记录（v0.4.0，已通过）

WSL 客户端 + Windows 客户端 + kklaptop Ubuntu 客户端 三端连同一 kklaptop server（真实 LAN HTTP），双向建/改/删/中文任务全部收敛一致。

### server 运维（v0.4.1）

启动参数持久化在 `server/settings.json`（listen/db/adminUser/密码哈希；显式指定则覆盖，未指定则沿用，首次必须给 `--admin-user/--admin-password`）；日志按日轮转留 7 天，**v0.5.1 起分级**：持久化文件只写关键操作（注册/登录成败/实体与图片的真实增改/管理动作/启动配置），每请求访问行与 UDP 发现应答只进 stdout（5 秒一轮的自动同步若全落盘一天就是十几万行噪音）；管理界面 `http://host:port/admin`（v0.5.1 重做为单页控制台，见下）；登录 token 有效期固定 30 天（`--token-ttl-days` 已删除）。

### 自动同步（v0.4.1，已被 v0.5.0 重写）

`sync.intervalSeconds`（默认 30，5-86400）取代旧 intervalMinutes；前端 `syncRunner.ts` 由 App onMount 启动（全平台含 Android，浏览器预览不启动），周期 pull+push，设置变化自动重排定时器。同步面板已去掉 caps.desktop 门控（移动端可用），配对表单默认填 displayName；面板/启动日志不再展示任何加密安全口号。

### 局域网自动发现（v0.5.0）

客户端 `core/src/sync/discovery.rs` + 服务端 `server/src/discovery.rs`。客户端在固定 UDP 52177 上发两轮广播（255.255.255.255）+ 组播（239.255.77.52）查询，收**单播**应答 `{protocol,name,port,version,instanceId}`，再用 3 秒超时 `/healthz` 复核拿权威 name/version/instanceId，host 取应答报文的源 IP。服务端单播应答是刻意的：Android 收组播要 MulticastLock（额外权限 + 唤醒 Wi-Fi 芯片），收单播不用。**发现端口与 TCP 端口解耦**（应答里带真实 TCP 端口），改了 `--listen` 端口照样能被发现；前提是非回环监听 + UDP 52177 未被同机其它进程占用（占用则启动日志降级提示，客户端只能手填 ip:port）。**v0.6.0 起的用法**：局域网方式里「发现」用来**按名字选定主机**（`sync.lanPeer`），应答里的 instanceId 供 epoch 判断；自建服务方式才把应答的 url 当 `--server` 填。入口：设置 → 数据同步 → 局域网主机行的「发现」按钮（下拉浮层点选）；CLI `sync discover [--timeout-ms]`——该命令**不要求数据目录已存在**（它是配对之前的动作，`command_needs_data` 里显式放行）。广播本身失败（容器禁 UDP 之类）按「没发现」处理并报错 `SYNC_LAN_HOST_NOT_FOUND`（Io 类，走静默重连），别把 socket 错误抛给用户。

### 图片同步（v0.5.0）

`core/src/sync/images.rs` + 服务端 `images` 表（密文存 SQLite BLOB）。三类文件本体一起同步：`img/data/<nodeId>/`（markdown 插图）、`img/background/`（列表背景）、`img/avator/`（头像，历史拼写别改）。图片是**内容寻址的不可变 blob**：ID = `sha256(kind|nodeId|filename)`（跨设备确定性一致且不含路径分隔符，可直接进 URL），无 LWW/OCC，靠明文 sha256 幂等——同名同内容不重传，服务端只存一份；命名冲突由文件名本身（`md-<纳秒>-<进程内计数器>.<ext>`）挡住。传输用裸字节体（不做 base64 膨胀），元数据走 query 参数（自写百分号编码），nonce 走响应头 `x-kxtodo-nonce`。实体流与图片流共用服务端 `users.current_seq` 计数器，但客户端各记水位（`lastPulledSeq` / `lastPulledImageSeq`，都单调安全）。删除**不**传播（孤儿图片留给后续的「清理无引用图片」功能）。落盘一律 `.part` + rename 原子写；服务端给的 nodeId/filename 会参与本地路径拼接，**两侧都做穿越校验**（`..`/分隔符/控制字符/前导点/超长一律拒）。本机清单的内容哈希按 (mtime,size) 缓存，否则 5 秒一轮的自动同步会反复重读所有图片。图片没有独立开关（v0.5.1）：插图（entry）跟随「同步数据」范围，背景/头像跟随「同步设置」范围；`runtime/sync.json` 记 `scopeSignature`（v0.5.0 时是三段 data|settings|schedules；**v0.7.0 起是五段**，日记与账本各占一段，见「实体与范围」），签名一变两条水位归零全量重拉——增量流是按范围过滤的，不重置水位历史实体就永远拉不回来。**滚动升级兼容**：老服务器（< v0.5.0）没有图片路由，客户端识别 `SYNC_HTTP_404/405/501` 后降级成一条 warning，数据同步照常——否则用户一升级客户端整条同步就断了。

### 自动同步真正生效（v0.5.0，修的是 v0.4.1 的死代码）

旧 `startAutoSync()` 第一行是 `if (!coreMode || timer !== null) return;`，而 App onMount 调它时 `hydrate()` 还没跑完、`coreMode` 恒为 false，于是**定时器从来没建立过**，间隔设成多少都没用（用户只能两端各自手点「立即同步」）。现在：入口不再用 coreMode 门控，改订阅 `isHydrated`，水合完成后启动并**立即同步一次**（初次连接）；用递归 `setTimeout` 而非 `setInterval`（跑完再排下一轮，同步耗时超过间隔不会堆积并发，也便于在线/掉线两种节奏切换）；间隔低于 5 秒一律按 5 秒生效（前端 normalize + core configure 双层夹取）；自动同步**不发通知**（周期弹通知很烦人，结果只在设置页「最近同步」里体现）；页面从后台回前台且距上次同步超过一个间隔时补一次（Android 会节流后台定时器）；手动「立即同步」/下拉刷新撞上 `SYNC_IN_PROGRESS` **不再自己插一轮**（v0.6.7）：`dispatchSyncNow` 改为等正在进行的那轮跑完（轮询 `sync.status`，看 `lastSyncAt` 变化），把它落盘的 `lastResult` 当本轮结果报出去——下拉撞自动循环那轮时再插一轮是纯浪费，把「正在同步」报成「同步失败」更是误导（安卓下拉体感最差）；等的那轮自己失败（`lastError` 换新）就按它的错误报；等满 50 秒还没收尾才给一句中性的「同步正在进行中，完成后可再试」，只有真正的连接故障才走 `report(error, "同步失败")`。**v0.5.1 节奏校准**：下一轮 = **本轮开始时间 + 间隔**（绝对截止时间，不是「跑完再等一个间隔」，否则周期 = 间隔 + 同步耗时逐轮漂移）；排程时间写进 `nextSyncAt` store，设置面板显示「下次同步 Ns」倒计时，节奏对不对一眼可见；改间隔/暂停/恢复**实时生效**（已过期立即补一次）。**坑**：`sync configure/register/login/unpair` 必须 `emit_domain_event(Domain::Settings)`，否则用 CLI 改间隔时正在运行的 GUI 前端永远等不到 appSettings 变化（GUI 自己面板的路径靠显式 refreshFromCore 兜着，跨客户端就漏了）。暂停 = `sync.enabled=false`（配对信息全保留，`sync now` 报 `SYNC_PAUSED`、退出码 4），解除配对才会清密码；`sync status` 的 `paused` 字段据此给出。

### 掉线处理与 UI 不阻塞（v0.5.0）

① `core_dispatch` / `core_snapshot` 从同步命令改成 **async + `tauri::async_runtime::spawn_blocking`**——Tauri 的同步命令跑在主线程上，一次 30 秒超时的同步请求或一次 Argon2id 派生（约 1 秒）就能把整个窗口和 IPC 卡死，这正是「服务器掉线时打开设置界面卡住几秒」的根因；② `sync status` 变成**纯本地读**（settings + `runtime/sync.json` 缓存），网络探测独立成 `sync probe`（3 秒超时 `/healthz` + `/me`），面板打开时后台调用；③ 在线结论缓存在 `runtime/sync.json`（`serverOnline`/`lastSeenAt`/`lastError`），面板据此显示「服务器（🟢已连接 / 🔴已掉线 / 未探测）」，前端 store 是 `syncConnection`；④ 掉线后按 `sync.reconnectSeconds`（默认 300，5-86400）静默重连，恢复即立刻同步，全程不打扰用户；⑤ `derive_keys` 加了进程内缓存（键 = `sha256(salt|secret)`，不落明文），5 秒一轮的自动同步不再每次白烧约 1 秒 Argon2id（移动端发热/耗电的关键）。

### server 运维（v0.5.0 追加）

`--name`（展示名，发现列表里显示；缺省用主机名 COMPUTERNAME/HOSTNAME，再退 `kxtodo-server`）；默认监听改成 `0.0.0.0:52177`（`--listen` 允许只给 ip 或只给端口，省略端口时用 52177；纯数字仍按端口理解，兼容旧写法）；`--data-dir` 指定 settings.json/log/server.pid 的根目录——**e2e 测试必须传**，否则 spawn 的真实 server 会覆盖开发机/生产机 `%LOCALAPPDATA%\kxtodo\server\settings.json` 里的管理员凭据；`--daemon` 后台静默运行（Linux `process_group(0)` + `Stdio::null`，Windows `CREATE_NO_WINDOW|CREATE_NEW_PROCESS_GROUP`；父进程打印 pid 后等 pidfile 就绪才退出，起不来会告警退出码 4）+ `--stop`（先确认 pid 真属于 kxtodo-server 再动手：Windows `tasklist`、Linux `/proc/<pid>/comm`，防过期 pidfile 误杀）；`/healthz` 返回 name；`me` 与 admin 概览含图片计数/体积；请求体上限提到 128MB（图片密文）。

### 账户模型简化（v0.5.1）

账户 = **用户名 + 密码**，`sync.email` 与 CLI `--email` 全部删除；派生盐从 `kxtodo|username|email` 改为 `kxtodo|username`，服务端 users 表 `UNIQUE(username)`。**旧库迁移**：启动时检测到 users 表带 email 列就把旧账户整体归档进 `users_legacy` 并重建 users 表（`db.migrate_legacy_accounts`）——旧账户的 auth_key 用旧盐派生，新客户端永远算不出来，留在 users 里只会白占用户名让同名注册被 ACCOUNT_EXISTS 拒掉；归档不删数据（实体/图片仍在，管理台「遗留账户」区可查看与删除）。**不写兼容代码**：旧账户就是登不上了，这是用户确认过的取舍。

### 同步范围三勾选框（v0.5.1，**已被 v0.7.0 的五个勾选框取代**）

设置面板曾只留「同步数据（节点/任务/插图）」「同步设置（配置/配色/背景）」「同步任务」三项，数据与设置默认开；`syncImages` 配置项删除，图片按类别归属范围（entry→数据，background/avatar→设置），见「图片同步」条目的 scopeSignature 机制。
**当前态是五个勾选框**（数据 / 设置 / 任务 / 日记 / 账本，`Scopes` 五个 bool、scopeSignature 五段），见上面「实体与范围」那条——日记与账本各住自己的领域文件后各多一个开关，默认都开（日记此前搭 `scopes.data` 的车，默认开是为了升级上来的用户行为不变）。

### 暂停/恢复同步（v0.5.1）

`sync.enabled=false` = 暂停（自动循环停止、`sync now` 报 `SYNC_PAUSED`），服务器地址/用户名/密码全保留；`sync unpair` 才清密码。设置面板按钮顺序固定为「解除配对 / 暂停·恢复同步 / 立即同步」，暂停时立即同步禁用。

### 配对历史（v0.5.1，v0.6.0 扩展）

`runtime/sync-history.json`（0600，MRU 最多 8 条：mode/serverUrl/lanPeer/username/secret/usedAt），`pair_device` 登录成功后记录；设置页「数据同步」标题右侧历史图标按钮打开浮层一键回填（密码默认打码带显隐切换、可单条删除；回填会连通信方式与主机名一起恢复）；CLI `sync history [--remove N]`。v0.5.1 的旧记录没有 mode 字段，按自建服务理解。密码明文落盘与 `settings.json` 的 `sync.secret` 同等级，不额外降低安全等级。

### 设置面板同步卡片重排（v0.5.1 → v0.6.2）

v0.6.2 的结构：**通信方式下拉**（局域网/自建服务/P2P，固定 9.5em 宽，菜单 `width:max-content` 从右缘展开——触发器 auto 宽会把菜单挤到选项折行）→ 模式专有块（局域网：主机勾选 + 名字 + **管理后台**一行，或「主机 + 可编辑的主机字段（名字或 ip）+ 发现」一行；P2P：设备名字输入 + 设备行 + 列表浮层 + 折叠的自部署 relay/目录）→ **唯一一条 `.sync-status` 状态条**（圆点 + 一句话 + 右侧下次同步倒计时）→ 暂停/错误短句 → 账户行（已配对显示用户名 + 「修改」展开输入框，**本机做服务器时也要能改账户**）或账户表单 → 「同步内容」三勾选一行 + 「同步间隔/掉线重连」两数字框一行 → 动作按钮（三列 / 编辑态两列 / 未配对单列；**掉线时第三个按钮文案变「立即重连」**，连上后自动变回并已完成一次同步）。v0.6.2 的几条硬规则：①「管理台」一律称**管理后台**，「打开」按钮把管理员账密拼在 URL **片段**里自动登录（`admin_login.html` 读完立刻 `history.replaceState` 抹掉；片段不发往服务器，凭据不进访问日志）；②勾选「本机作为服务器」前要求本机已有同步账户/密码（设置值或表单里刚填的都认），没有就一句话告警 + 把勾选拨回（设置没变 Svelte 不会重渲染 checked，得手动拨）；③「最近同步」统一 `YYYY/MM/DD HH:mm:ss ↑推送 ↓拉取`（实体与图片合并计数，不分类）；④P2P 的设备名字与局域网主机名共用 `sync.lanName`。**总原则（用户反馈）**：配置界面不堆说明段落——会错乱、遮盖且冗余；细节一律进 `title` 提示，状态与错误只给一句话。按钮样式仍只许 `settings-button` / `menu-action-button` 两类。

### 管理控制台重做（v0.5.1）

`admin_console.html` / `admin_login.html` 两个内嵌单页（`include_str!`，无外部资产），深色玻璃拟态 + 侧栏五页（概览/账户/数据库/操作日志/会话与活动）；数据源是单个聚合接口 `/admin/api/dashboard`（概览统计 + 每表行数与密文体积 + 图片按类别 + db/WAL 文件大小 + 用户与遗留账户 + token + 持久化操作日志环形缓冲 + 进程内运行指标）。运行指标在 `metrics.rs`：每分钟请求桶（前端补零画 30 分钟活动曲线）、热点路由（路径里的 ID 折叠成 `{id}`）、每用户活动（最近活动/来源 IP/拉推次数）；来源 IP 靠 `into_make_service_with_connect_info::<SocketAddr>()` + `ConnectInfo`（局域网直连没有 XFF）。**两个坑**：① 管理路由必须用绝对路径 + `merge`，`nest("/admin", …)` 在 axum 0.8 下只匹配 `/admin`，手输 `/admin/` 会 404；② 日志行的 kind 小标签类名不能叫 `op`——会和行级 `.op` 选择器撞上，小标签继承 `display:grid` 后文字被挤到标签外（已改名 `.op-row`）。

### 传输分层（v0.6.0，v0.6.1 补完 P2P）

`sync/transport.rs`（HTTP 客户端 12 个方法，三种方式共用一份实现）/ `sync/endpoint.rs`（「这一轮连哪儿」：自建服务直连、局域网按名字找主机、P2P 按枢纽规则拨号）/ `engine.rs`（pull→merge→push 编排）/ `merge.rs`+`crypto.rs`（不变）。**换通信方式只动 endpoint，不动内核**——这是分层的全部意义。`sync.mode` = `lan|server|p2p`；缺省时按 `serverUrl` 是否填过推断（`SyncSettings::effective_mode` 与前端 `normalizeSyncMode` 同口径），所以 v0.5.1 的配置升上来直接可用，不会掉进「还没选主机」的空状态。P2P 的 base_url 是一条**本地临时隧道**，`endpoint::Resolution` 带着隧道句柄，调用方必须把它活到本轮结束（句柄一掉地址即作废）。

### 内置主机（v0.6.0「本机作为服务器」）

server crate 加了 lib 目标，GUI/APK 在自己进程内跑 `kxtodo_server::host::start`（Android 唯一可行路径：targetSdk≥29 不能 exec 外部二进制；桌面 spawn 子进程会让 Android 与 P2P 临时主机各走一套，故统一 in-process）。**core 决定该不该跑，宿主决定怎么跑**：`HostCore::reconcile_sync_host` 挂在 `emit_domain_event(Domain::Settings)` 上（与 scheduler reload 同一套路，不引入新机制），实现是 `HostBackend::sync_host_start/stop`（必须非阻塞：可能在一次命令执行途中被调用）。进程级动作（pidfile / `--daemon` / `--stop` / 信号 / 退出码 / `--update`）只留在 `main.rs` 薄壳——内置主机若写 pidfile，`kxtodo-server --stop` 会误杀宿主 GUI。内置主机的数据目录是 `<todo 数据目录>/server/`（刻意不用 `default_server_dir()`，否则同机两实例与 e2e 测试会共用一份服务器配置，测试就会覆盖开发机真实的 admin 凭据）。

### 主机身份是名字（v0.6.0）

`sync.lanName`（局域网内唯一；勾选当主机前 `ensure_host_name_available` 跑一次发现查重，重名报 `SYNC_HOST_NAME_TAKEN`；广播本身跑不起来则跳过查重，别为环境怪癖挡用户）。客户端选定 `sync.lanPeer`（**名字**，不是 ip:port），上次连到的 host:port 只作为缓存放 `runtime/sync.json.lanEndpoint`，缓存探不通就按名字重新发现——主机换 IP、端口上移都无感。角色二选一（当主机 / 选主机），不变式收在 `SyncSettings::apply_lan_role`（`sync configure` 与 `config set` 共用一条口径；设了其中一个另一个自动清掉）。

### runtime/sync-host.json（v0.6.0）

宿主写、core 只读（实际端口 / instanceId / 管理台地址与账号密码），与 `runtime/host.json`（IPC 描述符）同一套路，于是另开的 CLI 进程也能连上同一个内置主机。管理台密码明文落盘是刻意的：只在首次生成时拿得到（settings.json 里只有哈希），不落盘用户重启后就进不去自己的管理台；与 `sync.secret` 同级（0600、只存本机、不参与同步）。凭据默认不输出，`sync status --show-secrets` 才给。

### instance epoch（v0.6.0，换主机/库重建的自愈）

DB meta 表存 `instanceId`（建库时生成、重启不变），`/healthz`、`/me`、admin dashboard 都返回；客户端记在 `sync.json.serverInstanceId`，不一致就 `reseed_for_new_host`：两条拉取水位 + `pushed` 台账 + token + 图片清单缓存**一起**清零，本机全量副本重新播种（墓碑在 settings 实体里，不会复活已删条目）。**没有这条，换主机会静默地什么都拉不到、推的时候一路 OCC 409**（新库 seq 从 1 开始而本地水位停在几百）。配套 `ensure_login`：账户不在这台主机上就当场注册（并发撞车退回登录）——新库里根本没有这个账户，拒绝同步会把设备晾在一边；密码错仍是 `AUTH_FAILED`，不会被误当成「账户不存在」而悄悄注册新账户。

### 统一 pair（v0.6.0）

`sync register` / `sync login` 合并成 `sync pair`（前端一个「开始同步」按钮，账户不存在当场创建）；`endpoint::PairRequest` 描述「往哪儿配」；配对只写当前模式对应的地址字段（局域网配对不抹掉自建服务地址，反之亦然），所以切换方式不用重填另一种。CLI 路由注意：数据目录有常驻 Host 时 CLI 的**所有**命令都经 IPC 转给 Host 执行，所以 `sync configure --lan-host true` 会真的让正在运行的 GUI 起停内置服务器。**v0.7.3：同步账户与 `profile.displayName` 彻底无关**——早先未配对时用户名输入框会回落到显示名预填，用户因此以为「同步用的就是我这个名字」；现在预填只来自 `sync.username`（解除配对后仍在设置里），显示名一个字都不带。

### 端口生命周期（v0.6.0 上移，v0.6.2 清理）

内置主机与独立二进制都是「端口被占用就往后监听」（`port_fallback`；实际端口一律从 handle / 描述符取，别假定等于配置端口；`configuredPort` 与 `port` 分开记就是为了区分「用户改了端口」和「被占用上移」）。v0.6.2 补的是**释放**这一半：切换通信方式 / 取消「本机作为服务器」时内置主机优雅停机，重启路径会先**有界等待旧服务循环真正退出**（`ServerHandle::shutdown`，5 秒上限）再 bind——只「请求」停机就接着 bind 会撞上自己还没释放的 socket，端口白白上移一格，用户看到的「52177 被占用」往往就是自己留下的。客户端侧：`sync.lanPeer` 除了名字也认 `ip` / `ip:port`（UDP 发现被防火墙挡住时的手工出路），端口留空就在 `discovery::PORT_SWEEP`（52177-52180）上依次试；缓存的 IP 还在但端口探不通时也对同一台机器扫一遍再认名字。残留描述符自愈：`reconcile_sync_host` 发现描述符说在跑但 `pid` 已死（`ipc::host_process_alive`）就清掉当没在跑——端口随进程消失了，残留描述符只会让客户端一直连一个不存在的地址。**活着的别的 kxtodo 进程占端口不去杀**（那可能是用户真在用的独立 server），只上移 + 日志说明。

### P2P 同步（v0.6.1 落地，v0.6.2 补名字，v0.6.3 改逐台对账）

iroh 1.1 承载（QUIC + 打洞 + n0 免费公共 relay；`sync.p2pRelay`/`sync.p2pDirectory` 是留给自部署的覆盖口，默认必须能用免费服务）。同账户设备靠**账户派生密钥**（`SyncKeys.dir_key`，HKDF 独立分支）签的一条 pkarr TXT（DNS 名 `_kxtodo`，≤1000 字节，条目 = `d=<z32 id>.<unix 秒>[.<回环/私网直连地址>]`）互相发现——任何设备只凭用户名密码就能解析出兄弟设备，零额外配置。**一轮跟所有在线设备各对账一次（v0.6.3，`endpoint::resolve_p2p_round`）**：本机内置库排第一，其余逐台拨号，在 **TCP-over-tunnel**（本地临时端口 ↔ iroh 双向流 ↔ 对端内置库回环端口）里各跑一次完整的普通 HTTP 同步，报告聚合成一份（`SyncReport.peers` = 本轮真正对账过的名单，手动同步的 toast 会带上）——transport/merge/crypto 一行没动。**为什么不再只连主设备**：两台设备对目录的视图一旦不一致（发布/解析有先后、条目过期、目录服务不可达），双方会各自认定自己是主设备、各自只跟自己的内置库对账，界面报「同步成功」数据却永远不交汇；逐台对账不依赖选举，视图不一致最多本轮少交换一台。主设备（在线设备里 EndpointId 最小者）退化为**配对/连通性探测的单端点回退口径**（`resolve_p2p`）。某台不在线本轮跳过（拨号失败进 5 分钟冷却），全部不在线才报 `SYNC_P2P_NO_PEER`；目录里只有本机时报告留一条 warning 说明「本轮只与本机内置库对账」。**设备名（v0.6.2，v0.6.3 补本机）**：每台设备用**自己的设备私钥**签一条名字记录（DNS 名 `_name`，内容 `n=<名字>`，键 = 自己的 EndpointId，与账户目录互不覆盖）；`sync peers` 里**本机这条直接取运行时正在发布的名字**（解析自己的记录要绕网络且刚改名时还没发出去），其余先读本机 `p2p.json` 已知记录、缺的再解析名字记录（60 秒负缓存）。设备名 = `sync.lanName`（与局域网主机名同一个字段，缺省机器名），改名不重启端点，发布循环自己跟上。硬约束：relay 无 store-and-forward，对方不在线是正常情况（按 reconnectSeconds 静默重试）；n0 公共 relay 官方口径只适合 dev/testing（用户已接受）。对账状态**逐主机库**存（`runtime/sync.json` 的 `peers` 表，键 = instanceId）：逐台对账每轮来回换档，图片清单缓存因此改成**按 scope 分键的小 map**（单槽会在各库之间抖动）。P2P 运行时与内置服务器都由常驻进程按设置启停（`reconcile_sync_host` 一条路径），设备身份私钥在 `runtime/p2p.json`（0600）。**配对早于设置落盘**（凭据还在请求里），而这两样能力要等 Settings 域事件才启——鸡生蛋，所以 `sync pair` 在解析端点前经 `HostServices::ensure_p2p_services` 请宿主按「将要生效」的设置先起好；没有宿主的进程（CLI 单跑）解析时才报「打开应用」。**同步节奏是多端一致的（v0.6.3）**：`intervalSeconds`/`reconnectSeconds` 进 settings 实体的共享子集（其余 sync 配置仍是本机偏好），一端改了随设置实体推到其它设备（LWW，最后改的赢）；`sync configure` 真的改了节奏才刷新 `syncUpdatedAt`，否则推不出去。不做「取最小值」式协商：复制型设置没有中心协调者，min 需要逐设备记录 + 过期机制，且节奏会随谁在线而变，比「一端改全端跟随」更难解释。**手动同步重排周期（v0.6.3）**：手动 `sync now` 完成（成败都算）写 `stores.manualSyncAt`，自动循环订阅它把下一轮从这一刻起算一个完整间隔，否则旧排程几秒后又插一轮。调研存档：croc 否决——角色构造时定死、单向一次性、无机器接口（非 Windows 上 `send --code` 静默 exit 0）、Android 不能 exec、v11 硬拒 v10 peer。

## 同步功能总开关（v0.6.10，`features.sync`，默认开）

关掉 = 同步的一切功能停——设置页同步整段隐藏（配对信息保留）、`syncRunner` 的 `shouldRun` 带 `featureOn`、Workspace/ListMenu 的「立即同步」入口与 F5 快捷键不给、core 的 `sync_dispatch` 除只读 status/history 外一律报 `SYNC_FEATURE_DISABLED`、`reconcile_sync_host` 的 wanted 恒 false（内置主机与 P2P 运行时一起停）。已有配对的用户不该升级后静默失去同步，所以默认 true。

## 设置共享子集的三处清单必须一致（v0.8.3 修的两个洞）

一个设置项要跨设备，得同时出现在三个地方：`merge.rs::settings_payload`（发）、`merge.rs::apply_settings_record`（收）、`ops_config.rs::is_shared_settings_path`（改了它才刷新 settings 实体的 LWW 时间戳）。**三处各说各话就是这两个洞的成因**：

- **只发不收**：`appearance.tagPresets` / `appearance.dueColors` 一直在载荷里，`apply_settings_record` 却从不读——A 设备改了预置标签，B 设备永远收不到；更糟的是 A 这一改会让整条 settings 实体赢下 LWW，连带把 B 刚改的其它共享设置压掉。修法：apply 侧补读，**校验口径复用 `ops_config::expect_tag_presets` / `expect_due_colors`**（与 `config.set` 同一条路，不各写一份）。v0.8.3 新增的 `appearance.diaryTagPresets`（日记专用的另一套预置）三处一起加。
- **整块替换把本机偏好拨回默认**：features 分支原先是 `settings.features = from_value::<FeatureSettings>(...)`，而载荷只带三个共享键（`showCategoryBadges` / `dueHighlight` / `linkRender`），缺的键全按 serde default 回填——远端赢一次 LWW，接收端的 `mobileBack` / `weekStart` / `editorToolbar` / `features.sync` 就被静默重置。修法：**逐字段覆盖，只动载荷里实际出现的键**（写法对齐 profile / diary / ledger 分支）。`profile` 分支当年修同一个坑时把原理写得很清楚（「缺键不该理解成清成默认」），却没做同类扫描——**「缺键 = 这条记录没提它」是共享子集的通用语义**。
- 钉子：`merge.rs` 的 `presets_and_due_colors_round_trip_and_features_merge_per_field`（往返 + 本机偏好保留 + 非法载荷整组丢弃）。

## 文件传输助手复用 P2P 那套（v0.8.3，`transfer.rs`）

「两台设备凭一句口令互传文件」用的是与 P2P 同步**同一套 iroh + pkarr 打洞**，但**必须是独立的一条通道**：

- **ALPN 不同**（`kxtodo-transfer/1` vs 同步的 `kxtodo-p2p/1`）。同一个 iroh 端点上 ALPN 是唯一的路由依据，重了就会被同步的 accept 循环抢走连接。
- **房间密钥的派生串带自己的域前缀**（`kxtodo-transfer/v1/room/{code}`、握手令牌 `kxtodo-transfer/v1/token/{code}`）：口令 → sha256 → pkarr 的 `SecretKey`，zone 就是公钥，所以「同一句口令 = 同一个房间」。域前缀分开，传输的房间就不可能撞上同步的记录。
- **relay 与 pkarr 目录沿用同步那一套**：`transfer.relay` 空 = 跟 `sync.p2pRelay` 同一个，再空 = n0 公共服务，`disabled` = 只直连/局域网；工具界面右上角可自选（与 P2P 同步的高级覆盖同一个交互）。这两个字段都是**本机配置、不进共享子集**。
- **与账号体系完全无关**（需求明确要求）：不读同步凭据、不写审计、不进 settings 的同步子集。会话是内存里的 `Session`（`cancel(id)` 可中止），进度经 `TransferSink`（`Arc<dyn Fn(Value)>`）吐给前端。
- **v0.8.4 起是「一个常驻在线会话」**（不再是「一次一发」）：输完口令就 `go_online`——用**稳定设备密钥**（`runtime/transfer-identity.json`）起端点、发布房间条目 + 自己的名字（`_name` TXT，复用同步那份 `publish_name`/`fetch_name`，挂在**自己 EndpointId 的 zone** 上）、每 2 秒轮询房间把「匹配到的设备」推给界面、收到拨入就地开一个子会话；发送是「挑一台设备发」（`send(target, payload)`，文件清单或一段文本）。口令记住在 `runtime/transfer-code.json`（明文，与 `sync-credentials.json` 同一条先例），历史与设备名录在 `runtime/transfer-history.json`（近 50 条 + 按 device-id 去重 32 台）——**这三个都在 runtime、不进同步**。
- **接收确认是协议的一部分**：收到文件清单先推 `request` 事件（对方名字 + 清单 + 总大小），等界面 `decide`（60 秒超时当拒绝）；「自动接收」（`transfer.autoAccept`）只是跳过等待。**文本消息走同一条握手**（`mode=text`），不落盘、不进保存位置。
- **别拿 `to_z32()` 的 id 去 `FromStr`**：iroh 只认 RFC4648 base32 与 hex，z32 是另一套字母表——会话里另存一份「z32 → EndpointAddr」的地址表，发送时查表拿可拨号地址。
- 设备名在 `transfer.deviceName`、自动接收在 `transfer.autoAccept`（都是**本机偏好**，`is_shared_settings_path` 不含 `transfer.*`）。
- 握手令牌对**排序后的两个 EndpointId** 做 HMAC：连上之后还要确认对面就是同一句口令的那台，而不是同房间里的第三者。
- 移动端：发送侧走 webview 的 file input，字节按 base64 分片暂存 `runtime/transfer-outbox`（scoped storage 下 core 只能这样拿到内容）；**接收目录两端是同一份实现**——都用 `app.path().download_dir()` 下的 `kxtodo-transfer`。理由：Android 的 `download_dir()` 就是 `getExternalFilesDir(DIRECTORY_DOWNLOADS)`（外部目录，不需要任何权限、USB 与文件管理器都看得到），而 `app_data_dir()` 在 Android 上是**内部** `/data/data/<pkg>`，收到的文件用户根本找不到。**Tauri v2 的 `PathResolver` 没有 `external_app_data_dir()`**（Android 那份只有 audio/cache/config/data/local_data/document/download/picture/public/video/resource/`app_*`/temp/home 这些），凭空写一个平台专有 API 的后果是本地桌面编译与 `ci.yml` 全绿、只有 `release.yml` 的安卓那一栏红。
- 测试用**假的 pkarr relay**（从 `p2p_e2e.rs` 抄的那一套）在本机跑通完整往返，不依赖公网：`crates/core/tests/transfer.rs`。

### v0.8.5：生命周期、复用地基的三条硬规则

- **发送会话绝不持有在线端点**：`iroh::Endpoint::close()` 对**任何一个 clone** 调用都会关掉整个端点（没有引用计数保活）。发送从「每次自起端点」改成复用在线端点之后，收尾若还关端点，一次发完就把整台设备弄下线（`TRANSFER_CLOSED`）。取消与收尾关的都是**会话自己的 `Connection`**（`Session.connection`）；端点只有 `go_offline` 关。测试断言：发完 `status(sender)["online"] === true` 且反向还能再传一条。
- **确认卡是多槽**：`Online.pending` 是 `HashMap<String, PendingRequest>`（request_id 为键），`decide` 按 id 应答；UI 的 `state.requests` 是数组。单槽时第二个拨入会顶掉第一张，第一张的等待循环干等到 60 秒超时当拒绝。
- **目录查询要有缓存**：设备名按 device-id 缓存 20 秒（`NAME_CACHE_TTL`）——每 2 秒对每台设备各查一次公网 pkarr 是纯浪费。send 的 fallback 与设备列表**同一个 fresh 口径**（45 秒）：列表里看不见的设备，发送也如实说「不在线」；fallback 里在房间条目上按 `entry.id.to_z32() == target` 比对，**不要拿 z32 去 `FromStr`**。
- **会话状态与事件订阅住 `transferStore.ts`**（前端模块级单例，`App.svelte` 启动调 `ensureTransferRuntime()`）：有活跃任务不断连；已配对（在线）退出工具页保持在线、手动「离线」才下线（并清掉记住的口令）；未配对退出即清理。工具页只是它的视图。人不在工具页时 `request` 发系统通知 + 侧栏「工具箱」行挂待办角标（当前通知栈没有点击回调，这是诚实落位而不是假装点击穿透）。
- **协议卫生**：大文件读写走 `tokio::fs` / `spawn_blocking`（运行时只有 2 个 worker 线程，同步 IO 会卡住 accept 与心跳）；历史文件读-改-写有进程内锁；同名自动重命名 1000 个候选用尽报 `TRANSFER_NAME_EXHAUSTED`，**绝不回退覆盖**。

## 相关但住在别处的同步内容

| 主题 | 住在哪 |
|---|---|
| 同步分层图（设备 ↔ 主机 ↔ SQLite） | `architecture.md`「同步分层（一图）」 |
| `syncRunner.ts`（全平台自动同步循环的前端实现：订阅 `isHydrated`、递归 `setTimeout`、绝对截止时间排程、在线/掉线双节奏、回前台补一次、`paired` 判据、`stores.syncConnection`） | `frontend.md`「前端分层」的 `syncRunner.ts` 条 |
| 设置抽屉「数据同步」分区的结构与折叠行为、配对历史图标按钮挂在 `SettingsSection` 的 `actions` slot | `ui-patterns.md`「设置抽屉」 |
| ⋯ 列表菜单与日记/记账齿轮面板里的「立即同步」项、快捷键默认 F5（`settings.shortcuts.syncNow`） | `ui-patterns.md`「⋯ 列表菜单」 |
| 移动端下拉同步（`pullrefresh.ts`）与下拉刷新指示器 | `ui-patterns.md`「移动端（Android）」 |
| 手动同步的 toast 在移动端不带 P2P 设备名单 | `ui-patterns.md`「移动端（Android）」 |
| 日记与记账各自的同步 scope、`--sync-diary/--sync-ledger`、四种 ledger 实体 kind | 上面「实体与范围」小节 + `ledger.md` |
| 同步凭据明文留档（`runtime/sync-credentials.json`）、同步账户长度下限（用户名 ≥ 4、密码 ≥ 6） | `history/v0.7.5-v0.7.8.md` 的 v0.7.8 ⑨⑩ |
| kxtodo-server 的双平台发布产物（`kxtodo-server` / `kxtodo-server.exe`） | `build-and-release.md` |
| `wmi` 版本钉死 0.18.1（iroh→netwatch→wmi 的版本范围错配） | 上面「踩坑记录」⑥ + `build-and-release.md`「依赖钉死」 |
| iroh 1.1 要求 rust-version 1.91（四个 manifest 已抬） | 上面「踩坑记录」⑦ |
| 更新下载多通道测速选路（应用内更新，不是同步） | `build-and-release.md` |
