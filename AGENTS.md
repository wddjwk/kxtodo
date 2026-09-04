# KXToDo 开发者指南

KXToDo（Todo Note）是一款本地优先的待办 + 快捷笔记桌面应用：左侧分类/条目树、右侧 Markdown 卡片画布（交互参考 Microsoft To Do，品牌与实现完全原创），外加 CLI、定时任务与系统通知。技术栈：Rust + Tauri 2（桌面壳与后端）、Svelte 4 + TypeScript + Vite（前端）、CodeMirror 6（编辑器）、marked + DOMPurify + highlight.js（渲染）。

## 系统架构

### 进程拓扑（v10，最重要的认知）

```
kxtodo.exe (GUI)                    kxtodo-cli (CLI)
  │ windows 子系统，无控制台          │ 控制台程序，跑完即退
  │ 内嵌 Host（IPC 服务端 + 调度引擎） │ 薄入口 → kxtodo_core::cli::main_entry
  └──────────┬───────────────────────┘
             │  共享同一个 Domain Core（crates/core，kxtodo-core）
             ▼
   Repository（fs2 文件锁 + 原子写 + revision + 幂等台账）
             ▼
   <数据目录>/data.json + settings.json + tasks.json
```

- **GUI 是唯一常驻进程**：窗口关闭（默认隐藏到托盘）后 Host 继续跑调度与 IPC；无窗口、无启用任务时看门狗自动退出。
- **CLI 不持有状态**：需要常驻能力（notify 弹通知、schedule run）时，CLI 通过 IPC 找 Host；Host 不在就拉起 GUI 同目录 exe 的隐藏 Host 模式（`--kxtodo-host`），找不到 GUI 则报 `GUI_NOT_FOUND`。
- **GUI/CLI/Agent 三方写操作走同一条业务命令层**（Domain Core 的 Invocation → 域分发 → envelope 输出），不存在"读全量 JSON 改完写回"的路径。
- **Android 同栈**：APK 内嵌同一个 kxtodo-core，HostCore 进程内直跑（`init_mobile_core`：Repository + 移动端 HostBackend），**不启 IPC server / 调度引擎 / 看门狗 / 托盘**；写操作与桌面走完全相同的 core_dispatch 命令层。浏览器 dev 预览（非 Tauri）才走 localStorage legacy 路径。
- **Linux 桌面与 Windows 同拓扑**：GUI 常驻 Host（IPC 服务端 + 调度引擎）+ CLI 经 IPC 找 Host；IPC 传输为抽象命名空间 Unix socket，数据目录 `$XDG_DATA_HOME/kxtodo/todo-note-data`（无则 `~/.local/share`）。GUI 以 **AppImage** 分发，而 CLI 的 `notify` / `schedule run` 需要 GUI 承担隐藏 Host（`--kxtodo-host`）：把 AppImage（或解包出的 GUI 二进制）以 `kxtodo` 为名软链到 CLI 同目录即可被 `find_gui_exe` 找到，否则报 `GUI_NOT_FOUND`。core 内的 unix 差异：子进程输出 lossy UTF-8 解码、PATH 解析校验可执行位、调度动作进程树用 process_group + killpg 整组控制、隐藏 Host 以 `Stdio::null` + 独立进程组脱离终端、迁移自 Windows 的外来盘符路径（`C:\...`）防止被当相对路径拼到 cwd、runtime 缓存配置失效时回退重新探测（仅 Linux）。

### 数据目录解析

- GUI：桌面端 `<平台数据根>/kxtodo/todo-note-data/`（Windows `%LOCALAPPDATA%`、Linux `$XDG_DATA_HOME` 或 `~/.local/share`、macOS `~/Library/Application Support`，即 `kxtodo_core::repo::default_data_dir()`），移动端回退 `app_data_dir()`。缺文件时 `Repository::ensure_initialized()` 立即落盘默认三文件。
- CLI：`--data-dir` > 系统默认数据目录（同 GUI）；最终没有 `data.json` 报 `DATA_DIR_NOT_FOUND`（退出码 3），**CLI 永不静默创建数据**。
- **不做兼容**：项目没有正式 release，不存在需要兼容的旧版用户。数据格式、数据位置等变更一律直接切换，不写迁移/兼容代码；旧数据由用户自行迁移或丢弃。

### 前端分层（src/）

- **stores.ts**：Svelte stores 单一事实来源。`appState`/`appSettings` + derived（`selectedNode`/`visibleTasks`/`listCounts`/`accent`…）。桌面与移动端 hydrate 都走 `coreSnapshot`，之后靠 `kxtodo://domain-changed` 事件 + `refreshFromCore` 增量回刷。
- **actions.ts**：GUI 全部写操作的业务命令层，桌面/移动统一 coreDispatch。**新写操作一律加在这里，不要在组件里直接改 store 或 invoke。**
- **backend.ts**：Tauri invoke 桥接。桌面专属能力（托盘/自启/全局快捷键/webview 缩放/原生文件对话框/导出落盘）经 capabilities 门控在移动端 no-op 或改走替代路径（file input + dataURL 命令、Kotlin 分享桥）；浏览器 dev 回退 localStorage。
- **capabilities.ts**：平台能力层（scheduler/trayLifecycle/globalShortcuts/windowZoom/popupNotificationWindow/systemNotifications/nativeFileDialogs/updateChannel/desktop）。Linux 取值：`updateChannel = "none"`（应用内更新关闭）、`systemNotifications = true` + `popupNotificationWindow = false`（走系统通知，不自绘通知窗）。组件按能力裁剪 UI，不再散落 isMobile 判断；加新平台时只扩这里。
- **platform.ts**：hostOs 检测（官方 `@tauri-apps/plugin-os` 的同步 `platform()`，UA 仅作回退）+ 移动端检测 + 三层历史栈路由（list → content → editor/settings，`{mv:...}` history 条目，popstate 回写 store）。**`startMobileRouter()` 只能由 App onMount 调用**——模块顶层挂载会因 platform ↔ stores/backend/capabilities 循环依赖 TDZ 白屏。
- **longpress.ts**：触摸长按 action（500ms、10px 移动容差、抑制窗去重 Chromium 补发的原生 contextmenu），树行/任务卡/侧栏空白区共用——移动端长按 = 桌面右键。
- **scheduleAdapter.ts**：v9 ScheduleEntry（spec/state/ui 三段）↔ UI 编辑模型双向适配，patch 时保留 CLI 专属字段。
- **纯逻辑**：`nodes.ts`（树查询）、`styles.ts`（样式计算）、`sort.ts`（7 种排序）、`markdown.ts`（渲染消毒）、`defaults.ts`（默认值 + 旧数据规范化）、`platform.ts`（移动端检测 + 导航）。
- **组件**：`App.svelte`（协调器）→ `Sidebar`（导航 + 树 + 右键菜单）、`Workspace`（列表头 + TaskCard 列表 + 添加栏；`selectedNode.id === "scheduled"` 时切换为 `ScheduledTasksView`）、`SettingsDrawer`、`TitleBar`、`Toast`。
- **editor/**：浮窗 Markdown 编辑器（CodeMirror 6，动态 import，挂在 App 层避开 workspace overflow 裁剪）。
- **menu/**：统一菜单系统（ContextMenu/MenuItem/MenuSeparator），所有右键/⋯ 菜单都基于它。

### CSS

全局 CSS（非 Svelte scoped——`{@html}` 渲染的 Markdown 没有 scoped 属性，触及不到）。`main.ts` 按级联顺序导入：base → titlebar → sidebar → workspace → menu → editor → settings → shared → mobile。同名类用父选择器区分（如 `.sidebar .collapse-button` vs `.workspace .collapse-button`）。移动端样式全部收在 `.app-shell.mobile` 下，桌面零副作用。

**按钮样式规范（不许再发明新样式）**：
- 菜单/浮层面板里的动作按钮 → `menu-action-button`（menu.css：白底细边框、圆角 7、hover #f2f4f7）。
- 设置抽屉里的按钮 → `settings-button`（settings.css：同一视觉语言），主操作加 `.primary`（accent 填充白字）。
- 菜单项一律走 `MenuItem` 组件（menu-item-button），不写裸 `<button>`。
- 新写任何按钮前先想这两个类能不能用；风格不一致的裸按钮视为 bug。

## UI 布局与特性

- **布局**：标题栏 + 左侧栏（#f0f0f0 一体化背景）+ 工作区（左上角 12px 圆角）。左侧栏 = 个人资料卡、搜索、系统导航（我的一天/计划内/收藏/定时任务）、自定义树、底部新建。工作区 = 列表头（标题可内联重命名；右侧 展开全部/收起全部/⋯ 菜单）、任务卡片流、底部添加栏。
- **任务卡片**：单击选中复制文本；双击展开/收起（不选词）；右侧笔图标打开浮窗编辑器（编辑/预览切换、插图、Esc 或点外部保存关闭）；右键出菜单（日期/标签/emoji/移动/删除）。
- **树**：分类可折叠、图标可选（Lucide/emoji）、指针拖拽排序（before/after/inside 指示、边缘自动滚动、悬停展开、Esc 取消、拖到空白落根级末尾）、右键菜单、空白区右键新建。
- **⋯ 列表菜单**（ListMenu.svelte）：重命名、移动到分组、排序方式、删除、导出/导入、UI 颜色、背景（色盘 + 莫奈预设 + 图片 + 透明度）。
- **定时任务**：独立视图，卡片三态（compact/expanded/editing），新建后停在编辑态（ui.editing 持久化）。触发器 once/interval/calendar/condition；动作 脚本/可执行文件/通知；执行历史 `schedule logs`。
- **我的一天**：标题下显示当天日期；灯泡 = 智能建议；日历 = 月/周视图回看各天完成项；已完成区只显示当天完成。
- **设置抽屉**：个人资料（头像 dataURL）、外观（缩放/字号/链接打开方式）、生命周期（关闭到托盘/开机自启）、快捷键、云同步占位。
- **Linux 桌面**：与 Windows 同一套现代 UI（`decorations:false` + 自绘 TitleBar，不换原生窗口装饰）；通知走 tauri-plugin-notification 系统通知（libnotify/D-Bus，`capabilities/linux.json` 授权），不自绘通知窗（客户端绝对定位在 Wayland 下不可行）；关闭按钮默认**退出应用**而非隐藏到托盘（WSLg/GNOME 托盘常不可见，设置里可改回 close-to-tray）；应用内更新在 Linux 关闭（`updateChannel = "none"`，用户从 GitHub Releases 下载新 AppImage 覆盖）；全局快捷键受插件平台能力限制（X11 可用，纯 Wayland 抓不到），不做会话嗅探特判。
- **移动端（Android）**：三级导航——主界面是分类列表，点分组展开、点条目/系统列表推入内容页（顶部返回键），设置是独立整页（侧栏资料卡进入），硬件返回键沿历史栈逐级回退（编辑器→内容→列表→退出，MainActivity 的 OnBackPressedCallback 驱动 webview.goBack）。长按 = 右键菜单；定时任务整体隐藏（无调度引擎）；通知走 tauri-plugin-notification 系统通知（首次发送请求 POST_NOTIFICATIONS）；更新 = 设置页检查 → 下载固定名 APK → Kotlin 桥（`window.kxtodoAndroid.installApk`）拉起系统安装器覆盖安装。导出走系统分享面板（shareText 桥），图片选择走隐藏 file input + dataURL 命令。桌面体验不受影响（isMobile 只看 userAgent，能力门控保证桌面命令面不变）。

## 如何修改 / 维护 / 拓展

| 要做什么 | 动哪里 |
|---|---|
| 加/改 CLI 命令 | `crates/core/src/cli.rs`（clap 树）+ 对应 `ops_*.rs`；`schema.rs`/`skills.rs` 自动跟随 |
| 加 GUI 写操作 | `crates/core/src/ops_gui.rs` 加命令 → `actions.ts` 加 coreDispatch 包装 → 组件调用 actions |
| 加设置项 | `model.rs` SettingsFile + `defaults.ts` 默认值/normalize + `SettingsDrawer.svelte` UI |
| 加调度触发/动作类型 | `model.rs`（discriminator 分支）+ `ops_schedule.rs` 白名单校验 + `plan.rs`/`scheduler.rs` 执行 + `scheduleAdapter.ts` 适配 + `ScheduledTasksView.svelte` 编辑表单 |
| 改外观 | 全局 CSS 文件按区域找；配色变量在 base.css；菜单样式统一在 menu.css |
| 加原生能力 | Tauri 命令/插件，桌面专有逻辑必须 `#[cfg(desktop)]` 隔离并在移动端给空实现（前端 invoke 不能炸） |
| Agent 技能文档 | 只编辑 `skills/kxtodo/SKILL.md`（编译期 include_str! 嵌入，发布 exe 自包含） |

**铁律**：写路径永远过 Domain Core 命令层；前端永不直接改 JSON；GUI 桥接默认 `controls.yes = true`（GUI 操作即用户确认，CLI 的确认门不适用于 GUI）。

## 构建 / 测试 / 发布

```bash
npm install                # 依赖
npm run desktop:dev        # 桌面开发（vite + tauri dev）
scripts/cargo-msvc.sh test -p kxtodo-core   # Rust 测试（Git Bash 下必须用这个包装！）
.\release.ps1 windows      # 本地构建 Windows 双产物
.\release.ps1 all          # Windows 双产物 + Android APK
./release.sh               # 本地构建 Linux 制品（AppImage + CLI，须在 Linux 上跑）
node scripts/mobile-ux-test.mjs   # 移动端 UX 回归（playwright-core + 系统 Edge，需先 npm run dev）
.\scripts\publish.ps1      # 一键发布：三产物构建 + gh release create（需要 gh 已登录；release/ 下有同版本 Linux 制品则一并上传，缺失只警告）
```

**版本号只有 git 一个来源**：最近的 `v*` tag，其次最近一条 `vX.Y.Z` 开头的 commit message。`build.rs`（根 crate + crates/core）构建期调用 git 注入 `KXTODO_VERSION`，GUI 经 `app_version` 命令展示在设置页，CLI 的 `version` 命令同源。**仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）——发版只打 tag/写 commit，永远不要往文件里同步版本号。注意 `git describe` 优先于 commit message：**发版必须先 commit 再在 HEAD 上打 tag**，否则 describe 回到旧 tag，构建/发布会拿旧版本号且不报错。

产物：`release/KXToDo-<版本>.exe`（GUI）+ `KXToDo-CLI-<版本>.exe`（CLI）+ `KXToDo.apk`（Android，固定名不带版本——覆盖安装不留历史包；旁挂 `KXToDo.apk.version` sidecar 记录构建版本供 publish 识别旧产物）。Linux：`release/KXToDo_<版本>_amd64.AppImage`（官方 tauri bundler 标准命名 `productName_version_arch`；bundle 由 `src-tauri/tauri.linux.conf.json` 平台覆盖启用，`targets` 仅 `appimage`）+ `release/KXToDo-CLI-<版本>-gnu`（CLI 裸二进制，"-gnu" 是文件名一部分不是扩展名）；版本号构建期经 `tauri build --config "{\"version\":\"$VERSION\"}"` 内联 JSON 注入，**仓库文件仍不写版本号**。构建入口分工：Windows/Android 走 `release.ps1` → `package.ps1`；Linux 走 `release.sh`（版本解析与 build.rs 同语义，AppImage 由 tauri CLI 的 beforeBuildCommand 顺带跑前端构建，不另行 `npm run build`）。APK 的 versionName/versionCode 由 package.ps1 注入的 `KXTODO_VERSION` 环境变量进 gradle（versionCode = 900000000 + X*1000000 + Y*1000 + Z，基线高于旧构建的 8002001 保证升级不降级）。没有 GitHub 构建流水线（已删），构建与发布全在本地。

## 环境坑位（Windows 开发必读）

1. **Git Bash 里 cargo 链接失败**（"link: extra operand"）：uutils-coreutils 的 `link` 遮蔽了 MSVC `link.exe`。**一切 cargo 调用走 `scripts/cargo-msvc.sh`**。
2. **PowerShell 必须 `-NoProfile` 调脚本**的说法反过来也成立：package.ps1/release.ps1 已内置 MSVC 环境自举（cl.exe 不在 PATH 时从 setup_x64.bat 导入），直接跑即可。
3. **Git Bash 里 `taskkill /PID` 会被转成 UNC 路径**——杀进程用 `powershell -NoProfile -Command "Stop-Process -Id <pid> -Force"`。
4. **单实例标识 `com.wddjwk.kxtodo` 全局唯一**：debug、release、旧版本 exe 互相同标识，启动新实例会转发到已运行的旧实例（表现为"改了代码没生效"）。**调试前先把所有 kxtodo/KXToDo 进程杀光（含托盘）**；用户反馈"修复没生效"也优先怀疑旧进程残留。
5. **tauri-plugin-window-state 默认管所有窗口**：动态 label 的窗口（通知窗 `notification-N` 按进程内计数器复用 label）会被恢复历史"不可见/旧位置"状态——曾导致通知窗建好了却看不见。必须用 `with_filter` 按前缀排除；主窗口可见性若由代码接管（conf `visible:false` + 前端 reveal），还要把 `StateFlags::VISIBLE` 从持久化里剥掉，否则恢复逻辑会绕过 reveal 提前显示窗口。
6. **pwsh 5.1 写文件默认 GBK**：脚本里写中文文本一律 `[System.IO.File]::WriteAllText` + UTF-8 no-BOM。
7. **裸 cargo 交叉检查 Android 目标需要 NDK clang 环境**：ureq/ring 是共享依赖后，`cargo check --target aarch64-linux-android` 会在 ring 的 cc-rs 构建脚本里找 clang 失败。gradle 的 rust 插件（`tauri android build`）会自己配好；手工 check 需导出：`PATH += $NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin`、`CC/CXX/AR_aarch64_linux_android=clang.exe/clang++.exe/llvm-ar.exe`、`CFLAGS/CXXFLAGS_aarch64_linux_android=--target=aarch64-linux-android24`。
8. **Node 24 + vite 在 Windows 的退出期 libuv 断言 flake**：`npm run build` 可能已打印 `✓ built in Xs` 但进程退出时崩在 `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)`（exit -1073740791），package.ps1 会按约定报 "Frontend build failed"；`tauri android build` 的 beforeBuildCommand 再跑一次 npm 时同样会中。**偶发，直接重跑该目标即可**（产物以 release/ 下文件与 sidecar 为准）。另外：**跑 release/publish 脚本不要用 `| tail` 管道**——管道会把退出码掩成 0 造成"构建成功"误报，重定向到日志文件再 tail。

### Linux 开发坑位

1. **官方 apt 依赖清单**：`sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`。release.sh 启动时门控：pkg-config 查 `webkit2gtk-4.1` / `gtk+-3.0` / `librsvg-2.0` / `openssl`，appindicator 接受 `ayatana-appindicator3-0.1` **或** `appindicator3-0.1` 任一；**libxdo 例外**——Debian/Ubuntu 的 `libxdo-dev` 只装 `/usr/include/xdo.h` 不带 `libxdo.pc`，所以改查头文件 + `libxdo.so*`（有 .pc 时优先 pkg-config），否则会在装齐依赖的机器上误报缺库。缺库直接打印上面这行安装命令并退出。
2. **裸 cargo 构建出 dev 模式制品（整窗白屏的真正根因）**：Tauri 的 dev/prod 由编译期 `custom-protocol` feature 决定（`tauri::is_dev() = !cfg!(feature = "custom-protocol")`），该 feature **只有 tauri CLI 构建时才会启用**。裸 `cargo build --release` 产出的二进制会去加载 `devUrl`（127.0.0.1:1420），没有 vite 服务时整窗空白——webview 的网络日志里能看到连 1420 的痕迹，极易误判为渲染/GPU 问题。Linux 制品一律走 `npx tauri build`（release.sh 已如此）；手工验证构建用同命令或显式 `cargo build --release --features tauri/custom-protocol`。WSLg 下 stderr 的 MESA ZINK / libEGL 警告与 Gtk-CRITICAL scale-factor 告警是渲染栈噪音，与白屏无关；个别 GPU 栈真遇渲染问题才手动 `export WEBKIT_DISABLE_DMABUF_RENDERER=1`（WebKit 官方开关）。
3. **托盘依赖 appindicator 宿主**：GNOME 默认不显示托盘图标（需装 AppIndicator 扩展），WSLg 下基本不可见——因此 Linux 默认关闭 close-to-tray（关闭按钮直接退出应用），设置项仍可改回。
4. **AppImage 运行需 FUSE**：WSL2 有 `/dev/fuse` 可直接跑；无 FUSE 的环境用 `./KXToDo_<版本>_amd64.AppImage --appimage-extract-and-run`。**首次构建**时 tauri CLI 会自动下载 linuxdeploy，需要网络（之后走缓存）。
5. **cargo 在 Linux 直接可用**：`scripts/cargo-msvc.sh` 只是 Git Bash 下 `link.exe` 被遮蔽的解法，Linux 裸 cargo 即可；构建输出同样不要接 `| tail` 之类管道（掩退出码），release.sh 一律直通终端。
6. **WSLg 的 XWayland 丢光标 + AppImage 强制 x11**：本机 WSLg 实测——任何 X11 客户端（连系统 GTK 探针程序）都看不见鼠标光标，Wayland 原生一切正常；而 AppImage 的 AppRun（linuxdeploy-plugin-gtk 钩子）为绕旧版 WebKitGTK 的 Wayland 崩溃（tauri#8541）强制 `GDK_BACKEND=x11`，正好踩进坏通路（表现：整窗交互正常但光标消失，连进程内原生 GTK 对话框也丢光标）。`run()` 在「APPDIR 存在 + WAYLAND_DISPLAY 存在 + GDK_BACKEND==x11」时撤掉该强制值回 Wayland 原生；纯 X11 会话保持 x11。若将来在旧 WebKitGTK 环境遇 Wayland 崩溃，重新评估这段撤销逻辑。

## Android 开发与构建经验（v0.2.0 实战沉淀）

本机环境：`ANDROID_HOME=D:\software\Android\sdk`、`NDK_HOME=...\ndk\30.0.14904198`、JDK 23（keytool 在 PATH）。构建唯一入口 `.\release.ps1 android|all`（内部 gradle + cargo-ndk 自配工具链）；**不要手工跑 gradlew**。

1. **gen/android 的所有权**：`app/build.gradle.kts`、`app/src/main/**`（Manifest、Kotlin、res）、`app/proguard-rules.pro` 是用户文件可改；`generated/`、`tauri.build.gradle.kts`、`build/` 每次构建再生成，别改。`tauri.properties` 会被 tauri CLI 再生成且内容可能过期（曾残留 versionName 8.2.1）——**版本一律走 package.ps1 注入的 `KXTODO_VERSION` 环境变量**，别信 tauri.properties。
2. **返回键**：生成的 `TauriActivity` 把 `handleBackNavigation` 固定为 false（webview 历史不接管），必须在 `MainActivity.onCreate` 自己注册 `OnBackPressedCallback`（canGoBack→goBack 否则 finish），否则硬件返回直接退应用。goBack 触发的 popstate 由 platform.ts 路由消费。
3. **Kotlin 桥（JS→原生）模式**：`MainActivity.onWebViewCreate` 里 `addJavascriptInterface`；方法标 `@JavascriptInterface`、同步返回 `""`=成功/错误串；**release 开了 minify，必须在 proguard-rules.pro 加 keep 规则**否则桥方法被摇掉。文件分享/安装走 FileProvider：authority = `packageName + ".fileprovider"`（Manifest 已声明，`res/xml/file_paths.xml` 的 cache-path "." 覆盖 cacheDir）；installApk 前校验路径 canonical 后落在 cacheDir 内。
4. **tauri-plugin-dialog 在 Android 的 save() 返回 content:// URI**，Rust `fs::write` 写不了——移动端导出/落盘一律改 Kotlin 桥（shareText 写 cacheDir + ACTION_SEND）或 `<input type=file>` + dataURL 命令；导入用 file input 的 `file.text()`。
5. **触摸长按语义**：Chromium/WebView 在触摸长按后会补发原生 `contextmenu`，自定义长按 handler 要用抑制窗（longpress.ts 的 `isLongPressSuppressed`）去重，否则一次手势开两次菜单；嵌套容器（行 + 外层 nav）会各武装一个长按定时器，外层 handler 必须检查抑制标志/内层菜单已开，否则行菜单被空白区菜单顶替。长按抬手补发的 click 也要吞掉，否则会冒泡关掉刚开的菜单。文本选中放大镜靠 mobile.css 的 `user-select:none + -webkit-touch-callout:none` 抑制。
6. **坐标与缩放**：app-shell 是 transform 缩放的，`position:fixed` 子元素（菜单/浮层）的坐标系跟着缩放——所有用 clientX/Y 定位的浮层都要除以 `uiScaleValue()` 换算逻辑坐标（TaskCard 日期弹窗、ContextMenu 同套路）；移动端开启界面缩放后这条对所有浮层生效。
7. **模块循环 TDZ 白屏**：platform↔stores/backend/capabilities 存在循环依赖，任何在模块顶层订阅 stores 的代码（如历史栈路由）会在启动时 ReferenceError 白屏（桌面+移动全平台）。订阅类初始化必须封装成函数由 App onMount 调用；验证手段：`node` 直接 eval 生产 bundle（配 DOM stub）能复现 TDZ，比肉眼看代码快。
8. **移动端 UX 验证没有真机就用 Playwright 模拟**：`scripts/mobile-ux-test.mjs`（playwright-core + `channel:"msedge"`，Android UA + hasTouch + isMobile context 连 vite dev）。合成 PointerEvent（pointerType:"touch"）可驱动长按 action；浏览器 dev 走 localStorage legacy 路径，足够验证导航/菜单/设置页等纯前端逻辑。APK 原生侧（Kotlin 桥、安装器、返回键）只能靠代码评审 + `aapt dump badging` / `apksigner verify --print-certs` 核验产物元数据与签名。
9. **签名与升级**：keystore 在 `src-tauri/gen/android/keystore/release.jks`（gitignore，密码 kxtodo，package.ps1 首跑自动生成）——**丢了它旧装就无法覆盖升级**。versionCode 公式 `900000000 + X*1000000 + Y*1000 + Z`（gradle 内），基线高于历史脏值 8002001；改公式前先确认单调递增，否则安装器报降级拒装。
10. **APK 产物策略**：release 资产固定名 `KXToDo.apk`（不带版本，覆盖安装不留历史包），旁挂 `KXToDo.apk.version` sidecar 供 publish 识别旧产物；应用内更新 = 下载固定名到 cacheDir → 桥接 installApk 拉系统安装器，不做 shim/多版本/更新日志那套桌面逻辑。
11. **图标同步**：`scripts/make-icon.py` 同时生成桌面 icons 与 android mipmap 五密度（launcher/round/foreground，foreground 按 108dp 画布 66% 安全区）；换 logo 后跑一次即全平台同步，package.ps1 每次构建前会自动跑。
12. **通知**：移动端走 tauri-plugin-notification（Rust 注册插件 + capabilities/mobile.json 的 `notification:default` + Manifest `POST_NOTIFICATIONS`）；Android 13+ 需运行时权限，前端发送前 `isPermissionGranted/requestPermission`，拒绝则降级 Toast。桌面自绘通知窗逻辑不动。
13. **能力门控优先于 isMobile 散落判断**：平台差异收敛到 `src/lib/capabilities.ts`（scheduler/trayLifecycle/globalShortcuts/windowZoom/popupNotificationWindow/systemNotifications/nativeFileDialogs/updateChannel/desktop）；Rust 侧对应 `#[cfg(desktop)]`/`#[cfg(not(desktop))]` 命令面。新增平台只扩这两处 + CSS 命名空间，不改业务组件。

## computer-use 调试经验（WebView2 应用）

KXToDo 的窗口内容是 WebView2 渲染的，**UIA 拿不到 DOM 树**（accessibility 为空），只能用像素截图 + 坐标点击。多轮实战总结：

1. **截图坐标系**：`get_app_state` 返回的坐标是缩放后截图像素，click/scroll/type 直接用；每次操作后必须重新 get_app_state 拿新 snapshot_id 和新坐标，不要复用旧坐标点变了位置的元素。
2. **input_revision 是你的朋友**：两次截图 revision 相同 = 画面没变（操作没生效，或捕获到了缓存帧）。不同 = 真的重绘了。判"点没点上"先看 revision。
3. **hover 无法合成**：CU 没有"移动鼠标不点击"，悬停子菜单、悬停展开这类行为测不了。用 Playwright（`channel: "msedge"` 直接驱动系统 Edge/WebView2 内核）连 vite dev server（`npm run dev` 的 1420 端口）补测 hover 路径——webapp-testing 技能的 `with_server.py` 可管生命周期。
4. **vite dev 长开 + HMR 会污染判断**：dev 实例在多轮源码热更后，运行中的组件可能持有新旧混合的响应式状态，表现出"磁盘数据正确但界面不对""同一帧里两个状态混渲"等灵异现象。**改完前端代码验证前，杀掉 dev 实例重启干净进程**；vite server 本身不用重启（它只serve源码）。
5. **验证数据的权威来源是磁盘文件**：默认数据目录（Windows `%LOCALAPPDATA%\kxtodo\todo-note-data`）下的 *.json 直接可读。界面存疑时先 `cat` 数据文件区分"写错了"还是"画错了"，能省一半时间。
6. **Esc 是 CU 的取消键**：对目标应用发 Esc 可能被 CU 层拦截/取消会话，测试"Esc 关闭浮窗"这类交互时优先用 Playwright 或改用其他关闭路径验证。
7. **通知/托盘弹窗是独立窗口**：`list_windows` 里主窗口旁的小窗口（如 `com.wddjwk.kxtodo-siw`、通知窗）要按 hwnd 单独截图。
8. **定时任务触发等时间相关验证**：用 CLI 造一个 `once` 任务设到 1 分钟后，比改系统时间或注入时钟省事得多；触发后看 `schedule logs` 和磁盘 state 即可闭环。
