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

**乐观更新一律过 `withRollback(store, label, mutate, run)`（v0.8.3）**：先本地生效、再落盘，失败按原值回滚并 toast。回滚前用**对象身份**判「这期间是否又被改过」（store 是不可变更新，身份还是我们写进去的那一份才回滚，否则不动，免得抹掉用户后一次改动）——与 `setConfig` 同一套语义。已接上的五处：`setNodeIcon` / `setNodeCardStyle` / `renameNode` / `applyTreeOrder` / `setBackground`。**必须回滚的理由**：`gui.*` 不发域事件，失败之后没有任何快照来纠正，界面会与盘上的状态永久分叉（`applyTreeOrder` 的 `TREE_ORDER_MISMATCH` 是设计内失败，最容易撞上）。纯 UI 的展开态（`set-item-ui` 一族）刻意不接：失败无实质后果，回滚反而与用户连点打架。

`setUiColor` / `unsetUiColor` 走 `config.set` 的 mapKey 分支，**要记 `noteEnvelopeRevision(envelope.meta)`**（与 `setConfig` 同一条纪律，否则每改一次颜色多付一轮快照往返）。

**导入设置按 core 的字段目录摊（v0.8.3）**：`settingsWritePlan()` 先取 `config.list`，命中已知路径就整份写下并停止下钻（`appearance.dueColors` 这类整份对象因此不会被摊成 core 不认的 `dueColors.<节点id>` 叶子），map 型（`appearance.uiColors`）按 mapKey 逐键写，目录里没有的键跳过；`applyImportedSettings()` 逐条 try/catch，单条失败只报条数不中断。旧实现无脑摊到叶子、只硬编码过滤 `uiColors.*`，于是「全部数据」导入只要带 settings 就必然失败（`sync.syncDiary`/`syncLedger` 当时不是 config 路径 → 第一条 `UNKNOWN_CONFIG_KEY` → 整轮中断，data 已写、settings 半截）。

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

**v0.8.3 新增/改口径的纯逻辑**：

- **`markdownIndent.ts`**：进 marked 之前保住行首空白（缩进换成不间断空格 + 给**上一行**行尾补 markdown 硬换行）。硬换行必须补在上一行：插在本行行首会让这一行不再是续行，marked 把列表提前收口、缩进行掉到列表外另起一段。**不动缩进属于结构的行**（列表标记 / 引用 / 标题 / 分隔线 / setext 下划线）与围栏内、缩进代码块（≥4 空格且无 tab）。导出的 `indentWidthOf`（tab 按 4 算的纯计数）被 `markdownTasks.ts` 复用。
- **`markdownTasks.ts` 改口径**：**有序列表同样产任务框**（`1. [ ] x` / `1) [ ] x`），行匹配补认 `\d{1,9}[.)]`；≥4 空格缩进要带 `listOpen` 判断——列表**外**是缩进代码块（不算），列表**内**是子列表（算），空行不重置（松散列表），顶格普通行才收列表。此前只认 `[-*+]`，一出现有序任务项，渲染出的框与源码行就错位（点勾选无声空操作却照样写盘 / 混排时翻错行），而旧单测把「有序不算」钉成了契约。
- **`dueHighlight.ts` 四档**：`DueBucket` 加 `overdue`，默认配色四个（灰/红/黄/蓝），已过期不插值不加重，渐变锚点是后三档。**档数是跨语言常量**（前端默认配色 / `normalizeDueColors` / core 的 `expect_due_colors` / 色盘 UI 四处），`normalizeDueColors` 以 `DEFAULT_DUE_COLORS.length` 为准，另三处由 `tests/frontend_contract.rs` 钉住。
- **`currentTime.ts`**：`currentMinute` 可读 store（60 秒对齐 + `visibilitychange`/`focus` 补刷），驱动「刚过期」的翻转与「几小时内」的加重档——不能靠每次渲染重算 `new Date()`（卡片不会自己重渲）。
- **`reminders.ts`**：提醒规则的前端纯逻辑（`remindersParam` 编码成 CLI/core 认的 `due-<分钟>` / `+1h` / RFC3339，以及展示用的文案），与 core 的 `reminders.rs::parse_rules` 同口径，有单测。
- **`sort.ts::compareDue`**：截止排序比**完整时刻**（`dueMoment`，没有时刻按当天 23:59:59），不再比日期字符串——否则同一天里「09:00 到期」与「只精确到天」永远平手，顺序交给 sort 的稳定性 = 交给插入顺序（表现是「带时刻的恒在上面，升序降序都一样」）。无日期的两个方向都沉底。
- **`tools/catalog.ts` / `tools/navigation.ts`**：工具**目录**（id/名称/描述）与**注册表**（图标 + 懒加载组件）分开，侧栏的固定行只需要前者——让 nav 直接 import registry 会把 lucide 图标与所有工具 chunk 拉进首屏链。`navigation.ts` 管 `tool:<id>` ↔ 固定行 id；**v0.8.4 起它还是「当前打开哪个工具」的唯一真源**（`toolRoute: Writable<ToolId | null>`，子视图纯派生）。

**v0.8.4 新增的纯逻辑**：

- **`windowing.ts`**：长列表窗口化的全部公式（`prefixSums` / `indexAtOffset` 二分 / `windowRange`（上下 overscan + `maxCount` 硬上限）/ `initialCount`（首屏至少 30）/ 滚动锚点 `anchorAt` + `scrollTopForAnchor`）。单独成模块是为了能跑 node 单测——这几条公式分错了只表现为「列表跳一下」，肉眼很难定位。13 条单测。
- **`colorPreview.ts`**：取色预览的活值 store（`scope` = 节点 id / `diary` / `ledger` / `toolbox`）+ 三个纯选择器（`accentWithPreview` / `backgroundWithPreview` / `dueColorsWithPreview`）。**消费端只认自己那一份 scope**（不然在日记页改色会把工作区也染上）；6 条单测。

### 前端单测（v0.8.0，vitest）

`npm run test:unit`（vitest 5）：十二个 spec `src/lib/__tests__/{ledger,clock,defaults,markdownTasks,markdownIndent,reminders,sort,rmb,dueHighlight,windowing,colorPreview,taskToggle}.spec.ts`（230 用例），覆盖资金路径（`parseYuanToCents` / 余额 / 统计分桶）、时刻（`parseClock` / `clockOf`）、normalize、markdown 的行首空白与任务项映射、提醒规则编解码、截止排序、金额大写与临期配色。**独立 `vitest.config.ts`，刻意不复用 vite.config.ts**——vitest 的 CONFIG_NAMES 是「找到第一个就用」，独立文件就完全避开 svelte 插件、`optimizeDeps.include:["mermaid"]` 与 rollupOptions。environment = `node`（**v0.8.5 起装了 happy-dom**，只在需要真 DOM 的 spec 顶上写 `// @vitest-environment happy-dom`——`taskToggle.spec.ts` 要对比手术更新与整篇重渲的 DOM 形态；其余 spec 仍是 node，`defaults.ts` 顶层调 plugin-os 的 `platform()` 会抛 ReferenceError，正好被它自己的 try/catch 接住回落到 `navigator.userAgent`，node 环境因此跑得通）。**断言必须时区无关**（CI 的 ubuntu 是 UTC、开发机是 UTC+8）：一律用 `todayDate()`/`shiftDays()` 相对构造或无时区时间戳——五个时区（UTC / New_York / Berlin / Shanghai / Kiritimati）全绿是验收口径。CI 里插在 `npm run check` 之后、`npm run build` 之前。这套单测挖出了 6 个正确性问题（日记时刻从来没显示过、62 天分桶门槛两侧不同、≥39 位金额静默归零、搜「,」列出整本账、NaN 字号整页失效、legacy 单图折叠判据相反），清单与修复在 `history/v0.8.md` 批次 4。**v0.8.1 起纯逻辑要单独成模块才有单测**——`markdown.ts` 一 import 就要一个 window（DOMPurify 在模块级建实例），任务框的索引逻辑因此拆成了 `markdownTasks.ts`；边界值多的算法（金额大写、渐变色）也要有单测，别只靠手点两下。

**`defaults.spec.ts` 里有 normalize 的往返钉子（v0.8.3）**：拿 core 的 `Node` / `Item` 字段全集构造一份原始快照，跑 `normalizeState`，逐个断言**键集合与值都在**（外加幂等：把输出再喂一遍不变）。「core 加字段、前端 `normalize*` 漏掉 → 每次快照刷新抹掉用户值」这一类 bug 从此有自动化守着——`dueTime` 那次漏了两个大版本无人察觉，`node.updatedAt` 是第二个，靠的都是散文规则。

**跨语言的孪生常量钉在 Rust 侧**：`crates/core/tests/frontend_contract.rs` 用 `include_str!` 把前端 TS 当字符串读进来比对（TagColor 十色的三处名单、`BACKGROUND_MAX_EDGE` ↔ 壳的背景闸、`DEFAULT_DUE_COLORS` 档数 ↔ `expect_due_colors` 的校验）。手法照 `tests/ledger_icons.rs`；**新加一组「两边各写一份」的常量就往那里加一条**。

### 组件

`App.svelte`（协调器）→ `Sidebar`（导航 + 树 + 右键菜单 + 移动端搜索结果面板）、`Workspace`（列表头 + TaskCard 列表 + 添加栏；`selectedNode.id === "scheduled"` 时切换为 `ScheduledTasksView`）、`DiaryView`（日记整页，与 Workspace 平级）、`SearchResults`（移动端搜索结果）、`SettingsDrawer`、`TitleBar`、`Toast`。**v0.8.0 的渲染纪律**：TaskCard / DiaryCard 的 `fullHtml` **只在展开时渲染**（`$: fullHtml = isExpanded ? renderMarkdown(...) : ""`——**Svelte 的 `$:` 是急切求值，与模板消不消费无关**，折叠态卡片原来也白跑 12 步 markdown + DOMPurify；`resolvedMd` 仍 eagerly 算，它负责触发插图预加载）；`Workspace` 的 `measuredExpandable` 是 `Set<string>`，`handleCardMeasure` 判 `has` 后 add/delete 再 **`measuredExpandable = measuredExpandable` 自赋值失效**（Svelte 的失效基于赋值，Set 原地改不触发）；`IconGlyph` 的 `KNOWN_ICONS` 放在 `<script context="module">`（否则每个图标实例都重建一个 228 项的 Set）；IconPicker 在 App/Sidebar 里 `{#await import(...)}` 动态加载（它带着 emoji-picker-element，不进首屏 chunk）——**五处 `{#await import}` 都配了 `{:catch}` + `.lazy-fallback` 兜底块**（懒加载失败不能让用户「点开什么都没有也退不出去」）。

**v0.8.1 的两条渲染纪律**：① 卡片的完整渲染交给 `deferredMarkdown.ts`，别把重活压在展开那一次 flush 里；② `setConfig` 一律**先本地生效再落盘**（等 IPC + 原子写回来才翻 UI = 点一下顿一下），失败按原值回滚。

**v0.8.2 的四条**：① 长卡片**两阶段**（快速版同步上屏，装饰异步补），短卡片一条老路；② `canExpand` = 多行 ∪ 标题溢出 ∪ **当前就是展开的**（展开态挂载时 `measureTitle` 不在树上，量出来的那两项都不可信；`DiaryCard` 同款）；③ 插图不再进渲染缓存键——渲染出的是 `<img src="" data-md-img="nodeId/file">` 占位，由 `markdownWire` 的 `fillImagesIn` 订阅 `mdImageCache` 异步填（预热走 `preloadMarkdownImages`），缓存键因此与图片解析进度解耦；④ `SettingsDrawer` **分两段挂载**（`deepReady` 双 rAF）：个人资料/外观效果立即画，「特性开关」到「关于与更新」随后——移动端点头像进设置卡顿就是这么治的。

**v0.8.4 的五条**：① **长列表一律走 `VirtualStack.svelte`**（窗口化：`slot="item" let:row` 拿当前项；`fullBelow` 阈值内全量直出；高度由 `measureBus` 跟踪）——**store 里数据永远全量，只调「挂多少」**；virtua 用不了（runes `children` snippet 与 legacy `let:` 不兼容，编译期就报 `invalid_default_snippet`）。② **`$:` 语句里别依赖「被调用函数写的状态」重新调度**：Svelte 5 legacy 的 `legacy_pre_effect` 把 `active_effect` 指向父分支再 `untrack`，写入不会重新调度同组语句或模板（症状：路由到了界面不动）——`ToolboxView` 因此改成「store 派生」，`ToolboxView` / `Sidebar` 的固定行都从这个真源读。③ **收缩包裹容器里的内容一加粗就改变容器尺寸**：日历面板、菜单子面板里的内容都要定宽（`.date-picker-grid` 228px）。④ **`position: fixed` 之外，挂在主容器外的浮层还要 `--accent` 兜底**（`.app-shell` 上给一次默认值）——裸 `var(--accent)` 未定义会让整条声明失效（日历里「选中日白字透明底」就是这么来的）。⑤ **拖拽重排的落点要按布局算**：单列比 Y（行内上半/下半），双列/图标模式在同一竖带内比 X；拖动过程用「行实时让位 + `animate:flip`」而不是只画落点线。

**v0.8.3 的五条**：① **一个面板组件、多个入口**——`TaskDateReminderPanel.svelte` 被右键菜单（`Workspace`）、卡片日期浮层（`TaskCard`）、编辑器工具栏（`MarkdownEditorModal`）三处挂载，入口只给 `onSave`/`onClear`/`onClose`，内部状态机（`main`/`time`/`custom`）与返回键层级自洽（`embedded` prop 决定退到顶时是关浮层还是交还给宿主）；日历抽成 `CalendarGrid.svelte` 与 `DatePicker` 共用。② **`deferredMarkdown` 的短路键是「文本 + nodeId」**：同一份正文搬到别的条目下，本地图的解析结果完全不同（`transformLocalImages` 按 nodeId 找文件），只比文本会让搬走的卡片一直挂着指向旧节点的占位——随后跑「释放空间」那些图会被当孤儿真删。③ **色盘一律「活值在组件 state、`change` 才落盘」**：`<input type="color">` 的 `input` 事件在拖动过程中连发，逐次 `config.set` 会把 settings.json 的原子写打爆（用户看到的「自定义颜色保存失败：原子替换失败」）。四处（临期配色四块、节点色、背景色、日记/记账外观）同一套纪律。④ **`position: fixed` 的整屏浮层要进宿主的 `closeOverlays`，并在自己根上 `on:click|stopPropagation`**：前者保证公共入口关得掉（否则切页后浮层盖在新页面上残留），后者保证 App 的「点空白关所有浮层」不会让「在浮层里点一下」把自己关掉（`LedgerImagePreview` / 两个管理器都是这个写法）。⑤ **同一个浮层的返回键与 Esc 共用一个 `stepBack()`**：各写一份迟早分叉（`AccountManager` 的 Esc 曾跳过「账户类型小表单」那一档）。

**v0.8.5 的四条**：① **量尺寸分清两套坐标**：壳上有 `transform: scale(uiScale)`，`getBoundingClientRect` 是**缩放后的视觉像素**，而 `scrollTop`/`offsetHeight`/`clientHeight`/占位高度是**布局像素**。`VirtualStack.measure` 早先用 rect 量高，默认 0.75 缩放下每行欠 25%、累积误差让「跳转到某天」差好几张卡——**测量/记账用 `offset*`，命中/绘制用 rect**；`.virtual-item { display: flow-root }` 让子卡片外边距算进占位；`scrollToIndex` 落位后再双 rAF 对一次位（第一次用的是估高前缀和；浏览器把越界 scrollTop 夹回来不是用户操作，别设「用户滚过就放弃」的守卫）。② **传输助手的状态与事件订阅住 `transferStore.ts`**（模块级单例，`App.svelte` 启动 `ensureTransferRuntime()`），工具页只是视图——生命周期三规则与多槽确认卡见 `sync.md`。③ **渲染态勾选走「手术式更新」**（唯一被允许的「渲染非纯函数」例外）：`markdown.ts::markCheckedItem/unmarkCheckedItem`（渲染与点击共用）+ `setRenderedTaskBox`（目标态从源码推、**不要 preventDefault**）+ 卡片 `skipNextRender` 豁免 + `fullRender.adopt` 账本；`{@html}` 同值不重建，回退要靠「先清空再写回」。细节与三个必须一起上的理由见 `history/v0.8.5.md` 四。④ **`markdown.ts` 的整篇渲染缓存叫 `docCache`**（键 = 节点 + 全文，从来不是块级；真·块级缓存留档备查）。

### editor/

浮窗 Markdown 编辑器（CodeMirror 6，动态 import，挂在 App 层避开 workspace overflow 裁剪）。**有新建模式**（`taskId` 为空 + `draftNodeId`），与日记编辑器共用 editor.css 的 `.editor-meta*` 元数据行样式。**编辑/预览页签是滑块切换（v0.7.2）**：`.editor-mode-switch` 两列等宽 grid + 绝对定位的 `.editor-mode-thumb`，容器 `data-mode` 决定滑块平移（transition 在 transform 上）；早前是 active 按钮直接换底色，切换看着像闪一下。两个编辑器（任务/日记）共用这套。

### diary/

`DiaryCard`（日期栏 + 标题 + 摘要/展开正文 + 心情天气标签）、`DiaryEditor`（元数据行 + 标题 + CodeMirror 正文，动态 import 挂 App 层）、`DiaryEntryMenu`（卡片菜单，日记视图与搜索结果共用）。

### menu/

统一菜单系统（ContextMenu/MenuItem/MenuSeparator），所有右键/⋯ 菜单都基于它。

### v0.8.6 新增的前端模块

- `src/lib/searchScan.ts` —— 全局搜索的**分块扫描器**（`createSearchScanner` / `SEARCH_CHUNK` / `SEARCH_HIT_LIMIT`；idle + timeout 调度、token 作废、完成批才排序封顶）。消费端 `stores.ts` 的 `searchHits` / `searchScanning`（可写 store）与 `window.__kxtodoSearch` 调试出口。
- `src/lib/transferManifest.ts` —— 传输清单的路径语义（四个构造器 + `groupByRoot` / `joinPath` / `splitPathTail` / `isAbsoluteRel`），**纯逻辑、有单测**。
- `src/lib/transferEvents.ts` —— 传输事件 → 文案的纯映射（`transferErrorText` / `historyStatusStyle`）；单独成模块的理由是 `transferStore.ts` 静态 import 了 Tauri，node 单测 import 不进来。
- `src/lib/dragHit.ts` —— 拖动落点几何（`settledTop` / `schmitt` / `zoneAt` / `contiguousZones` / `positionInRow` / `keepsPreviousDecision` / `scaleOf`），纯函数 + 单测。
- `src/lib/cardOverlays.ts` —— 卡片级浮层与露出态的**全局单例**（`datePopoverTaskId` / `revealedTag` / `revealedEmoji`）+ **唯一一份** document pointerdown；App 启动调 `ensureCardOverlayRuntime()`。TaskCard 不再自带 `svelte:window`。
- `src/lib/colorPickerPanel.ts` + `src/lib/ColorPickerPanel.svelte` + `src/styles/colorpanel.css` —— 全应用统一取色盘（单例请求 store + iro 懒加载 + RGB/HEX 校验纯函数 `normalizeHexInput`/`parseChannelInput`）。12 处入口只调 `openColorPicker({key,color,anchor,onPreview,onConfirm,onCancel})`。**v0.8.7 补**：`openColorPicker` 先作废旧会话并给 key 追加会话序号（`${key}#${++nonce}`）；`confirm()` 前 `flushDraftsIntoPicker()` + `flushPendingPreview()`；面板操作行有「吸管」（`'EyeDropper' in window` 才渲染）；面板挂进 `.kx-color-canvas > .kx-color-canvas-scale`（反缩放层）+ 宿主高度 `base.offsetHeight / uiScale`。
- `src/lib/popover.ts::placePopover` —— 点锚定浮层的唯一几何（逻辑像素进/出），ContextMenu 与日期浮层共用。**v0.8.7 终裁**：下方放得下 → 左上角贴锚点；放不下 → 翻上、下边缘贴锚点（`mirrorXOnFlip` 时右下角贴锚点）；**没有「下方限高」这一档**；量高 `Math.max(rect.height / scale, scrollHeight)`。

### v0.8.7 新增/改动的前端点

- `src/lib/colorPreview.ts`：新增 `SYSTEM_VIEW_IDS` 与 `dueColorsForCard(preview, nodeId, viewId, stored, defaults)` —— 系统视图的**双回退**（自己条目优先、没配过才跟视图键），`TaskCard` 用它取值（单测 5 条）。
- `src/lib/searchScan.ts`：`stampOf` 改 `Date.parse` 数值（脏数据退 0）；中间批 `hits.slice(0, SEARCH_HIT_LIMIT)` 封顶。
- `src/lib/diary.ts::diaryByDate`：天内排序同样走 `Date.parse`（`createdStamp`）。
- `src/lib/styles.ts`：新增 `PAGE_HEADER_ICON_SIZE = 34`（页面头部图标唯一口径）；`toolboxAccent` / `toolboxBackground` 多一个可选 `toolId` 参数（两层回退：工具 → 主界面 → 默认）。
- `src/lib/defaults.ts`：`toolbox.toolAccents` / `toolbox.toolBackgrounds` 默认 `{}` 并归一（`normalizeHexColorMap`，与 `appearance.uiColors` 共用一份）；`transfer.relay` 的默认值从 `""` 改成 `"default"`（n0 公共 relay）。
- `src/lib/ColorDraftActions.svelte` **已删除**（确认统一进色盘面板）；`.color-draft-actions` 的 CSS 一并清掉。
- `src/lib/menu/MenuItem.svelte`：`checkable` prop —— 勾选标记住在**最左侧的定宽槽位**（未选中 `visibility: hidden`，仍占位），菜单宽度不随选中项变化。
- `src/lib/menu/ContextMenu.svelte`：`mirrorXOnFlip`（默认 `true`）。

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
