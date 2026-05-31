# Todo Note 开发者指南

## 产品目标

Todo Note 是一款快速、本地优先的待办与快捷笔记桌面应用，交互模式参考 Microsoft To Do 的左导航/右内容工作流，但使用完全原创的品牌、素材和实现。核心特点是将分层的分类/条目树与实时渲染的 Markdown 卡片结合，让每个条目既是行动项集合，也是轻量笔记。

## 核心原则

- **本地优先 & 便携**：应用数据、设置、快捷键、导出文件存放在可执行文件同级目录下（目录可写时），开发环境有安全回退路径。
- **跨平台桌面 + 移动**：当前主要目标为 Windows，自 v7.0.0 起通过 Tauri 2 mobile 支持 Android；保留 Linux / iOS 的可能性，不耦合 Windows 专有 API。
- **快速启动**：前端保持精简，通过 Tauri 命令直接持久化 JSON，不引入后台服务。
- **原创外观 + 熟悉的操作**：保持左导航/右卡片画布的经典布局和柔和卡片风格，不复制微软的专有资源或品牌。
- **未来可同步**：同步功能作为适配器边界保留，不将云服务商绑定到任务模型中。

## 技术栈

- **Rust + Tauri 2**：桌面壳、持久化、导入/导出、原生集成。前端为 `src-tauri/src/lib.rs` 中的 `run()`（`#[cfg_attr(mobile, tauri::mobile_entry_point)]`），`main.rs` 仅调用它；桌面专有插件（托盘、全局快捷键、窗口状态、单实例、开机自启）通过 `#[cfg(desktop)]` 和 `cfg(not(android/ios))` 目标依赖隔离，Android 不会编译它们。
- **Svelte 4 + TypeScript + Vite**：前端 UI，桌面与移动共用同一套组件。
- **Markdown 渲染**：客户端使用 GitHub 风格 Markdown 样式，支持标题、强调、高亮语法、链接、列表、任务列表、行内代码，以及 highlight.js 驱动的代码块（GitHub 浅色主题）。

## 架构（v6.0.0+）

前端采用模块化组件架构 + 集中式状态管理：

### 状态层（`src/lib/stores.ts`）
- **Svelte writable stores**：`appState`、`appSettings`、`isHydrated`、`showSettings`、`searchQuery`、`toastMessage`。
- **Derived stores**（计算值）：`selectedNode`、`listCounts`、`visibleTasks`、`accent`、`selectedBackground`、`isSearching`。
- `commit()` 更新 `appState` 并排队防抖保存；`commitSettings()` 对设置做同样操作，并在缩放变化时触发原生副作用（zoom、autostart）。
- `hydrate()` 加载持久化数据、同步原生外观、注册全局快捷键、同步生命周期设置。
- `isHydrated` 防护机制防止初始加载时触发保存。

### 纯逻辑模块
- **`src/lib/nodes.ts`**：树遍历与查询函数（`descendantEntryIds`、`nodeAndDescendantIds`、`ancestorIds`、`tasksForNode`、`buildListCounts`、`buildVisibleTasks`、`moveTargetOptions`、`getBackground`、`exportStateForNode`）。所有函数为纯函数，接受显式参数，不访问 store。
- **`src/lib/styles.ts`**：样式计算（`buildAppShellStyle`、`buildSettingsDrawerStyle`、`buildMainStyle`、`buildMenuStyle`、`accentForNode`、`uiScaleValue` 等）。

### 组件
- **`App.svelte`**：精简协调器（~60 行），挂载子组件、处理全局键盘快捷键、协调子组件间的 overlay 关闭。
- **`TitleBar.svelte`**：窗口标题栏 + 最小化/最大化/关闭按钮。
- **`Toast.svelte`**：浮动通知。
- **`Sidebar.svelte`**：左侧面板——个人资料卡片、搜索框、系统导航、自定义树（ListTree）、右键菜单、图标选择器、底部新建按钮、调整宽度手柄。
- **`Workspace.svelte`**：主内容区——列表头、列表菜单（排序/背景/主题/导出/导入）、任务列表（TaskCard）、任务右键菜单、添加事项栏、展开/收起全部按钮。
- **`SettingsDrawer.svelte`**：设置面板——个人资料、外观、生命周期、快捷键、云同步占位。
- **`TaskCard.svelte`**：单个任务卡片，支持 Markdown 渲染、编辑、展开/收起。
- **`ListTree.svelte`**：递归树组件，支持拖放。
- **`IconGlyph.svelte`**：Lucide 图标或 emoji 渲染。
- **`IconPicker.svelte`**：图标/emoji 选择器浮层。

### CSS 架构
样式拆分为 7 个全局 CSS 文件，在 `src/main.ts` 中按级联顺序导入：
- `src/styles/base.css`：根变量、重置、滚动条、app-shell、布局。
- `src/styles/titlebar.css`：标题栏 + 窗口控制按钮。
- `src/styles/sidebar.css`：侧边栏、个人资料、搜索、导航、树、图标选择器、右键菜单。
- `src/styles/workspace.css`：工作区、列表头、任务卡片、Markdown、菜单、排序子菜单、添加事项栏。
- `src/styles/settings.css`：设置抽屉。
- `src/styles/shared.css`：浮动通知、工具类。
- `src/styles/mobile.css`：Android/移动端堆叠布局（仅在 `.app-shell.mobile` 下生效，桌面无副作用）。

**为什么使用全局 CSS 而非 Svelte 作用域 `<style>`**：Markdown 内容通过 `{@html}` 渲染，没有 Svelte 作用域属性，作用域样式无法触及 Markdown 元素。`.collapse-button` 在侧边栏（树分类）和工作区（任务卡片）中使用同一类名但不同样式，通过 `.sidebar .collapse-button` 和 `.workspace .collapse-button` 选择器区分。

### 辅助模块
- `src/lib/types.ts`：数据模型类型（`AppNode`、`Task`、`Settings`、`AppState`、`ListBackground`）。
- `src/lib/defaults.ts`：默认值、数据规范化、莫奈配色预设、节点工厂函数。
- `src/lib/backend.ts`：Tauri 命令桥接层，带浏览器开发模式回退。
- `src/lib/markdown.ts`：Markdown 渲染与消毒。
- `src/lib/shortcuts.ts`：键盘快捷键匹配。
- `src/lib/platform.ts`：移动端检测（`isMobile`，基于 userAgent，保证 Windows 不受影响）与移动端导航状态（`mobileView`、`showMobileContent`、`showMobileList`，含 Android 硬件返回键 History 处理）。
- `src/lib/sync.ts`：同步适配器接口（占位）。

### 移动端（Android，v7.0.0+）
- **交互模型**：参考 Microsoft To Do 的 Android 版。打开应用只显示分类区域（桌面版左侧栏）；点击条目后推入正文区（桌面版右侧栏），顶部出现返回按钮。
- **检测**：`isMobile` 仅基于 `navigator.userAgent`（Android/iPhone/iPad）。Windows 桌面窗口有大尺寸最小宽度，永不命中，桌面体验保持不变。
- **布局**：`App.svelte` 在 `.app-shell` 上加 `mobile` + `view-list`/`view-content` 类；`src/styles/mobile.css` 将侧边栏与工作区绝对定位铺满屏幕并切换显隐，移动端缩放固定为 1（`buildMobileShellStyle`），隐藏标题栏与宽度手柄，设置抽屉全屏。
- **硬件返回键**：进入正文时 `history.pushState`，`popstate` 返回分类列表而非退出应用。
- **Rust 隔离**：托盘、全局快捷键、窗口状态、单实例、开机自启、`set_webview_zoom` 等命令在 Android 上为 `#[cfg(not(desktop))]` 的空实现，前端 invoke 不会失败；`open_url` 在移动端走 `tauri-plugin-opener`；`data_dir` 在移动端回退到 `app_data_dir()`。
- **已知限制**：移动端的文件导入/导出、背景图片选择依赖 `plugin-dialog` 的文件路径，在 Android 上可能静默失败（不崩溃），后续版本再做移动端专用文件流。

## 项目结构

```
src/
├── App.svelte              # 精简协调器
├── main.ts                 # 入口 + CSS 导入
├── styles/
│   ├── base.css            # 根样式、重置、布局
│   ├── titlebar.css        # 标题栏 + 窗口按钮
│   ├── sidebar.css         # 侧边栏 + 树 + 图标选择器
│   ├── workspace.css       # 任务 + Markdown + 菜单
│   ├── settings.css        # 设置抽屉
│   ├── shared.css          # 浮动通知
│   └── mobile.css          # 移动端堆叠布局
├── lib/
│   ├── stores.ts           # 集中式状态 + 持久化
│   ├── nodes.ts            # 纯树查询函数
│   ├── styles.ts           # 样式计算函数
│   ├── TitleBar.svelte     # 窗口控制
│   ├── Toast.svelte        # 浮动通知
│   ├── Sidebar.svelte      # 左侧面板
│   ├── Workspace.svelte    # 主内容区
│   ├── SettingsDrawer.svelte # 设置面板
│   ├── TaskCard.svelte     # 任务卡片组件
│   ├── ListTree.svelte     # 递归树组件
│   ├── IconGlyph.svelte    # 图标渲染
│   ├── IconPicker.svelte   # 图标/emoji 选择器
│   ├── types.ts            # 数据模型类型
│   ├── defaults.ts         # 默认值 + 数据规范化
│   ├── backend.ts          # Tauri 桥接
│   ├── markdown.ts         # Markdown 渲染
│   ├── shortcuts.ts        # 快捷键匹配
│   ├── platform.ts         # 移动端检测 + 移动导航状态
│   └── sync.ts             # 同步适配器（占位）
src-tauri/
├── src/
│   ├── main.rs             # 二进制入口，调用 lib 的 run()
│   └── lib.rs              # Rust 后端核心（desktop/mobile cfg 隔离）
└── gen/android/            # Tauri 生成的 Android Gradle 工程（首次 `tauri android init` 创建）
```

## 数据模型

- `AppNode` 表示左侧树节点。内置系统节点有 `my-day`（我的一天）、`planned`（计划内）、`important`（收藏）；自定义 `category` 节点为可展开/收起的文件夹；自定义 `entry` 节点拥有 Markdown 卡片。
- `Task` 属于某个 entry，存储 Markdown 内容、完成状态、完成时间戳 (`completedAt`)、"我的一天"/收藏标记、日期元数据（精确到天的 `YYYY-MM-DD` 格式）、以及瞬态的展开/编辑 UI 状态。
- `Settings` 存储可编辑的个人资料（含头像 data URL）、显示偏好（CSS 缩放、UI/Markdown/编辑器字号、链接打开模式）、生命周期偏好（关闭到托盘、开机自启）、本地快捷键绑定、全局唤起快捷键、云同步配置（已禁用）。
- 左侧树的顺序由 `nodes` 数组决定。拖放和"移动到分组"菜单需同时更新 `parentId` 和数组位置。
- 桌面端仅允许单实例运行，第二次启动会聚焦已有窗口；关闭窗口默认隐藏到托盘。

## 技术要点

- **UI 缩放**：`.app-shell` 使用 `transform: scale(var(--ui-scale))`，`transform-origin: top left`，内部尺寸用 `calc(100/scale)vw` 适配视口。
- **WebView2 怪癖**：当 `#app` 设置 `overflow: hidden` 时，缩放容器外的 `position: fixed` 元素不会渲染，因此窗口控制按钮必须放在 `.app-shell` 内部。
- **单实例**：`tauri_plugin_single_instance` 使用应用标识 `com.wddjwk.todonote`，release 目录下的旧 exe（同一标识）会造成冲突。
- **便携数据存储**：`data_dir()` 解析为 `<exe所在目录>/todo-note-data/`。
- **开机自启错误**：`tauri_plugin_autostart` 在禁用时如果快捷方式不存在会抛出 "file not found"（os error 2），当 `launchAtStartup` 为 false 时静默捕获。
- **左侧与顶部一体化**：标题栏和侧边栏共用 `#f0f0f0` 透明背景，工作区左上角有 `border-radius: 12px` 圆角，营造 Microsoft To Do 风格的一体化视觉效果。
- **排序功能**：工作区支持 7 种排序模式（创建时间正序/倒序、字母正序/倒序、截止时间正序/倒序、重要性），排序状态为组件局部变量。
- **展开/收起全部**：跳过单行 Markdown 内容的任务，避免展开后无意义。
- **自定义颜色**：背景颜色网格末尾有彩虹色盘按钮，点击调用原生 `<input type="color">` 色盘；EyeDropper API 可用时支持取色器。
- **日期精度**：任务日期使用 `date` 类型（精确到天），日期显示为中文格式（"X月X日"），可点击跳转日历选择。
- **"我的一天"功能**：标题下方显示当前日期；已完成区仅显示当天完成的任务（`completedAt` 过滤）；灯泡建议按钮提供智能添加建议（昨天/今天创建的未完成任务、今天到期的任务、回退到最近5条）；日历按钮展示月/周视图下各天的已完成任务。
- **完成时间追踪**：`completedAt` 字段在任务标记完成时设置，取消完成时清除。旧数据迁移时以 `updatedAt` 作为回退值。
- **双击防选中**：双击任务卡片展开/收起时自动清除文本选择，compact 模式下 `user-select: none`。

## 维护工作流

1. 安装依赖：`npm install`
2. 桌面开发运行：`npm run desktop:dev`
3. Android 开发运行：`npm run android:dev`（需 ANDROID_HOME / NDK，连接设备或模拟器）
4. 前端构建：`npm run build`
5. 一键打包发布（Windows + Android）：`.\scripts\package.ps1 -Version 7.0.0`
   - 仅 Windows：`.\scripts\package.ps1 -Version 7.0.0 -Targets windows`
   - 仅 Android：`.\scripts\package.ps1 -Version 7.0.0 -Targets android`
   - 产物输出到 `release/`：`KXToDo-<版本>.exe` 与 `KXToDo-<版本>.apk`
6. 加载截图测试数据：`.\scripts\load-sample-data.ps1`（生产默认数据保持最小化）
7. 新原生功能放在 Tauri 命令/插件后面（注意 desktop/mobile cfg 隔离），同步集成放在适配器边界后面。

## v7.0.0 变更摘要

### Android 支持（保持 Windows 体验不变）
- Rust 后端从 `main.rs` 重构为 `lib.rs` 的 `run()`（`#[cfg_attr(mobile, tauri::mobile_entry_point)]`），`main.rs` 仅调用它。
- `Cargo.toml` 新增 `[lib]`（`staticlib`/`cdylib`/`rlib`）；托盘、全局快捷键、窗口状态、单实例、开机自启移到 `cfg(not(android/ios))` 目标依赖，Android 改用 `tauri-plugin-opener`。
- 所有桌面专有逻辑用 `#[cfg(desktop)]` 隔离；相关命令在移动端提供空实现，前端 invoke 不报错。`data_dir` 在移动端回退到 `app_data_dir()`。
- 新增 `src/lib/platform.ts`（`isMobile`、`mobileView`）与 `src/styles/mobile.css`。

### Android 交互（对齐 Microsoft To Do 移动版）
- 打开仅显示分类区域；点击条目推入正文区，顶部返回按钮回到分类列表。
- 移动端缩放固定为 1、铺满屏幕，隐藏标题栏与宽度手柄，设置抽屉全屏。
- Android 硬件返回键经 History（pushState/popstate）返回分类列表而非退出应用。

### 打包
- `scripts/package.ps1` 支持 `-Targets all|windows|android`，一条命令产出 Windows exe + Android apk，并同步 `APP_VERSION`。
- 首次 Android 打包会自动安装 Rust Android 目标并在缺失时执行 `tauri android init`。

## v6.2.0 变更摘要

### 界面 & 交互
- 任务勾选圆圈：compact 模式居中，expanded/editing 模式固定在顶部。
- 双击展开/收起不再选中文本（`event.preventDefault()` + 清除选区）。
- 任务日期精度改为天（`date` 类型），移除分钟精度。
- 任务卡片上的日期可点击，跳出系统日期选择器。
- 创建任务时不再自动添加日期（"计划内"视图下也不自动设置）。

### "我的一天"增强
- 标题下方展示当前日期（"X月X日, 星期X"）。
- 新增灯泡建议按钮：显示昨天/今天创建的未完成任务、今日到期的任务，无匹配时回退显示最近5条。
- 新增日历按钮：月视图/周视图切换，展示各天已完成任务列表。
- 已完成区仅展示当天完成的任务。
- 设置日期为当天时自动添加到"我的一天"。

### 数据模型
- `Task` 新增 `completedAt?: string` 字段。
- `normalizeTask` 迁移旧数据时用 `updatedAt` 回退设置 `completedAt`。

## v6.1.0 变更摘要

### 界面美化
- 左侧栏 + 顶部栏颜色统一，去除分割线，工作区左上角圆角，呈现现代一体化视觉。
- 任务勾选圆圈垂直居中。
- 任务日期显示字体加大，中文格式化（"5月30日 22:00"）。
- 日期选择器默认当前时刻，支持精确到分钟。
- 顶栏收窄（44px → 36px）。
- 所有菜单样式统一（列表菜单、任务右键菜单、树右键菜单）。
- 编辑按钮图标更换为 PenLine。

### 功能改进
- 背景颜色网格末尾加入彩虹色盘按钮，调用系统颜色选择器。
- 预设颜色替换为莫奈配色（日出、睡莲、干草垛、教堂、花园、鸢尾等）。
- 颜色悬浮效果改为仅显示高亮外框。
- 三点菜单新增"排序方式"子菜单，支持 7 种排序。
- 三点菜单左侧新增"展开/收起全部"按钮（单行内容跳过展开）。
