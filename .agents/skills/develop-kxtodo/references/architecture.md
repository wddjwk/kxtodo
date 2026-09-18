# 架构：进程拓扑 / 五个领域文件 / 数据目录 / 同步分层

> **这份文件是什么**：KXToDo 的**当前架构描述**——三端进程拓扑、五个领域文件与数据地基、数据目录解析、同步分层图。
> **什么时候读它**：要理解「谁在跑、数据落在哪、写操作怎么走」时；改 core / repo / IPC / 移动端宿主 / 新增一类领域文件之前先读这里。
> 硬约束与铁律见 `invariants.md`；同步的全部细节见 `sync.md`；前端分层与 CSS 见 `frontend.md`。

## 目录

- [路径约定（先读这条）](#路径约定先读这条)
- [进程拓扑（v10，最重要的认知）](#进程拓扑v10最重要的认知)
- [五个领域文件与数据地基](#五个领域文件与数据地基)
- [数据目录解析](#数据目录解析)
- [同步分层（一图）](#同步分层一图)

## 路径约定（先读这条）

（**核实补充**，非原 AGENTS.md 文字，2026-09 按磁盘实况核对）AGENTS.md 与本 skill 里写的 `crates/core/src/...`、`crates/server/src/...` 一律**相对 `src-tauri/`**：磁盘真实位置是 `src-tauri/crates/core/src/...`、`src-tauri/crates/server/src/...`。`src/...`（前端 Svelte/TS/CSS）与 `scripts/...` 相对**仓库根**。Tauri 壳在 `src-tauri/src/lib.rs`；CLI 的薄入口 crate 在 `src-tauri/crates/cli/src/main.rs`（它只转调 `kxtodo_core::cli::main_entry`）。core 的测试在 `src-tauri/crates/core/tests/`（如 `tests/ledger_icons.rs` 的仓库相对写法指这里）。

## 进程拓扑（v10，最重要的认知）

```
kxtodo.exe (GUI)                    kxtodo-cli (CLI)
  │ windows 子系统，无控制台          │ 控制台程序，跑完即退
  │ 内嵌 Host（IPC 服务端 + 调度引擎） │ 薄入口 → kxtodo_core::cli::main_entry
  └──────────┬───────────────────────┘
             │  共享同一个 Domain Core（crates/core，kxtodo-core）
             ▼
   Repository（fs2 文件锁 + 原子写 + revision + 幂等台账）
             ▼
   <数据目录>/data.json + settings.json + tasks.json + diary.json + ledger.json
```

- **GUI 是唯一常驻进程**：窗口关闭（默认隐藏到托盘）后 Host 继续跑调度与 IPC；无窗口、无启用任务时看门狗自动退出。
- **CLI 不持有状态**：需要常驻能力（notify 弹通知、schedule run）时，CLI 通过 IPC 找 Host；Host 不在就拉起 GUI 同目录 exe 的隐藏 Host 模式（`--kxtodo-host`），找不到 GUI 则报 `GUI_NOT_FOUND`。
- **GUI/CLI/Agent 三方写操作走同一条业务命令层**（Domain Core 的 Invocation → 域分发 → envelope 输出），不存在"读全量 JSON 改完写回"的路径。
- **Android 同栈**：APK 内嵌同一个 kxtodo-core，HostCore 进程内直跑（`init_mobile_core`：Repository + 移动端 HostBackend），**不启 IPC server / 调度引擎 / 看门狗 / 托盘**；写操作与桌面走完全相同的 core_dispatch 命令层。浏览器 dev 预览（非 Tauri）才走 localStorage legacy 路径。
- **Linux 桌面与 Windows 同拓扑**：GUI 常驻 Host（IPC 服务端 + 调度引擎）+ CLI 经 IPC 找 Host；IPC 传输为抽象命名空间 Unix socket，数据目录 `$XDG_DATA_HOME/kxtodo/todo-note-data`（无则 `~/.local/share`）。GUI 以 **AppImage**（固定名 `KXToDo.AppImage`）分发，CLI 裸二进制固定名 `kxtodo-cli`；CLI 的 `notify` / `schedule run` 需要 GUI 承担隐藏 Host（`--kxtodo-host`）：把 `KXToDo.AppImage` 与 `kxtodo-cli` 放同一目录即可（`find_gui_exe` 认固定名 `KXToDo.AppImage`，也认软链为 `kxtodo` 的 GUI），否则报 `GUI_NOT_FOUND`。应用内更新会把这两个固定名制品下载到 `~/.local/share/kxtodo/bin/` 替换后重启。core 内的 unix 差异：子进程输出 lossy UTF-8 解码、PATH 解析校验可执行位、调度动作进程树用 process_group + killpg 整组控制、隐藏 Host 以 `Stdio::null` + 独立进程组脱离终端、迁移自 Windows 的外来盘符路径（`C:\...`）防止被当相对路径拼到 cwd、runtime 缓存配置失效时回退重新探测（仅 Linux）。

## 五个领域文件与数据地基

上面拓扑图最后一行的 `data.json + settings.json + tasks.json + diary.json + ledger.json` 就是**五个领域文件**（data 节点与任务 / settings 设置 / tasks 定时任务 / diary 日记 / ledger 记账）。它们各自的 `Domain` 变体、layout 路径、`load_*` / `write_*` 与 `ensure_initialized` 里的那一条都在 `crates/core/src/repo.rs`；**加一类要同步的实体要动哪几处**见 SKILL.md 的路由表「加一类**要同步的**实体」行（那一行是权威清单）。

- **数据地基（同步前置，schema v6）**：ID 128-bit 随机 hex（32-bit 会跨设备碰撞）；Node/Item 显式 `order: f64` 字段（同级排序唯一来源，数组顺序仅是渲染缓存，`gui.apply-tree-order` 写 order 不再裸排数组）；`collapsed`/`expanded` 是本机 UI 状态不参与同步。同步状态在 `runtime/sync.json`（deviceId/token/拉取水位/逐实体对账戳，0600），不属于五个 domain 文件。
- **`runtime/` 里住的都是「本机状态」，一律不参与同步**：`sync.json`（水位与设备身份）、`sync-host.json`（内置主机）、`sync-credentials.json`（明文凭据留档，0600）、`linkmeta.json`（超链接元数据缓存，带 `CACHE_VERSION`）、**`reminders.json`（v0.8.3：任务提醒的发送台账，至多一次的凭据）**、**`transfer-outbox/`（v0.8.3：移动端发送文件时的 base64 分片暂存，scoped storage 下 core 只能这样拿到 webview 选中的字节）**。台账进同步载荷的后果很具体：一台设备响过的提醒，另一台就永远不响了。
- **`crates/core/src/` 的两个 v0.8.3 新模块**：`reminders.rs`（任务提醒：规则解析/校验/折算、时钟跳变判定、`Engine::poll` 与台账）与 `transfer.rs`（文件传输：口令派生房间密钥与握手令牌、帧协议、`send`/`receive`/`cancel`、`safe_join`）。两者都**不是**领域文件的所有者：提醒规则住在 `Item.reminders`（跟任务一起同步），传输完全不落领域数据。

## 数据目录解析

- GUI：桌面端 `<平台数据根>/kxtodo/todo-note-data/`（Windows `%LOCALAPPDATA%`、Linux `$XDG_DATA_HOME` 或 `~/.local/share`、macOS `~/Library/Application Support`，即 `kxtodo_core::repo::default_data_dir()`），移动端回退 `app_data_dir()`。缺文件时 `Repository::ensure_initialized()` 立即落盘默认三文件。
- CLI：`--data-dir` > 系统默认数据目录（同 GUI）；最终没有 `data.json` 报 `DATA_DIR_NOT_FOUND`（退出码 3），**CLI 永不静默创建数据**。

（「**不做兼容**：项目没有正式 release……」这一条是铁律，全文见 `invariants.md` 的「不做兼容」。）

## 同步分层（一图）

下面是同步的**分层图**；安全模型、实体与范围、配对流程、传输分层（transport / endpoint / engine / merge / crypto）、内置主机、P2P、图片同步、掉线处理、instance epoch、server 运维等全部细节在 **`sync.md`**。

```
设备A (GUI/CLI, 任一平台)          设备B …
  └─ kxtodo-core::sync::engine     └─ 同一引擎
       │ XChaCha20-Poly1305 密文（AAD=实体ID / 图片ID）
       │ 实体走 JSON，图片走裸字节 blob 通道
       ▼
  主机 = 任一台设备上的 kxtodo-server（v0.6.0 起可 in-process 内嵌在 GUI/APK 里）
    独立二进制：默认监听 0.0.0.0:52177，数据默认 <平台数据根>/kxtodo/server/data.db
    内嵌主机：数据在 <todo 数据目录>/server/，监听端口被占用时自动上移
    SQLite 只存密文+版本号。它是**中转缓存，不是数据归属者**：每台设备都持全量副本，
    主机库丢了由设备重新播种（/healthz 的 instanceId 变化触发全量重对账）
    局域网发现：固定 UDP 52177 收广播/组播查询 → 单播应答 --name + 真实 TCP 端口
```
