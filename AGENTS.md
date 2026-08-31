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

### 数据目录解析

- GUI：桌面端 `<平台数据根>/kxtodo/todo-note-data/`（Windows `%LOCALAPPDATA%`、Linux `$XDG_DATA_HOME` 或 `~/.local/share`、macOS `~/Library/Application Support`，即 `kxtodo_core::repo::default_data_dir()`），移动端回退 `app_data_dir()`。缺文件时 `Repository::ensure_initialized()` 立即落盘默认三文件；GUI 启动时会把旧版 exe 同级的便携数据目录一次性迁入标准目录。
- CLI：`--data-dir` > 系统默认数据目录（同 GUI）；最终没有 `data.json` 报 `DATA_DIR_NOT_FOUND`（退出码 3），**CLI 永不静默创建数据**。

### 前端分层（src/）

- **stores.ts**：Svelte stores 单一事实来源。`appState`/`appSettings` + derived（`selectedNode`/`visibleTasks`/`listCounts`/`accent`…）。core 模式下 hydrate 走 `coreSnapshot`，之后靠 `kxtodo://domain-changed` 事件 + `refreshFromCore` 增量回刷。
- **actions.ts**：GUI 全部写操作的业务命令层。core 路径（`coreDispatch`）+ 移动端 legacy 回退。**新写操作一律加在这里，不要在组件里直接改 store 或 invoke。**
- **backend.ts**：Tauri invoke 桥接（含浏览器 dev 回退）。
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
- **移动端（Android）**：打开只见分类列表，点条目推入正文（顶部返回），硬件返回键经 History 回列表。桌面体验不受影响（isMobile 只看 userAgent）。

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
.\scripts\publish.ps1      # 一键发布：构建 + gh release create（需要 gh 已登录）
```

**版本号只有 git 一个来源**：最近的 `v*` tag，其次最近一条 `vX.Y.Z` 开头的 commit message。`build.rs`（根 crate + crates/core）构建期调用 git 注入 `KXTODO_VERSION`，GUI 经 `app_version` 命令展示在设置页，CLI 的 `version` 命令同源。**仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）——发版只打 tag/写 commit，永远不要往文件里同步版本号。

产物：`release/KXToDo-<版本>.exe`（GUI）+ `KXToDo-CLI-<版本>.exe`（CLI）。没有 GitHub 构建流水线（已删），构建与发布全在本地。

## 环境坑位（Windows 开发必读）

1. **Git Bash 里 cargo 链接失败**（"link: extra operand"）：uutils-coreutils 的 `link` 遮蔽了 MSVC `link.exe`。**一切 cargo 调用走 `scripts/cargo-msvc.sh`**。
2. **PowerShell 必须 `-NoProfile` 调脚本**的说法反过来也成立：package.ps1/release.ps1 已内置 MSVC 环境自举（cl.exe 不在 PATH 时从 setup_x64.bat 导入），直接跑即可。
3. **Git Bash 里 `taskkill /PID` 会被转成 UNC 路径**——杀进程用 `powershell -NoProfile -Command "Stop-Process -Id <pid> -Force"`。
4. **单实例标识 `com.wddjwk.kxtodo` 全局唯一**：debug、release、旧版本 exe 互相同标识，启动新实例会转发到已运行的旧实例（表现为"改了代码没生效"）。**调试前先把所有 kxtodo/KXToDo 进程杀光（含托盘）**；用户反馈"修复没生效"也优先怀疑旧进程残留。
5. **tauri-plugin-window-state 默认管所有窗口**：动态 label 的窗口（通知窗 `notification-N` 按进程内计数器复用 label）会被恢复历史"不可见/旧位置"状态——曾导致通知窗建好了却看不见。必须用 `with_filter` 按前缀排除；主窗口可见性若由代码接管（conf `visible:false` + 前端 reveal），还要把 `StateFlags::VISIBLE` 从持久化里剥掉，否则恢复逻辑会绕过 reveal 提前显示窗口。
6. **pwsh 5.1 写文件默认 GBK**：脚本里写中文文本一律 `[System.IO.File]::WriteAllText` + UTF-8 no-BOM。

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
