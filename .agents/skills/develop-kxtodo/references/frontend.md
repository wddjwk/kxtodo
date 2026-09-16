# 前端分层（src/）与 CSS

> **这份文件是什么**：前端 `src/` 的模块职责划分（stores / actions / backend / capabilities / platform / longpress / scheduleAdapter / syncRunner / 纯逻辑 / measureBus / 组件 / editor / diary / menu）、**前端单测（vitest，v0.8.0）**，以及全局 CSS 的级联顺序、层叠坑与按钮样式规范。
> **什么时候读它**：加/改任何前端写操作、加设置项、加平台能力、改组件结构、写或改任何 CSS、写前端纯逻辑单测、改渲染性能与测量逻辑、遇到「浮层被盖住 / 位置错乱 / 被 overflow 裁掉」、要新建按钮。
> 界面本身的细节（卡片、菜单、日记、记账、搜索、markdown 渲染…）在 `ui-patterns.md` 与 `ledger.md`；移动端/WebGL 类平台坑在 `pitfalls-android.md`。

## 目录

- **前端分层（src/）**
- [stores.ts](#storests)
- [actions.ts](#actionsts)
- [backend.ts](#backendts)
- [capabilities.ts](#capabilitiests)
- [platform.ts](#platformts)
- [longpress.ts](#longpressts)
- [scheduleAdapter.ts](#scheduleadapterts)
- [syncRunner.ts](#syncrunnerts)
- [纯逻辑](#纯逻辑)
- [前端单测（v0.8.0，vitest）](#前端单测v080vitest)
- [组件](#组件)
- [editor/](#editor)
- [diary/](#diary)
- [menu/](#menu)
- **CSS**
  - [全局 CSS 与级联顺序](#全局-css-与级联顺序)
  - [层叠坑（v0.6.6 踩、v0.6.7 修透）](#层叠坑v066-踩v067-修透)
  - [按钮样式规范（不许再发明新样式）](#按钮样式规范不许再发明新样式)

## 前端分层（src/）

### stores.ts

Svelte stores 单一事实来源。`appState`/`appSettings`/**`diaryEntries`**（日记是独立领域，不塞进 appState）+ derived（`selectedNode`/`visibleTasks`/`listCounts`/`accent`…）。桌面与移动端 hydrate 都走 `coreSnapshot`，之后靠 `kxtodo://domain-changed` 事件 + `refreshFromCore` 增量回刷。**v0.8.0 的按域回刷地基**：`appliedRevisions: Map<string,number>` + `noteRevisions()`，`listenCoreEvents` 在 `(appliedRevisions.get(domain) ?? 0) >= revision` 时跳过旧事件；`applySnapshot` 每个分支带 `&& snapshot.X !== undefined` 守卫——**按域拉取时缺的域不能被当成空**；`refreshFromCore` 在清空 pending 之前先捕获 `wanted` 再传给 `coreSnapshot(wanted)`（只拉脏域）；`hydrate()` 把 getAppVersion + hasCoreDispatch + event 模块动态 import 并行化。**v0.8.2 的两处地基**：① `appState`/`appSettings` 的**初值就来自缓存**（`cachedState()` / `cachedAppearance()+cachedProfile()+cachedFeatures()`），水合只是对账——首帧因此能画出用户上次的界面与设置；② `noteEnvelopeRevision(meta)` 把写命令信封里的 `revisionDomain`/`revision` 记进水位，`applySnapshot` 先存 `previous` 再记新值、**按域**判断 `advanced(domain)`，没前进就跳过 `set`（否则一次写入让 store 换两次身份，图表动画重启）。状态缓存的写入闭环也在这个文件里：防抖 800ms、stringify 比对去重、剥 scheduler、`isHydrated` 门控 + 水合完成补写一次、`visibilitychange`/`pagehide` flush。**`debouncedSearchQuery`**（180ms 防抖，空串立即生效、首次发射直通）被 `visibleTasks`/`isSearching`/`searchHits` 消费——每敲一个键就全量混排搜索是交互耗时大头。

### actions.ts

GUI 全部写操作的业务命令层，桌面/移动统一 coreDispatch。**新写操作一律加在这里，不要在组件里直接改 store 或 invoke。** `saveTaskMarkdown` 先算 `expandChanged` 再乐观更新，只在展开态真变了才发 `gui.set-item-ui`（v0.8.0；原来每次保存都白发一条）；`setItemsUi` 用 `new Set(ids)` 判成员。

### backend.ts

Tauri invoke 桥接。桌面专属能力（托盘/自启/全局快捷键/webview 缩放/原生文件对话框/导出落盘）经 capabilities 门控在移动端 no-op 或改走替代路径（file input + dataURL 命令、Kotlin 分享桥）；浏览器 dev 回退 localStorage。**v0.8.0 起 `CoreSnapshot` 字段全部可选** + `revisions?: Partial<Record<...>>`，`coreSnapshot(domains?: readonly string[])` 支持按域拉取（对应 `lib.rs::core_snapshot` 的 `wanted()` 过滤——**新加领域文件必须进那份名单**，见 `invariants.md` 第一节）。

### capabilities.ts

平台能力层（scheduler/trayLifecycle/globalShortcuts/windowZoom/popupNotificationWindow/systemNotifications/nativeFileDialogs/updateChannel/desktop/dataUrlImages）。`updateChannel`：移动端 `"apk"`，桌面（Windows/Linux）一律 `"desktop"`（下载固定名制品→替换→重启；已无 `"none"` 通道）。Linux 其余取值：`systemNotifications = true` + `popupNotificationWindow = false`（走系统通知，不自绘通知窗）。`dataUrlImages`（v0.5.0 起 = Linux **或** 移动端）：这两类环境的 webview 拿不到 asset 协议子资源（WebKitGTK 根本不发请求 / Android WebView 取不到 `http://asset.localhost/`），图像一律走 `image_data_url` 命令转 base64——**该命令必须同时注册进移动端 invoke_handler**，否则前端调用直接失败。组件按能力裁剪 UI，不再散落 isMobile 判断；加新平台时只扩这里。

### platform.ts

hostOs 检测（官方 `@tauri-apps/plugin-os` 的同步 `platform()`，UA 仅作回退）+ 移动端检测 + 三层历史栈路由（list → content → editor/settings，`{mv:...}` history 条目，popstate 回写 store）。**`startMobileRouter()` 只能由 App onMount 调用**——模块顶层挂载会因 platform ↔ stores/backend/capabilities 循环依赖 TDZ 白屏。

**返回键接管一律走 `createBackGuard()`**（`addBackInterceptor` 的封装）：`{#if}` 挂载式的浮层用 `$: backGuard(true, close)` + **`onDestroy(() => backGuard.dispose())`**，`open` 是 prop 的用 `$: backGuard(open, onClose)`。漏 dispose 的后果是**返回键被一个已经看不见的浮层永久吃掉**（v0.8.1：菜单开过一次之后整页返回键再也没反应）——SKILL.md 3.8。

### longpress.ts

触摸长按 action（500ms、10px 移动容差、抑制窗去重 Chromium 补发的原生 contextmenu），树行/任务卡/侧栏空白区共用——移动端长按 = 桌面右键。

### scheduleAdapter.ts

v9 ScheduleEntry（spec/state/ui 三段）↔ UI 编辑模型双向适配，patch 时保留 CLI 专属字段。

### syncRunner.ts

全平台自动同步循环（App onMount 调 `startAutoSync()`，浏览器预览不启动）。**入口绝不能用 `coreMode` 门控**——onMount 时水合还没完成、coreMode 恒为 false，v0.4.1 就是这样把整条自动同步写成了死代码；现在订阅 `isHydrated` 等水合完成再启动，并立即同步一次。**首轮同步刻意延迟 1200ms**（v0.8.0 的 `FIRST_ROUND_DELAY_MS`：`boot()` 结尾 `arm(1200)` 而不是立刻 `tick()`）——首轮不和水合抢主线程。递归 `setTimeout` + **绝对截止时间排程**（下一轮 = 本轮开始 + 间隔，v0.5.1 起；「跑完再等一个间隔」会让周期逐轮漂移）+ 在线/掉线双节奏（`intervalSeconds` / `reconnectSeconds`）+ 回前台补一次；排程时间写进 `stores.nextSyncAt`（面板显示「下次同步 Ns」）。**配对与开关分开判**（v0.6.0 起按通信方式）：`paired = 有账户密码 + 当前方式有明确对端`（自建服务看地址、局域网看「本机是主机或已选定主机」、P2P 不需要），与 core 的 `SyncSettings::is_paired` 同口径；`enabled=false` 是暂停（循环停止但配置保留）。连接状态写进 `stores.syncConnection`，设置面板只订阅它显示圆点状态。

### 纯逻辑

`nodes.ts`（树查询；v0.8.0 起 `buildListCounts` 单趟遍历计数 + `childrenOf` Map + 记忆化 `countSubtree`，搜索路径建 `nodeById` Map）、`styles.ts`（样式计算）、`sort.ts`（7 种排序）、`markdown.ts`（渲染消毒；v0.8.0 起 block/inline 两套 LRU 记忆化——300 条/4MB 与 800 条/400KB，`renderStats` 计数挂 `window.__kxtodoRenderStats` 供 perf-bench 断言「首屏 block 渲染为 0」；`DIAGRAM_PATTERN` 等正则在模块级编译一次，highlight/diagrams/codeblocks 都带早退）、**`measureBus.ts`（v0.8.0 新）**（全应用共享**一个 ResizeObserver + 一个 window resize 监听 + rAF 合帧**，导出 `observeResize(element, listener): () => void`；RO 的条目只通知受影响的元素、window resize 通知全部——**测量一律走它，别再每个组件自己 new RO**）、`defaults.ts`（默认值 + 旧数据规范化）、`platform.ts`（移动端检测 + 导航）、`diary.ts`（日记的日期/分组/日历/统计/摘要，无外部依赖）、`plannedGroups.ts`（计划内分区）、`clock.ts`（时刻；v0.8.0 起 `parseClock` 与 core `parse_clock` 对齐、`clockOf` 改用 `new Date()` 解析 + 本地钟点，来历见 `history/v0.8.md` 批次 4 ①）、`images.ts`（v0.8.0 起 `boundedPut` 给三个 dataURL 缓存加体积上限：md 64MB / background 32MB / avatar 8MB）、`ledger.ts`（记账纯逻辑，口径与约定见 `ledger.md`）、**`deferredMarkdown.ts`（v0.8.1 新，v0.8.2 改两阶段）**（展开一张卡片的渲染调度器：记忆化命中或短文本（< `TWO_PHASE_MIN_CHARS` = 1000）同步出完整版；长文本**先同步上屏快速版**（`renderMarkdownFast`：跳过 hljs、公式用 `restoreMathSource` 摆回转义源码），双 rAF 后再升级成完整版——单 rAF 的回调仍在当帧绘制之前触发，等于没让出时间。两版 HTML 逐字节相同时跳过第二次 `apply`（无代码块无公式的文档两版一样）。`schedule(markdown, nodeId)` / `cancel()`。早先只有「让帧再算」一条路，长卡片点开那一瞬画的是折叠态内容 = 用户看到的「先展开一个空白块」）、**`markdownTasks.ts`（v0.8.1 新）**（渲染出的第 N 个任务框 ↔ 源码第几行的唯一实现，判据照抄 marked 的 `listIsTask`；单独成模块是为了能跑 node 单测）、**`dueHighlight.ts`（v0.8.1 新）**（临期高亮：分档 / 三锚点渐变插值 / 加重档 / 颜色工具，纯函数）、**`rmb.ts`（v0.8.1 新）**（人民币金额 ↔ 财务大写**双向**：`toChineseYuan` 复用 `ledger.ts::parseYuanToCents` 解析、按四位一节分级；`fromChineseYuan` 反向识别（简繁体都认：九/玖、两/兩、貳/贰、参/參…，亿/万分级、节内零占位、「拾伍」十位无数字按 1、结尾「整/正」剥掉、有「元」但整数段为空回 null）+ `formatYuanNumber`）、**`tagColors.ts`（v0.8.1 新）**（**十色**配色表 + 胶囊内联样式，色值与 `workspace.css` 的 `.task-tag.tag-*` 同一组；v0.8.2 补 `pink`，选择器是两排 5 + 5，最后一颗是炫彩胶囊 = `<input type="color">` 色盘）、**`linkPreview.ts`（v0.7.7 起）**（超链接三档增强；**v0.8.2 起可逆**：`cardOriginalAnchor`/`titleOriginalText` 两张 WeakMap 留退路，`revertUnwanted` 在收集 anchor 之前先退，`applyTo` 在 await 之后重读档位——增强是破坏性的，而 markdown 有记忆化不会重渲，不留退路就等于「设置拨了画面不动」）。

### 前端单测（v0.8.0，vitest）

`npm run test:unit`（vitest 5）：六个 spec `src/lib/__tests__/{ledger,clock,defaults,markdownTasks,rmb,dueHighlight}.spec.ts`（161 用例），覆盖资金路径（`parseYuanToCents` / 余额 / 统计分桶）、时刻（`parseClock` / `clockOf`）与 normalize。**独立 `vitest.config.ts`，刻意不复用 vite.config.ts**——vitest 的 CONFIG_NAMES 是「找到第一个就用」，独立文件就完全避开 svelte 插件、`optimizeDeps.include:["mermaid"]` 与 rollupOptions。environment = `node`，**不装 jsdom/happy-dom**：`defaults.ts` 顶层调 plugin-os 的 `platform()` 会抛 ReferenceError，正好被它自己的 try/catch 接住回落到 `navigator.userAgent`，node 环境因此跑得通。**断言必须时区无关**（CI 的 ubuntu 是 UTC、开发机是 UTC+8）：一律用 `todayDate()`/`shiftDays()` 相对构造或无时区时间戳——五个时区（UTC / New_York / Berlin / Shanghai / Kiritimati）全绿是验收口径。CI 里插在 `npm run check` 之后、`npm run build` 之前。这套单测挖出了 6 个正确性问题（日记时刻从来没显示过、62 天分桶门槛两侧不同、≥39 位金额静默归零、搜「,」列出整本账、NaN 字号整页失效、legacy 单图折叠判据相反），清单与修复在 `history/v0.8.md` 批次 4。**v0.8.1 起纯逻辑要单独成模块才有单测**——`markdown.ts` 一 import 就要一个 window（DOMPurify 在模块级建实例），任务框的索引逻辑因此拆成了 `markdownTasks.ts`；边界值多的算法（金额大写、渐变色）也要有单测，别只靠手点两下。

### 组件

`App.svelte`（协调器）→ `Sidebar`（导航 + 树 + 右键菜单 + 移动端搜索结果面板）、`Workspace`（列表头 + TaskCard 列表 + 添加栏；`selectedNode.id === "scheduled"` 时切换为 `ScheduledTasksView`）、`DiaryView`（日记整页，与 Workspace 平级）、`SearchResults`（移动端搜索结果）、`SettingsDrawer`、`TitleBar`、`Toast`。**v0.8.0 的渲染纪律**：TaskCard / DiaryCard 的 `fullHtml` **只在展开时渲染**（`$: fullHtml = isExpanded ? renderMarkdown(...) : ""`——**Svelte 的 `$:` 是急切求值，与模板消不消费无关**，折叠态卡片原来也白跑 12 步 markdown + DOMPurify；`resolvedMd` 仍 eagerly 算，它负责触发插图预加载）；`Workspace` 的 `measuredExpandable` 是 `Set<string>`，`handleCardMeasure` 判 `has` 后 add/delete 再 **`measuredExpandable = measuredExpandable` 自赋值失效**（Svelte 的失效基于赋值，Set 原地改不触发）；`IconGlyph` 的 `KNOWN_ICONS` 放在 `<script context="module">`（否则每个图标实例都重建一个 228 项的 Set）；IconPicker 在 App/Sidebar 里 `{#await import(...)}` 动态加载（它带着 emoji-picker-element，不进首屏 chunk）——**五处 `{#await import}` 都配了 `{:catch}` + `.lazy-fallback` 兜底块**（懒加载失败不能让用户「点开什么都没有也退不出去」）。

**v0.8.1 的两条渲染纪律**：① 卡片的完整渲染交给 `deferredMarkdown.ts`，别把重活压在展开那一次 flush 里；② `setConfig` 一律**先本地生效再落盘**（等 IPC + 原子写回来才翻 UI = 点一下顿一下），失败按原值回滚。

**v0.8.2 的四条**：① 长卡片**两阶段**（快速版同步上屏，装饰异步补），短卡片一条老路；② `canExpand` = 多行 ∪ 标题溢出 ∪ **当前就是展开的**（展开态挂载时 `measureTitle` 不在树上，量出来的那两项都不可信；`DiaryCard` 同款）；③ 插图不再进渲染缓存键——渲染出的是 `<img src="" data-md-img="nodeId/file">` 占位，由 `markdownWire` 的 `fillImagesIn` 订阅 `mdImageCache` 异步填（预热走 `preloadMarkdownImages`），缓存键因此与图片解析进度解耦；④ `SettingsDrawer` **分两段挂载**（`deepReady` 双 rAF）：个人资料/外观效果立即画，「特性开关」到「关于与更新」随后——移动端点头像进设置卡顿就是这么治的。

### editor/

浮窗 Markdown 编辑器（CodeMirror 6，动态 import，挂在 App 层避开 workspace overflow 裁剪）。**有新建模式**（`taskId` 为空 + `draftNodeId`），与日记编辑器共用 editor.css 的 `.editor-meta*` 元数据行样式。**编辑/预览页签是滑块切换（v0.7.2）**：`.editor-mode-switch` 两列等宽 grid + 绝对定位的 `.editor-mode-thumb`，容器 `data-mode` 决定滑块平移（transition 在 transform 上）；早前是 active 按钮直接换底色，切换看着像闪一下。两个编辑器（任务/日记）共用这套。

### diary/

`DiaryCard`（日期栏 + 标题 + 摘要/展开正文 + 心情天气标签）、`DiaryEditor`（元数据行 + 标题 + CodeMirror 正文，动态 import 挂 App 层）、`DiaryEntryMenu`（卡片菜单，日记视图与搜索结果共用）。

### menu/

统一菜单系统（ContextMenu/MenuItem/MenuSeparator），所有右键/⋯ 菜单都基于它。

## CSS

### 全局 CSS 与级联顺序

全局 CSS（非 Svelte scoped——`{@html}` 渲染的 Markdown 没有 scoped 属性，触及不到）。`main.ts` 按级联顺序导入：base → titlebar → sidebar → workspace → settings → menu → shared → editor → diary → mobile（mobile 必须最后，它覆盖前面所有区域）。同名类用父选择器区分（如 `.sidebar .collapse-button` vs `.workspace .collapse-button`）。移动端样式全部收在 `.app-shell.mobile` 下，桌面零副作用。

### 层叠坑（v0.6.6 踩、v0.6.7 修透）

`.workspace > *` / `.diary-view > *` 这类「把直接子元素统一压到 `position:relative; z-index:1`」的规则有**两种**死法，都是特异性相同（`*` 不计特异性）谁在后面谁赢：① 新面板的 CSS 文件比 workspace.css 晚加载，于是 `.list-header` 的 `z-index:20` 被压成 1，头部下拉面板被 DOM 靠后的副标题盖住（齿轮点不开，Playwright 报 "intercepts pointer events"）；② 更狠的——`.diary-view > *` 写在 diary.css（比 menu.css 晚），把 `.context-menu` 的 `position:fixed` 与 `.diary-fab-row` 的 `position:absolute` 全盖成 `relative` 掉回正文流：+ 按钮跑到左下角、三点菜单被 flex 拉成整行宽、再被 `.diary-view` 的 `overflow:hidden` 裁掉一半（v0.6.6 日记界面「整个位置都错乱了」就是它）。修法不是补更高特异性的补丁，而是**把兜底规则换成显式列举正文流子元素**（`.diary-view > .diary-subtitle, .diary-view > .diary-search, .diary-view > .diary-scroll`），浮层与浮动按钮自己的定位规则不再有人抢；`.list-header` 自带 `position:relative; z-index:20` 也不必再列。新增整页面板时照抄 `.workspace` 的盒子样式，就要连这一条一起考虑：**绝不要写 `> *`**。**最后一处违例 `.workspace > *` 本身也在 v0.8.0 删掉了**，换成显式列举 `.workspace > .list-subtitle, .workspace > .task-list, .workspace > .add-task-bar, .workspace > .scheduler-panel` 四个正文流子元素：刻意**不列**的是自带定位的 `.list-header`（relative/z-index 20）、`.planned-group-bar`（relative/30）、`.link-preview-overlay`（fixed/9000）、`.context-menu`（menu.css，fixed/3600）；列进来的四个必须抬，因为 `.workspace::before` 是 absolute/z-index 0 的背景层，正文流子元素退回 static 就画在它下面（背景图会盖住卡片与输入栏）。

### 按钮样式规范（不许再发明新样式）

- 菜单/浮层面板里的动作按钮 → `menu-action-button`（menu.css：白底细边框、圆角 7、hover #f2f4f7）。
- 设置抽屉里的按钮 → `settings-button`（settings.css：同一视觉语言），主操作加 `.primary`（accent 填充白字）。
- 菜单项一律走 `MenuItem` 组件（menu-item-button），不写裸 `<button>`。
- 新写任何按钮前先想这两个类能不能用；风格不一致的裸按钮视为 bug。
