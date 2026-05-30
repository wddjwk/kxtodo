# Todo Note 开发者指南

## 产品目标

Todo Note 是一款快速、本地优先的待办与快捷笔记桌面应用，交互模式参考 Microsoft To Do 的左导航/右内容工作流，但使用完全原创的品牌、素材和实现。核心特点是将分层的分类/条目树与实时渲染的 Markdown 卡片结合，让每个条目既是行动项集合，也是轻量笔记。

## 核心原则

- **本地优先 & 便携**：应用数据、设置、快捷键、导出文件存放在可执行文件同级目录下（目录可写时），开发环境有安全回退路径。
- **单文件桌面应用**：当前主要目标为 Windows，但通过 Tauri 2 保留 Linux / Android 的可能性，不耦合 Windows 专有 API。
- **快速启动**：前端保持精简，通过 Tauri 命令直接持久化 JSON，不引入后台服务。
- **原创外观 + 熟悉的操作**：保持左导航/右卡片画布的经典布局和柔和卡片风格，不复制微软的专有资源或品牌。
- **未来可同步**：同步功能作为适配器边界保留，不将云服务商绑定到任务模型中。

## 技术栈

- **Rust + Tauri 2**：桌面壳、持久化、导入/导出、原生集成。
- **Svelte 4 + TypeScript + Vite**：前端 UI。
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
样式拆分为 6 个全局 CSS 文件，在 `src/main.ts` 中按级联顺序导入：
- `src/styles/base.css`：根变量、重置、滚动条、app-shell、布局。
- `src/styles/titlebar.css`：标题栏 + 窗口控制按钮。
- `src/styles/sidebar.css`：侧边栏、个人资料、搜索、导航、树、图标选择器、右键菜单。
- `src/styles/workspace.css`：工作区、列表头、任务卡片、Markdown、菜单、排序子菜单、添加事项栏。
- `src/styles/settings.css`：设置抽屉。
- `src/styles/shared.css`：浮动通知、工具类。

**为什么使用全局 CSS 而非 Svelte 作用域 `<style>`**：Markdown 内容通过 `{@html}` 渲染，没有 Svelte 作用域属性，作用域样式无法触及 Markdown 元素。`.collapse-button` 在侧边栏（树分类）和工作区（任务卡片）中使用同一类名但不同样式，通过 `.sidebar .collapse-button` 和 `.workspace .collapse-button` 选择器区分。

### 辅助模块
- `src/lib/types.ts`：数据模型类型（`AppNode`、`Task`、`Settings`、`AppState`、`ListBackground`）。
- `src/lib/defaults.ts`：默认值、数据规范化、莫奈配色预设、节点工厂函数。
- `src/lib/backend.ts`：Tauri 命令桥接层，带浏览器开发模式回退。
- `src/lib/markdown.ts`：Markdown 渲染与消毒。
- `src/lib/shortcuts.ts`：键盘快捷键匹配。
- `src/lib/sync.ts`：同步适配器接口（占位）。

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
│   └── shared.css          # 浮动通知
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
│   └── sync.ts             # 同步适配器（占位）
src-tauri/
└── src/main.rs             # Rust 后端
```

## 数据模型

- `AppNode` 表示左侧树节点。内置系统节点有 `my-day`（我的一天）、`planned`（计划内）、`important`（收藏）；自定义 `category` 节点为可展开/收起的文件夹；自定义 `entry` 节点拥有 Markdown 卡片。
- `Task` 属于某个 entry，存储 Markdown 内容、完成状态、"我的一天"/收藏标记、日期元数据（支持精确到分钟的 datetime-local 格式）、以及瞬态的展开/编辑 UI 状态。
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
- **日期精度**：任务日期使用 `datetime-local` 类型，支持精确到分钟，默认值为当前时刻。

## 维护工作流

1. 安装依赖：`npm install`
2. 开发运行：`npm run desktop:dev`
3. 前端构建：`npm run build`
4. 打包发布：`.\scripts\package.ps1 -Version 6.1.0`
5. 加载截图测试数据：`.\scripts\load-sample-data.ps1`（生产默认数据保持最小化）
6. 新原生功能放在 Tauri 命令/插件后面，同步集成放在适配器边界后面。

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
