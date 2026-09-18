# UI 布局与特性

> **这份文件是什么**：原 AGENTS.md「UI 布局与特性」章的**全部界面细节**——整体布局、任务卡片、列表分区与排序、树与图标选择器、⋯ 列表菜单、定时任务、工具箱、我的一天、日记、全局搜索混排、markdown 扩展渲染、输入框加号与编辑器元数据行、设置抽屉、Linux 桌面、移动端（Android）、编辑器 markdown 工具栏、一般卡片的 Markdown 压缩包、首帧缩放、返回键拦截器、幽灵点击与浮层层级、安卓退出生命周期。
> **什么时候读它**：改任何 Svelte 组件、改任何界面行为或手势、加/改右键与三点菜单、改设置抽屉、改移动端交互、改 markdown 渲染扩展。
> **不在这里**：记账域（→ `ledger.md`）、同步功能总开关与设置页同步卡片（→ `sync.md`）、前端模块职责与 CSS 级联/按钮规范（→ `frontend.md`）、v0.7.5–v0.7.8 的逐版打磨（→ `history/v0.7.5-v0.7.8.md`）。

## 目录

- [布局](#布局)
- [任务卡片](#任务卡片)
- [列表分区与排序（v0.6.3，v0.6.4 调整）](#列表分区与排序v063v064-调整)
- [树](#树)
- [⋯ 列表菜单](#-列表菜单)
- [定时任务](#定时任务)
- [工具箱（v0.7.5 起两端都有，注册表架构；v0.8.3 拆目录 + 可固定）](#工具箱v075-起两端都有注册表架构v083-拆目录--可固定)
- [日期与提醒面板（v0.8.3，`TaskDateReminderPanel.svelte`）](#日期与提醒面板v083taskdatereminderpanelsvelte)
- [色盘与取色器的统一纪律（v0.8.3）](#色盘与取色器的统一纪律v083)
- [我的一天](#我的一天)
- [日记（v0.6.5 引入，v0.6.6 拆文件 + 导入导出，`DiaryView.svelte` + `diary/`）](#日记v065-引入v066-拆文件--导入导出diaryviewsvelte--diary)
  - [头部是齿轮（v0.6.6）](#头部是齿轮v066)
  - [导入导出是给人看的 Markdown 树](#导入导出是给人看的-markdown-树)
  - [卡片与编辑器](#卡片与编辑器)
- [全局搜索混排（v0.6.6，v0.7.6 加记账）](#全局搜索混排v066v076-加记账)
- [markdown 扩展渲染（v0.6.8，`markdown.ts` + `markdownControls.ts` + `markdown-ext.css`）](#markdown-扩展渲染v068markdownts--markdowncontrolsts--markdown-extcss)
- [输入框加号与任务编辑器元数据行（v0.6.6）](#输入框加号与任务编辑器元数据行v066)
- [设置抽屉](#设置抽屉)
- [Linux 桌面](#linux-桌面)
- [移动端（Android）](#移动端android)
- [编辑器 markdown 工具栏（v0.6.9 移动端引入，v0.6.10 上桌面，`editor/MarkdownToolbar.svelte`）](#编辑器-markdown-工具栏v069-移动端引入v0610-上桌面editormarkdowntoolbarsvelte)
- [一般卡片的 Markdown 压缩包（v0.6.10，v0.7.2 收紧图片，`cards_archive.rs` + `task.exportMarkdown`/`task.importMarkdown`）](#一般卡片的-markdown-压缩包v0610v072-收紧图片cards_archivers--taskexportmarkdowntaskimportmarkdown)
- [首帧缩放不跳变（v0.6.10）](#首帧缩放不跳变v0610)
- [返回键拦截器注册表（v0.6.9，`platform.ts` 的 `addBackInterceptor`）](#返回键拦截器注册表v069platformts-的-addbackinterceptor)
- [幽灵点击与浮层层级（v0.7.3，`ghostClick.ts`）](#幽灵点击与浮层层级v073ghostclickts)
- [安卓退出生命周期（v0.6.9，MainActivity）](#安卓退出生命周期v069mainactivity)

## 正文

（下面是原章节的**全部条目，保持原序**；每条从列表项提升为小节，文字一字未改。「记账」与「同步功能总开关」两条已分别搬到 `ledger.md` 与 `sync.md`。）

### 布局

标题栏 + 左侧栏（#f0f0f0 一体化背景）+ 工作区（左上角 12px 圆角）。左侧栏 = 个人资料卡、搜索、系统导航（我的一天/计划内/收藏/**日记**/定时任务）、自定义树、底部新建；**固定导航 v0.7.3 起是一份数据**（`Sidebar.svelte` 的 `navRows`：四个系统节点 + 日记/记账/工具箱三条非节点行合成一张表），显示哪些、什么顺序由 `appearance.navItems` 决定（id 名单在 `nav.ts`，未知 id 在 core 侧就被拒），展示方式由 `appearance.navLayout` 决定：`list` 单列图标+名称（默认）/ `grid` 双列（`.system-nav.nav-grid`）/ `icons` 单行只图标、名称进 title、角标缩成右上角小点（`.system-nav.nav-icons`）；两者都是**本机偏好不进同步**。定时任务仍按 `caps` 在渲染时过滤掉（移动端没有调度引擎）；工具箱 v0.7.5 起两端都有（`caps.toolbox` 恒真，过滤机制保留给未来平台）。**日记不是节点**，是显式排在「收藏」下面的一行（移动端「定时任务」隐藏，于是正好在「工具箱」上面）。主区域由 Workspace 或 DiaryView 之一占据：桌面 `stores.diaryOpen`，移动端 `mobileView === "diary"`（历史栈一层）；被顶掉的那个只是 CSS 隐藏、组件仍挂着，于是日记编辑器里的链接照样用得上 Workspace 的内置预览浮层。工作区 = 列表头（标题可内联重命名；右侧 展开全部/收起全部/⋯ 菜单）、任务卡片流、底部添加栏。**标题与标题下小字的关系（v0.7.2 统一、v0.7.3 改对齐基准）**：小字（我的一天的日期、日记的「共 N 篇 · 本月 M 篇 · 连续 K 天」）统一走 shared.css 的 `.list-subtitle`——**左对齐到卡片左缘**（内容区左缘，v0.7.2 曾缩进 58px 对齐标题文字，用户要的是「对齐卡片起点」，于是缩进删掉）、与标题留约 10px 呼吸距离（桌面 `.list-header` margin-bottom 28px 配 -18px，移动端 18px 配 -8px）、字号 `calc(var(--font-control) - 3px)`（移动端 -4px）；记账的月份行不是小字而是**标题级**的 `.ledger-month-bar`（只有列表视图渲染），同样对齐卡片左缘。

### 任务卡片

单击选中复制文本；双击展开/收起（不选词）；右侧笔图标打开浮窗编辑器（编辑/预览切换、插图、Esc 或点外部保存关闭）；右键出菜单（日期/标签/emoji/移动/删除）。**折叠态标题的省略号规则（v0.6.3）**：桌面单行 nowrap，显示不全加省略号并可展开；移动端折行显示，**两行放得下就不夹行、不加省略号、不算可展开**，只有两行放不下才夹成两行并在第二行末尾加省略号（夹行与省略号都由 `.clamped` 类控制，类由卡片量出的 `titleOverflow` 决定——量的是**自然高度**与两行预算之比，夹行开着时 clientHeight 已被夹住量不出需要几行）。量标题用 `use:measureTitle` action + **ResizeObserver**：移动端首屏卡片在 view-list 下是 `display:none`，挂载时量到的全是 0，点进内容页变可见时只有 RO 能接到（window resize 事件不会发）。**移动端（v0.6.2 起）展开/折叠/编辑全部交给单击/双击/长按**：笔形编辑按钮与收起按钮连同它们占的网格列一并隐藏（`.task-card.expanded .task-body` 的 44px 预留也归零），展示区更宽；双击判定窗口 220ms（`DOUBLE_TAP_MS`，单击动作要等这个窗口过去才执行，太大单击发闷）。**标签/表情/日期在正文流里（v0.6.4，v0.6.6 改右对齐）**：它们不再各占一个 grid 列（空列 + gap 会在卡片右侧留一条死空白），折叠态 `.task-body` 是一行 flex：标题 `flex:1 1 0%` + `min-width:0` 吃掉剩余宽度、把标签/日期那一簇**挤到行尾**（`.task-tags` 用 `justify-content:flex-end`，多行时每行也右对齐）。**不能用 `margin-left:auto`**——auto margin 与 `flex-grow` 抢同一份剩余空间，两者同时存在时标签会停在中间。右缘在两端自动不同：桌面 grid 是 `[勾选, 正文, 编辑]`，正文列右缘就是笔形按钮左侧；移动端隐藏了编辑列，于是直接贴卡片右内边距——**不要写死偏移量**。标签太多时容器自己收缩（上限 68% 宽），永远不会把标题挤到 0，也不会浮在文字上方。展开态不变：标签仍占最后一行、左对齐。空标签容器 `:empty` 不占位。**一般卡片（v0.6.7 下掉角标）**：v0.6.4 曾在一般卡片左上角画一个主题色 +/- 小角标当展开折叠把手，v0.6.7 按用户反馈完全移除（`.card-corner` 与 `.task-card.plain` 的额外左内边距一并删除）——展开/折叠仍由桌面双击、移动端单击与展开态的收起按钮承担；一般卡片照旧不画已完成删除线/灰字（它没有「已完成」语义）。**展开态只认存储值（v0.6.8）**：`isExpanded = task.expanded === true`，`canExpand`（多行或标题溢出，量出来的易失值）只门控手势与按钮——此前拿它门控渲染，列表增减导致滚动条出现/消失、宽度一变标题溢出判定翻转，存储态为 expanded 的卡片就会在「完成/删除/新增别的任务」时自己弹开。编辑器保存正文也不再按行数强制展开（保留当前展开态）。**标签/表情两段式点按（v0.6.9）**：触屏第一下点标签/表情只露红叉（`.reveal-delete`），第二下点文字才编辑/换表情、点红叉才删除；桌面不变（hover 露叉、点文字直接编辑）。配套铁律：**红叉隐藏态必须 `pointer-events: none`**（workspace.css 的 `.task-tag/.task-emoji-badge .tag-delete`，hover 与 `.reveal-delete` 才恢复 auto）——红叉缩在角上（top/right -7px），隐藏但可点的话触屏第一下往往正落在它上面，表现成「点一下标签直接删除」。卡片与两个编辑器共用这套语义（编辑器里第二下点文字进内联编辑输入框）。**v0.8.0 的性能口径**：卡片的 `fullHtml` **只在展开时渲染**（`$: fullHtml = isExpanded ? renderMarkdown(...) : ""`——Svelte 的 `$:` 是急切求值、与模板消不消费无关，折叠态卡片原来也白跑一遍完整 markdown 渲染，一屏 300 张折叠卡就是 300 次白渲染）；`resolvedMd` 仍 eagerly 算（它负责触发插图预加载）；量标题的 `measureTitle` 走 `measureBus.ts::observeResize`（全应用共享一个 ResizeObserver + 一个 window resize 监听 + rAF 合帧，**别再每卡自己 new 一个 RO**）；markdown 渲染本身带 block/inline 两套 LRU 记忆化（`markdown.ts`，命中率挂在 `window.__kxtodoRenderStats`，详见 `frontend.md`）。

### 列表分区与排序（v0.6.3，v0.6.4 调整）

「已完成」区固定按完成时间降序（最新完成在最上），三点菜单的排序方式只作用于未完成部分；「计划内」选「全部」时按互斥分区渲染（已逾期/今天/近三天=明天~+2/本周剩余/稍后，**今天独立成桶**、空分区不出现，`plannedGroups.plannedSections`），分区标题是**可折叠按钮**（chevron 旋转、带条数，折叠态本地 UI 状态不持久化）；分区标题与「计划内」分组标签用**半透明淡白**（`rgb(255 255 255 / 50%)`、hover `70%` + 灰字）——比白色卡片（`84%`）更透一档，读作卡片的浅色同类；早前的淡灰半透明（`rgb(146 152 160 / 14%)`）在白卡片旁显得脏，已换掉。**卡片类型**（列表菜单「卡片类型」→ Todo卡片/一般卡片，node 的 `cardStyle` 字段，core `task.modify --cardStyle`）：一般卡片隐藏勾选框、内容占满第一列（`.task-card.plain`），且**不分区已完成**——已完成的当普通卡片混在主列表里；纯渲染差异，任务的增删改查/完成状态/手势一律不变。系统视图（我的一天/计划内/搜索）里每张卡片按**自己所属条目**的 cardStyle 渲染。**一般卡片不统计角标（v0.7.2）**：`nodes.buildListCounts` 把 cardStyle 为一般卡片的条目及其任务排除在未完成计数外（分组折叠时的汇总同样排除）——它没有「未完成」语义，角标挂着只会误导。**「已完成」区默认折叠（v0.6.11）**：用户展开/收起后按当前视图记住（localStorage `kxtodo-completed-open`，键 = 条目 id / `planned`，本机 UI 状态不进同步）；换视图回读，没配置过就是折叠。

### 树

分类可折叠、图标可选（Lucide/emoji）、指针拖拽排序（before/after/inside 指示、边缘自动滚动、悬停展开、Esc 取消、拖到空白落根级末尾）、右键菜单、空白区右键新建。**图标选择器 v0.7.5 复用记账图标目录**（`IconPicker.svelte`：简笔画 = `LEDGER_ICON_GROUPS` 20 组 228 个，分组 chips + 网格与记账管理器同款；选中值存 lucide 的 PascalCase 名，`IconGlyph` 加了 ledgerComponent 分支——旧 kebab 33 白名单照旧渲染、两边都不认识才当 emoji）；`emoji-picker-element` 的内部滚动条在 shadow DOM 里、外部样式表够不着，挂载后注入一段 style 藏掉；选择器桌面居中对话框、移动端贴底抽屉。**v0.7.7 重排**：顶部「常用图标」（最近用过的，`recentIcons.ts`，表情与简笔画混排、最多两行）→ 分组 chips + 简笔画网格（**固定五行高 238px、自己滚、不画滚动条**，两端一致）→ 「全部表情」；记账表单里的图标网格移动端也同样自己滚。任务「添加表情」与两个编辑器共用同一组件，一并生效。**v0.8.0 起 IconPicker 在 App/Sidebar 里 `{#await import(...)}` 动态加载**（它带着 emoji-picker-element，不进首屏 chunk）；`IconGlyph` 的 `KNOWN_ICONS` 白名单放在 `<script context="module">`——原来每个图标实例都重建一个 228 项的 Set，一屏几十个图标就是几十次。

### ⋯ 列表菜单

（ListMenu.svelte）：立即同步（同步已配对且未暂停时显示）、重命名、移动到分组、排序方式、卡片类型、删除、导出/导入、UI 颜色、背景（色盘 + 莫奈预设 + 图片 + 透明度）。**立即同步快捷键默认 F5**（`settings.shortcuts.syncNow`，可配置，App.svelte 的 `matchesShortcut` 分支处理）。**日记模式（v0.6.6 起）多一个「标签」子菜单（v0.6.8）**：已有标签可改名/删除、输入框 + 颜色圆点添加、清除所有，与任务菜单同一套交互，写入走 `updateDiaryEntry` 的 tags 整体替换。**拖动跟手（v0.6.8）**：日记模式的外观写入走 `config.set`——它等 IPC 往返回来才更新 store，滞后的回渲会把「已提交的旧值」写回 range/color input，透明度条不跟手、取色器跳变（条目页的 `setBackground` 同步改 store 所以没这病）；透明度条/取色器/背景链接输入在交互期间用本地草稿冻结（`opacityLive/uiColorLive/linkLive`），change/blur 后再跟随已提交值。

### 定时任务

独立视图，卡片三态（compact/expanded/editing），新建后停在编辑态（ui.editing 持久化）。触发器 once/interval/calendar/condition；动作 脚本/可执行文件/通知；执行历史 `schedule logs`。

**v0.8.3 起移动端也有这一页**（`caps.scheduler` 在移动端翻真，侧栏「定时任务」行随之出现）：移动端只允许 `Action::Notification`、不允许 `Trigger::Condition`（`ops_schedule.rs::ensure_action_supported`），因为脚本/可执行文件在 Android 上没有可执行的落点。**任务提醒不是定时任务**：它不建 `ScheduleEntry`、不进 `tasks.json`，只是借调度线程那 500ms 的节拍跑 `reminders::Engine::poll`（详见 `history/v0.8.3.md` 一）。

### 工具箱（v0.7.5 起两端都有，注册表架构；v0.8.3 拆目录 + 可固定）

**目录与注册表分开（v0.8.3）**：`tools/catalog.ts` 只有 `id/name/desc`（`TOOL_CATALOG` + `isToolId` + `toolCatalogEntry`），`tools/registry.ts` 才是「图标 + `available()` + `load()` 动态 import」的注册点。拆开的理由很具体：侧栏的固定行也要认得工具 id，让 `nav.ts` 直接 import registry 会把 lucide 图标与所有工具的 chunk 拉进首屏链。`tools/navigation.ts` 管 `tool:<id>` ↔ 固定行。

`ToolboxView.svelte` 只是壳：列表画 `availableTools()`、点开懒加载子视图（`{#await loadToolModule(tool)}`，模块缓存失败不留）——**新增工具 = 目录加一项 + 注册表加一项 + 写一个组件，壳与样式零改动**。v0.8.3 起卡片可**右键「固定此工具」**钉进侧栏固定区（`appearance.navItems` 里追加 `tool:<id>`，core 侧 `NAV_TOOL_IDS` 白名单校验）。v0.8.4 的三处变化：① **打开哪一个工具只有一处真源**（`tools/navigation.ts` 的 `toolRoute`，子视图从它纯派生）——壳里不再有 `activeToolId` 这种影子状态；② 子页头部去掉「返回工具箱」整行，改成右上角两枚按钮（左 = 回工具箱、右 = ⋯ 外观菜单，工具页的背景色/主题色走同一套「草稿 → 保存」）；③ **返回一律回工具箱主界面**（`fromPin` 已删）。固定区拖动排序的落点按布局算（单列比 Y、双列/图标在同一竖带内比 X），拖动时行实时让位（`animate:flip`）。现有工具：随机数（`RandomTool`）、人民币大小写（`RmbTool`，双向、宽输入框 `.toolbox-text-input-wide`、支持汉字常显 `.toolbox-han-hint`）、草稿纸（`ScratchpadTool`，纯 textarea、**不做任何渲染**；正文字数据域 `data.json` 的 `scratchpad`（跟着「同步数据」走），localStorage 只做首帧缓存，防抖 5 秒 + 切后台/关页面前 flush；图标是手绘的 `ScratchpadIcon.svelte`）、文件传输助手（`TransferTool`，见下）。

路由与记账同口径：桌面 `stores.toolboxOpen` + `.app-shell.toolbox-open` 藏工作区（搜索态让回）、移动端历史栈 `toolbox` 层；侧栏行与设置「固定分组」勾选由 `caps.toolbox`（恒真）过滤。共享样式在 `toolbox.css`（桌面容器限宽 720px），移动端整页容器覆盖仍在 mobile.css。

### 日期与提醒面板（v0.8.3，`TaskDateReminderPanel.svelte`）

**一个组件、三个入口**：右键菜单「日期与提醒」（原「添加日期」）、卡片上点日期或时刻（`.task-date-popover`）、编辑器工具栏的日期按钮（`.editor-meta-pop`）。入口只挂组件并给 `onSave`/`onClear`/`onClose`；`embedded` prop 决定退到顶时是关自己还是交还宿主。

自上而下：日历（`CalendarGrid.svelte`，与 `DatePicker` 共用一份实现）→ 时刻行（`Clock` 图标；无值时灰色占位「添加时间」，有值时显示时刻 + 清除叉）→ 提醒行（`Bell` 图标；占位「添加提醒」，已有一串胶囊、悬浮出叉，末尾加号 → 五项菜单）→ 底部「清除 / 保存」。

几条**刻意**的交互口径：① **点日期不关面板**——它是多项配置的表单，不是一次性选择器，只有清除/保存或点外面才关；② 时刻的双轨滚轮默认停在当前时刻，底部「清除 / 确认」**都回上一级**（一个抹值一个写值）；③ 添加提醒菜单里「截止前一小时 / 截止前五分钟」**只在设了分钟时刻时可用**；④「自定义」是二级视图，顶上「日期 / 时间」两个标签页；⑤ 返回键/左上角返回**逐级退**：加提醒小菜单 → 自定义（回日历）→ 双轨（回主视图）→ 关面板。

**日记不是这一套**（需求 10.6）：日记右键叫「修改日期」，只有日历、没有时刻行与提醒行（日记的时刻在 `createdAt` 里，不是提醒的依附对象），选完即把日记挪到那天。

### 色盘与取色器的统一纪律（v0.8.3）

**活值住在组件 state，只有 `change` 才写盘**：`<input type="color">` 的 `input` 事件在拖动过程里连发，逐次 `config.set` 会把 settings.json 的原子写打爆——用户看到的就是「自定义颜色保存失败：原子替换失败」。四处同一套写法（临期高亮四块、节点 UI 色、列表背景色、日记/记账外观）：`on:input` 只更新本地草稿（色块**实时**跟着变），`on:change`（松手）才提交一次。

**临期高亮是四档**（已过期 / 今天 / 明天 / 后天，默认灰红黄蓝），色盘在 ⋯ 列表菜单里是一行四个色块 + 「默认」按钮（`.due-color-row`，flex nowrap）。档数是跨语言常量，改它要同步四处——见 `frontend.md` 与 `tests/frontend_contract.rs`。

### 我的一天

标题下显示当天日期；灯泡 = 智能建议；日历 = 月/周视图回看各天完成项；已完成区只显示当天完成。

### 日记（v0.6.5 引入，v0.6.6 拆文件 + 导入导出，`DiaryView.svelte` + `diary/`）

侧栏「收藏」下面一行进入，与 task 平行的另一类内容——**不属于任何 entry/category**，没有完成状态，也不进角标计数（但**进全局搜索**，见下条）。数据住在**自己的 `diary.json`**（第四个领域文件，独立 revision / 幂等台账 / 埋碑 / 域事件），前端也单独一个 `stores.diaryEntries`（不塞进 appState）——写一篇日记不该抬高 data 域的 revision。三种视图由头部段控切换，选择持久化在 `settings.diary.view`（`list|calendar|group`，**本机偏好，不进同步共享子集**）：**列表** = 年份分隔行 + 卡片（左侧日期栏「日/月」+ 竖线）；**日历** = 居中月历（有日记的格子标心情 emoji，没心情标圆点，多篇再加角标计数），点格子在下方列出当天卡片；**分组** = 年→月两级可折叠（折叠态本机不持久化）。三种视图右下角同一个浮动 + 按钮：**列表/分组写今天，日历写选中的那天**；「今」按钮的条件不只看选中日——**日历翻到别的月份也要出现**（选中日可能还是今天，但画面已不在今天这一屏）。

#### 头部是齿轮（v0.6.6）

与其它页面一致，齿轮下拉面板里是「搜索日记」与「日记菜单」两项；三点菜单复用 `ListMenu`（`diaryMode` + `background`/`accentColor` 覆盖传参），于是 UI 主题色、背景色盘/预设/图片/透明度、立即同步全部白拿，只把节点专属项（重命名/移动/排序/卡片类型/删除/JSON 导入导出）换掉。日记的主题色与背景存在 `settings.diary`（`accent`/`backgroundColor`/`backgroundImage`/`backgroundOpacity`），**这四项进同步共享子集**（外观该多端一致，与条目背景/uiColors 一个待遇），`view` 刻意不进。

#### 导入导出是给人看的 Markdown 树

（`diary_archive.rs`，zip crate 只开 deflate 保持纯 Rust）：`年/月/YYYYMMDD[_序号][_标题].md`，一天多篇才带序号，没标题就只用日期；`title/date/time/tags/mood/weather/createdAt` 写在 YAML front-matter（空字段不写）。标题过滤掉路径分隔符与 Windows 保留字符、反复剥前导点（「.. .. x」这种点与空格交错的要循环剥），撞名拿 id 尾巴区分。导入反向很宽容：没 front-matter 的手写 md 也能进（日期退回文件名/目录，2 月 30 日这种要拒）、非 UTF-8 按 lossy 解码、无效条目跳过并计入 `skipped`；**全程在内存解压，不落文件系统，所以没有 zip-slip**，但有解压炸弹护栏（200MB/2万文件/单文件 20MB）。**同一天已有日记不算冲突**：直接追加成另一篇，不合并正文也不去重（所以同一个包导两遍会得到两份——导入因此走确认门）。桌面另存为对话框、导入走原生打开；Android 的 dialog save() 只给 content:// URI，所以导出不传 path、让 Rust 落进缓存目录再由 Kotlin `shareFile` 桥拉起分享面板，导入走隐藏 file input + **base64**（不用 Vec<u8>：JSON IPC 会把 1MB 摊成一百万个数字）。**插图随包走（v0.6.7）**：导出时正文里读得到的本地图（裸文件名）改写成 `images/<文件名>` 的包内相对路径（解压出来 年/月 与 images/ 同级，直接能看），字节以 Stored 进包（图片已压缩，二次 Deflate 只烧 CPU）；导入**两遍扫描**（先收 images/ 再解析 md——包内 images/ 排在 md 之后，单遍扫描引用归一永远看不到图），引用归一回裸文件名、字节落回 `img/data/diary/`，**已存在的同名文件不覆盖**（图片是内容寻址的不可变 blob，重导必须幂等），落盘张数进返回值 `images`。解析在 **core 侧**完成（`diary.import` 收 `zipBase64`，CLI 与 GUI 桥都只递字节）：插图要和 entries 一起拿到才能落盘。远程链接与 `data:` 内联图永远不碰；包里没有的引用原样保留；包内文件名落盘前过 `is_safe_image_name`（无分隔符/`..`/控制字符/长度有界）。

#### 卡片与编辑器

手势与任务卡片同一套（桌面双击展开、移动端单击展开双击进编辑器、右键/长按出菜单）；菜单抽成 `diary/DiaryEntryMenu.svelte`（日记视图与全局搜索结果共用一份）。编辑器 `diary/DiaryEditor.svelte` 动态 import 挂 App 层，**新建时标题与正文都空就不落盘**（空草稿关掉即消失）；元数据行的日期/心情/天气/标签浮层**用捕获阶段的 pointerdown+focusin 关**——对话框对 pointerdown/click 做了 stopPropagation，冒泡阶段的 window 监听根本收不到里面的点击；Esc 是两段式（先收浮层再关编辑器）。**移动端编辑器元数据行只留图标（v0.6.11）**：心情/天气/标签触发器的占位文字包在 `.trigger-label` 里，mobile.css 隐藏它（已选的值保留：表情/天气图标本身、标签 chip）；桌面保持图标+文字现状。**安卓上卡片不能有点按蓝罩**：WebView 默认给可点元素补一层 tap highlight，单击是展开、双击是编辑，罩子看起来像「卡片被选中了」——`.diary-view` 内的可点元素一律 `-webkit-tap-highlight-color: transparent`。插图复用现成的按条目分目录通道，伪条目 id = `diary`（`diary.ts` 的 `DIARY_IMAGE_NODE`），图片存储与同步一行都不用改。头部副标题给「共 N 篇 · 本月 M 篇 · 连续 K 天」（连续天数今天还没写不算断，从昨天接着数）。**长单行也要可展开（v0.6.9，与 todo 卡片统一口径）**：单行但折行超过两行的正文、单行显示不全的标题都算可展开——字符数阈值（摘要 180 字截断）识别不了「没截断但两行放不下」，得量：`measureExcerpt` 临时摘掉 `-webkit-line-clamp` 量自然高度比两行预算、`measureTitle` 比 scrollWidth/clientWidth，都配 ResizeObserver（v0.8.0 起统一走 `measureBus.ts::observeResize` 的共享 RO）；展开态同样只认存储值 `entry.expanded === true`（v0.6.8 在 todo 卡片修过的病，日记卡片当时漏了）。**DiaryCard 的 `fullHtml` 同 TaskCard 口径：只在展开时渲染**（v0.8.0，`$:` 是急切求值，折叠卡不该白跑完整 markdown 渲染）。

### 全局搜索混排（v0.6.6，v0.7.6 加记账）

搜索不再只找任务——`nodes.buildSearchHits` 把任务（含已完成）、日记与**记账（v0.7.6：`ledger.filterLedgerEntries` 匹配分类/备注/金额）**合成一条 `SearchHit` union（`kind: task | diary | ledger`），按「最近改动」降序，匹配规则复用各自那条（任务 `buildVisibleTasks`、日记 `diary.filterDiaries`、记账 `ledger.filterLedgerEntries`）不另写一份；记账命中渲染成 `LedgerEntryCard`（单条卡片，点击打开记账面板改那一笔），桌面工作区与移动端结果面板都是同一份数据、同一张卡；每条任务的 `cardStyle` 按**它自己所属条目**取（搜索结果混着多个条目，不能拿全局 accent）。渲染分两处：桌面在 `Workspace` 里把原来的 taskRows + 已完成分区**整体换成一条混排列表**（搜索态不再分「已完成」折叠区，排序就是相关性）；**移动端单独一个 `SearchResults.svelte` 挂在侧栏搜索框下方**——搜索结果本来只在工作区渲染，而移动端首屏是列表视图、`.view-list .workspace` 是 `display:none`，于是安卓上搜什么都「没反应」。搜索态用 `.sidebar.searching` 把导航/树/底栏收起来，把剩下的高度交给结果面板；面板里的卡片接线全部接了（展开/编辑/勾选/日期/标签/表情/菜单），**不留按了没反应的死控件**；菜单多一项「打开所在列表/在日记中打开」（跳转时清搜索词）；链接直接交系统浏览器，因为应用内预览浮层长在不渲染的工作区里。**v0.6.7 三处修复**：① 桌面右侧停在日记页时搜索没有落点（工作区被 `.diary-open` 藏了）——app-shell 加 `searching` 类，`.app-shell.diary-open.searching` 把工作区换回来、日记藏起来，清空搜索词即回日记；② 移动端搜索态在历史栈上占一层 `{mv:"search"}`（`platform.ts`），安卓返回键经 popstate 落到进入搜索前那一层并清词——没有这层的话搜索结果页吃不到返回信号，多按一下就 finish() 退出应用；程序化跳转用 `dropSearchLayer()`（replaceState 原地撤层），否则异步 back 会和随后的 push 抢栈；订阅的**初始触发不许 back**（上次会话残留的栈顶 + 空词，挂载期间动历史栈会和 WebView 初始化抢）；③ 结果面板挂在侧栏里拿不到工作区内联的 `--accent`，`var(--accent)` 画的勾选圆圈/日期栏会整个消失——每条结果包一层 `.search-hit` 自带所属条目的主题色（日记用日记主题色），与 cardStyle 按条目取是同一条道理。**v0.8.0 起搜索输入经 `debouncedSearchQuery`（180ms 防抖，空串立即生效、首次发射直通）**，`visibleTasks`/`isSearching`/`searchHits` 消费的都是防抖值——每敲一个键就全量混排搜索（任务+日记+记账）曾是交互耗时大头。

### markdown 扩展渲染（v0.6.8，`markdown.ts` + `markdownControls.ts` + `markdown-ext.css`）

全部本地、不走 CDN。① **callout** = GitHub 告警语法 `> [!type] 标题`，marked block 扩展实现，类型/配色/头部图标一比一复刻 ksimple（`admonitionIcons.ts` 由 ksimple 的 FA 图标 svg 提取生成，改图标别手改这个文件）；② **数学公式** KaTeX 静态引入（`$...$` 内联、`$$...$$` 块级；先摘代码再摘数学再还原代码，sanitize 之后才回填 KaTeX HTML，代码里的 `$` 不会被吃）；③ **mermaid / markmap** 代码块渲染成图：渲染阶段只产出图框占位（源码 **base64** 进 `data-source`），`markdownWire` action 挂载后用 MutationObserver 盯容器子树异步填图（动态 import，chunk 按需加载）；④ **代码块**带语言条 + 复制 + 折叠（>18 行默认折起）；⑤ **front-matter** 渲染成键值表，不再露两行 `---`；⑥ 段落/列表链接前加链环图标（CSS data-URI）。交互（缩放/平移/全屏/源码切换/复制）全部走 `markdownWire` 的事件委托，组件只需在渲染 markdown 的容器上 `use:markdownWire`（TaskCard/DiaryCard/两个编辑器预览）。**三个坑**：DOMPurify 会剥掉值里含 `-->` 的属性（mermaid 箭头必中）→ 源码必须 base64 进属性；无参 Svelte action 的 `update()` 不会被调用，`{@html}` 重渲换掉 canvas 后只有 MutationObserver 能接住；dev 下 mermaid 的 esm 入口靠运行时动态 import 自己的 chunks，vite 边跑边优化会整页 reload/404（图永远「渲染中」），`vite.config.ts` 的 `optimizeDeps.include: ["mermaid"]` 不能删。**v0.6.9 扩充**：```mindmap 与 ```markmap 都渲染成 markmap（mermaid 自带的 mindmap 图类型让位；语言名单 `DIAGRAM_LANGS` 是 transformDiagrams 正则与 transformCodeBlocks 跳过判定的唯一来源，加语言只改这一处）；`:emoji:` 短码走 gemoji 本地表（`nameToEmoji`，打包约 +250KB，只认真表里的名字，代码与公式已摘走不会误伤）；公式定界符扩到 `$..$`/`$ .. $`/`\(..\)` 内联与 `$$..$$`/`\[..\]` 块级（货币反例靠「两侧空白状态不一致才拒」+ 闭合 `$` 后不跟数字挡住），块级 KaTeX 用 **span** 承载（`$$..$$` 写在段落中间时 div 会把 `<p>` 截断——restoreMath 在 sanitize 之后回填，DOMPurify 管不到这里的标签合法性）；GFM 任务列表 `- [ ]/- [x]` 去列表圆点只留 checkbox（`li:has(> input[type=checkbox])` + accent-color）。**v0.6.9 四个坑**：① base.css 给折叠箭头用的通用 `.collapsed { transform: rotate(-90deg) }` 是全局类名，代码块折叠态也叫 `collapsed` → 整块代码被转 90°「竖起来」，规则已限定 `svg.collapsed`，**新代码别再拿 `collapsed` 当状态类名**；② 全屏浮层的 Esc 必须 preventDefault+stopPropagation，否则编辑器的 window keydown 接着把编辑器也关掉（两段式 Esc 的第一段被全屏吃掉后第二段落给编辑器）；③ markmap 全屏不能克隆 DOM——d3 的缩放/平移监听挂在原 svg 上，cloneNode 不带监听，全屏里要拿 `data-source` 重建 view；④ 移动端图是**捏合缩放**不是滚轮：`markdownControls` 用双指针距离比算 scale、质心位移算平移（markmap 交给 d3 自己的 touch 处理）。拖拽图/插图期间 `body.kx-dragging` 禁选文本（指针在图上滑动会顺手框选周围的字）。**分割线（v0.6.11）**：github-markdown-css 的 hr 是 0.25em 灰色实心带，太粗太突兀，markdown-ext.css 覆盖成 1px 灰色细虚线。

### 输入框加号与任务编辑器元数据行（v0.6.6）

「添加事项」左侧的加号是真按钮（`.composer-plus`，观感不变、hover 放大 + 淡底），点它直接开编辑器新建一条；输入框本身的 Enter 流程不变。编辑器因此有了**新建模式**（`stores.editorDraftNode` 存归属条目 id，与 `editorTaskId` 互斥且共用同一层移动端历史栈）：`task.add` 拒绝空 markdown，所以不能先建后编，改成**保存时才创建、空正文关掉即消失**（与日记同一条规则）。`MarkdownEditorModal` 也加了与日记同款的元数据行（日期/表情/标签，样式在 editor.css 的 `.editor-meta*` 一套两边共用）；**日期不设默认值**，填了就按卡片菜单「添加日期」的语义同时写 dueDate+plannedDate（等于今天时顺带进我的一天）。两个编辑器的浮层都是**捕获阶段监听 + 两段式 Esc**，且 `onDestroy` 只落盘不回调 onClose（组件正在拆）。

### 设置抽屉

**v0.7.2 起每个大类是可折叠分区**（`SettingsSection.svelte`：标题行 = 折叠按钮 + 右侧 `actions` 具名 slot——数据同步的配对历史图标挂在那儿，动作按钮不能嵌进折叠按钮里；折叠状态记 localStorage `kxtodo-settings-section:<key>`，本机 UI 状态不进同步）。分区（v0.7.3 重组）：个人资料（头像 dataURL）、**外观效果**（字号六项走两列紧凑网格：UI 字号/正文字号/记账字号/日记字号/编辑器字号/标签字号；缩放与编辑器尺寸；**固定分组**：`appearance.navItems` 勾选哪些行 + `appearance.navLayout` 单列/双列/只图标；**新建分组默认外观**并入此分区）、特性开关、**窗口与系统**（链接打开 v0.7.3 从外观搬进来；关闭到托盘/开机自启仍由 `caps.trayLifecycle` 门控在分区**内部**，于是移动端也渲染这个分区、链接打开在手机上照样能配）、快捷键、数据同步、**存储空间**（v0.7.3：`storage.usage` 盘点 + 「释放空间」按钮调 `storage.clean`，清理后 toast 报释放了多少；浏览器预览没有数据目录，只显示说明）、关于与更新。**特性开关区（v0.6.11）是一整块灰色卡片**（`.settings-card`，与同步配置同观感）里三行开关列表，行下不再放注释——说明全部进勾选框的 `title` 悬浮提示。**编辑器尺寸比例（v0.6.9，桌面专属）**：`appearance.editorWidthPercent/editorHeightPercent`（30-100，默认 72/86）以**内联百分比**作用在 `.editor-dialog` 上——百分比相对 app-shell，窗口全屏/改尺寸时比例不变、尺寸跟着变；移动端固定铺满不给内联值（否则内联样式会压过 mobile.css 的 100%）。配置面六处同步：model.rs 的 AppearanceSettings（含 NewNodeDefaults）+ ops_config.rs 的 KNOWN_FIELDS/get/set/set_default + merge.rs 共享子集 + defaults.ts 归一 + types.ts + SettingsDrawer。**v0.8.0 起这六个字号真正管住全应用**：CSS 里除 `base.css` 的 5 个 `--*-font-size` 变量定义（变量本身，运行时由 `styles.ts::fontVars` 内联覆盖）外**不再有任何 `font-size: Npx`**——100 多处写死字号全部收编成 `calc(var(--font-*) ± N)` 或 `var(--font-*)`，默认设置下逐像素等价；基变量按区域选（sidebar/mobile/titlebar/editor/markdown-ext/settings/toolbox/workspace 走 `--font-control`，settings.css 用等价的 `--ui-font-size`；ledger.css 走 `--font-ledger`；diary.css 走 `--font-diary`）；顺带删掉了无消费者的死自定义属性 `--category-font-size`。**唯一刻意保留 px 的是 `ledger/AssetsTrend.svelte` 的内联 `axisFont`**：SVG 整体按 viewBox 缩放，`axisFont` 是按容器宽度**补**字号的，用 CSS 变量会被二次缩放。**v0.7.4**：存储空间「数据占用」下面列出占用组成（插图 / 背景与头像 / 服务器日志 / 备份 / 数据与运行时——末项是总占用减前几项的余桶，域 JSON、同步水位与服务器库都归它）；`.settings-segmented` 修成带边框的等分滑块（早前作为 grid 子项被 stretch 成一条长灰条、按钮挤在左端，按钮 `flex:1 1 0` 均分才对）；外观效果里 `.settings-block + .settings-block` 加淡虚线分隔、块标签加粗，字号/缩放/固定分组/默认外观不再连成一片。

### Linux 桌面

与 Windows 同一套现代 UI（`decorations:false` + 自绘 TitleBar，不换原生窗口装饰）；通知走 tauri-plugin-notification 系统通知（libnotify/D-Bus，`capabilities/linux.json` 授权），不自绘通知窗（客户端绝对定位在 Wayland 下不可行）；关闭按钮默认**退出应用**而非隐藏到托盘（WSLg/GNOME 托盘常不可见，隐藏到看不见的托盘等于把应用弄丢；设置里仍可显式开启 close-to-tray）；**托盘缺库降级（v0.6.3）**：缺 ayatana-appindicator3/appindicator3 动态库时 `TrayIconBuilder::build` 会失败——`setup_tray` 的失败**不再上抛**（曾经直接把整个程序弄崩），只记日志并把 `LifecycleState.tray_available` 置 false，应用以无托盘模式照常跑；关闭主窗口时 `close_to_tray && tray_available` 才隐藏，否则退出（设置里写着退到托盘也回落退出）。前端经 `tray_available` 命令查询：不可用时设置页把下拉固定成「直接退出应用」并说明原因，选「退到托盘」会被 toast 拦下。生命周期默认值 = 不开自启 + 关闭到托盘（Linux 首跑默认退出，理由同上），用户设置后以用户值为准；应用内更新与 Windows 同机制（`updateChannel = "desktop"`：下载固定名 `KXToDo.AppImage` + `kxtodo-cli` 到 `~/.local/share/kxtodo/bin/` 替换后尝试自动重启，拉起失败则提示手动重启）；全局快捷键受插件平台能力限制（X11 可用，纯 Wayland 抓不到），不做会话嗅探特判。

### 移动端（Android）

三级导航——主界面是分类列表（**v0.7.2 起行名比桌面大一档**，`--font-list + 2px`，站着单手也读得清），点分组展开、点条目/系统列表推入内容页；**左上角返回箭头（v0.7.2 删、v0.7.6 以特性开关回来）**：四个整页（Workspace 内容页 / DiaryView / LedgerView / ToolboxView）各自渲染 `<MobileBack />`（`MobileBack.svelte`），是否显示由 `features.mobileBack` 决定（**默认关**——安卓用户习惯系统返回键，常驻箭头只会把页面图标与标题推离卡片左缘），点击走 `platform.ts::goBackLevel()`（有历史层就 `history.back()`，与系统返回键同一条栈；没有层就什么都不做，列表页不渲染它）。设置抽屉仍保留自己的关闭入口（`.drawer-header .mobile-back`，与特性开关无关）。硬件返回键沿历史栈逐级回退（编辑器→内容→列表→退出，MainActivity 的 OnBackPressedCallback 驱动 webview.goBack）。**v0.6.8 返回键桥**：OnBackPressedCallback 先 `evaluateJavascript` 问 `window.kxtodoBackHandler()`（`platform.ts` 注册）：搜索态消费掉并清词，返回 false 才走 webview.goBack / finish——搜索态**不再往历史栈压层**，v0.6.7 的 `{mv:"search"}` 层方案撤销：pushState 条目会进 WebView 的会话恢复，「搜索过再退出、重开必闪退一次」就是从那来的（不压层后重开不再恢复出搜索栈）。**编辑器标签/表情删除叉（v0.6.8 多端统一）**：桌面 hover 露出；触屏（`platform.ts` 的 `touchOnly` = `matchMedia("(hover: none)")`）点一下标签露出、点别处收回（`.reveal-delete`），不再像 v0.6.7 那样移动端常驻一排红叉。长按 = 右键菜单（菜单期间卡片内禁止选中文本）；定时任务整体隐藏（无调度引擎）；通知走 tauri-plugin-notification 系统通知（首次发送请求 POST_NOTIFICATIONS）；更新 = 设置页检查 → 下载固定名 APK → Kotlin 桥（`window.kxtodoAndroid.installApk`）拉起系统安装器覆盖安装。导出走系统分享面板（文本走 shareText 桥，**二进制文件走 shareFile 桥**：Rust 先写进缓存目录，桥只负责拿路径换 FileProvider URI 拉 ACTION_SEND，与 installApk 同一套缓存目录穿越校验），图片选择走隐藏 file input + dataURL 命令。**手动同步的 toast 不带 P2P 设备名单**（v0.6.6）：手机上一条 toast 就那么宽，设备名会把真正的 ↑x ↓y 挤没；桌面保留名单（「同步成功但数据没动」需要这份证据）。**图像渲染（v0.5.0）走 dataURL 而非 asset 协议**：Android WebView 取不到 `http://asset.localhost/` 子资源，表现是 markdown 里只有 `![](…)` 语法、图不出（`caps.dataUrlImages` 覆盖移动端）。**编辑器全屏要留安全区**：`.app-shell.mobile .editor-overlay` 用 `env(safe-area-inset-*) × var(--safe-inv)` 做内边距，否则顶部编辑/预览切换与保存按钮会钻到系统状态栏底下（index.html 已带 `viewport-fit=cover`）。桌面体验不受影响（isMobile 只看 userAgent，能力门控保证桌面命令面不变）。**卡片交互（v0.6.2）**：移动端单击 = 展开/折叠、双击 = 进编辑器（单击动作延后一个双击窗口执行，两击判定同时看 `event.detail` 与时间间隔——WebView 合成 click 的 detail 不可靠）、长按 = 菜单；笔形编辑按钮与它占的网格列在移动端隐藏（展示区更宽，编辑走双击）；折叠态单行过长时折行显示、最多两行 + 省略号（`-webkit-line-clamp:2`，仍算折叠态）；**单行显示不全也算可展开**——卡片用 action 量折叠态标题是否被截断（`scrollWidth/scrollHeight` 比 client 尺寸），展开即把这一行显示完整，桌面双击/移动端单击/展开收起按钮同一条控制链（`expand` 事件带卡片量出来的目标态，Workspace 不再自己判多行）。**二级菜单钻入式（v0.6.2）**：窄屏上子面板贴右缘展开必然超屏，改成显示二级时隐藏一级（`.context-menu.sub-open`，一级项 display:none、子面板转行内静态），顶部一个「返回」收回一级；开合信号走 `menu/submenu.ts` 的模块级 store（slot 内容没有父子 prop 通道），ContextMenu 在移动端开合时重新收敛位置与限高。**内置链接预览（v0.6.2 标题栏，v0.6.8 两端统一标题栏）**：顶部一条「标签页」式标题栏（网页标题过长省略 + 右侧「在系统浏览器打开」+ 关闭 X），标题取链接文字、同源时 iframe load 后读 `document.title` 覆盖、再退主机名；移动端标题栏吃 `safe-area-inset-top`。v0.6.7 曾把桌面改成无标题栏全屏 + 鼠标上划感应带浮出圆钮，用户反馈还是标题栏顺手，v0.6.8 撤销（`.link-preview-hotzone/.link-preview-float` 已删）。**网站拒绝被框不是配置问题**——GitHub 等发 `X-Frame-Options: deny`（或 CSP `frame-ancestors`），Chromium/WebView2 对 iframe 强制执行且没有任何开关可关，iframe 路线天生打不开这类站，「在系统浏览器打开」按钮是唯一合理的出路。**下拉同步（v0.6.3）**：同步开启时列表滚到顶再下拉，过阈值松手跑一轮手动同步（`pullrefresh.ts` action，只在「顶 + 单指 + 下拽」窗口里 preventDefault，滚动/长按/单击双击不受影响），提示条高度跟手；齿轮面板同时有「立即同步」项。**钻入式二级菜单仅移动端（v0.6.3 修）**：`.context-menu.sub-open` 与「返回」按钮都只在移动端出现——桌面端二级菜单是绝对定位浮出面板，一级不隐藏也不需要返回（v0.6.2 因 `isMobile` 被当布尔用而误伤了桌面）。**菜单内输入不关菜单（v0.6.3 修）**：软键盘弹出会同时触发 window resize 与把输入框滚进可见区的 scroll，两条都撞在关菜单的信号上（安卓点标签输入框菜单就消失、点颜色圆圈没事）；ContextMenu 在菜单内有聚焦的 input/textarea/select 时：scroll 一律不关，resize 改成重新收敛位置与限高。**下拉刷新指示器（v0.6.4）**：文字提示条换成居中的圆形同步图标——随下拉进度旋转、过阈值变主题色、同步中持续自转（`.pull-sync-indicator`），高度仍跟手。**钻入式隐藏用兜底选择器（v0.6.4 修）**：`sub-open > *:not(.menu-item.has-submenu):not(.submenu-back)` 全隐藏——旧规则逐个列举 `.menu-item/.menu-separator/.menu-section-title`，漏了 ListMenu 下半部的裸 div/label/input（UI颜色行、色盘、背景链接、透明度、上传按钮），表现是「一半二级菜单一半一级内容」。

### 编辑器 markdown 工具栏（v0.6.9 移动端引入，v0.6.10 上桌面，`editor/MarkdownToolbar.svelte`）

元数据行（日期/表情/标签）下方一行，16 个图标按钮：图片/加粗/斜体/高亮/下划线/删除线/超链接/h1-h6/checkbox/无序/有序；编辑动作在 `codemirrorSetup.ts`（`wrapSelection` 包裹或拆包裹、`setHeading` 换级别且**光标落在「# 」之后**、`toggleLinePrefix` 行前缀开关、`insertLink` 选区落在 url 上）。**按钮 pointerdown 一律 preventDefault**——一抢焦点输入法就收下去了。按钮顺序：图片/加粗/斜体/高亮/下划线/删除线/超链接/**checkbox**/h1-h6/无序/有序（checkbox 在超链接与 h1 之间）；列表前缀按钮与标题同口径把光标落在前缀之后。**是否渲染由特性开关 `features.editorToolbar` 控制，桌面与移动端一致（v0.6.11 起移动端也受控）**。**输入法避让不能指望 adjustResize**（实测安卓 WebView 并不总是缩，工具栏会留在页面底部被盖住）：`imeInset` action 用 `visualViewport` 实测被吃掉的高度（`innerHeight - vv.height - vv.offsetTop`，WebView 真缩了差值为 0 不会重复补偿）给浮层垫 padding-bottom，视觉像素除回 uiScale。

### 一般卡片的 Markdown 压缩包（v0.6.10，v0.7.2 收紧图片，`cards_archive.rs` + `task.exportMarkdown`/`task.importMarkdown`）

条目 cardStyle 为一般卡片时三点菜单多「导出为 Markdown / 导入 Markdown 压缩包 / 导入 Markdown 文件夹」三项（文件夹项由 `caps.nativeFileDialogs` 门控在桌面——Android 没有目录选择器仍走压缩包）。包根下一堆 `<YYYYMMDD>_<正文前10字>.md`（撞名加序号）+ `images/<文件名>`（Stored）；导出把本地图引用改写成包内相对路径，导入两遍扫描（先图后 md）把引用归一回裸文件名、插图落回条目插图目录且**同名不覆盖**；网络图片不碰。**v0.7.2：图只跟引用走**——导出本来就只打包正文引用得到的图；导入（zip 与文件夹两条路共用 `finalize_cards`）只落**被任一 md 引用**的图，包/目录里没人指的图直接剔掉，孤儿图从源头不生。**v0.7.2：文件夹导入** = `task.importMarkdown` 的 `folderPath`（与 `zipBase64` 互斥），`parse_folder` 递归走目录、md 与图任意深度都收，同一套护栏与引用归一；Tauri 命令 `cards_import_folder`。护栏与日记压缩包同一套口径（200MB/2万文件/单文件20MB、文件名穿越、lossy 解码）。Tauri 命令 `cards_export_zip`/`cards_import_zip`/`cards_import_folder` 两个 invoke_handler 都要注册（桌面另存为/打开/选目录对话框，移动端缓存+分享桥与 file input base64）。**保存时清理无引用插图（v0.7.2 引入、v0.7.3 去掉宽限窗，`image_gc.rs`）**：task.add/modify/remove 与 diary add/modify/remove 写盘后扫对应插图目录，删掉没有任何 markdown 再引用的图。v0.7.2 曾有 5 分钟宽限窗防「在途图」，v0.7.3 按用户要求删掉——**真正的不变式是「清理只发生在本地写盘之后、且引用集就是刚写进去的那份 markdown」**：同步合并走 `repo::write_*`（`sync/engine.rs`），从不触发清理，所以「图片先到、实体后到」的在途图根本不会被扫到；宽限窗只是让孤儿图多活五分钟。IO 错误全吞、目录不存在直接返回 0，**清理永远不许弄失败保存本身**；开销 = 一次 read_dir + 扫描本来就在内存里的 markdown。**「释放空间」（v0.7.3，`ops_storage.rs`）**：`storage.usage` 只读盘点（插图/背景/头像各分「总量 + 无引用量」、临时文件、旧服务器日志、备份），`storage.clean`（high-risk-write，GUI 桥恒带 yes）删无引用插图与空目录、无引用背景/头像、根目录 `.*.tmp` 与 `img/` 下的 `.part`、以及内置服务器 `server/log/server-*.log` 里**除最近两份与今天之外**的旧日志；返回 `freedBytes` 与各类计数，单个 IO 失败进 `warnings` 不硬失败。**边界写死在模块文档里**：五个域 JSON、`runtime/*`（同步水位/设备密钥/配对历史）、服务器 `settings.json`/`data.db`（账户与 token 哈希住在库里）一概不碰。背景/头像的「无引用」判据 = `data.backgrounds[*].image` + `settings.{appearance.newNodeDefaults,diary,ledger}.backgroundImage` + `settings.profile.avatar` 里的 `img:<文件名>`。

### 首帧缩放不跳变（v0.6.10）

水合是异步的（安卓要等 core 读设置），第一帧按默认缩放渲染、设置到了再跳——观感「卡卡的、不稳定」。`defaults.ts` 的 `cachedAppearance()`/`writeAppearanceCache()` 把外观（缩放+四种字号）缓存进 localStorage，`stores.ts` 的 appSettings 初始值直接并入缓存，首帧即到位；设置每次变化写回缓存。**v0.6.11 补资料**：`cachedProfile()`/`writeProfileCache()` 同一条思路缓存 displayName/email/avatar（dataURL 超 1.5MB 不缓存），否则侧栏首帧会闪一下 Example User 与默认头像再跳成用户自己的。

### 返回键拦截器注册表（v0.6.9，`platform.ts` 的 `addBackInterceptor`）

不占历史栈的覆盖层（全屏图预览）注册「返回时先收自己」的回调，`kxtodoBackHandler` 先问拦截器（后注册先问）再问搜索态。全屏罩子不接返回键的表现是：返回把历史栈上的编辑器弹掉了，罩子却还留在原地——界面「卡住」，再退出再重开又撞下面的恢复链闪退。**全屏浮层要吃状态栏安全区**（mobile.css 的 `.app-shell.mobile .md-fullscreen-bar`）：浮层是 JS 建出来挂进 `.app-shell` 的（`markdownControls` 的 `ensureOverlay` 刻意不挂 body，否则拿不到 `--safe-inv`）。**这条安全区坑是第三次踩**（v0.6.8 编辑器全屏、v0.6.8 链接预览标题栏、v0.6.9 图全屏工具栏）——以后任何「JS 命令式建的全屏/浮层」都要先问一句：移动端顶部避让了吗？

### 幽灵点击与浮层层级（v0.7.3，`ghostClick.ts`）

移动端底部抽屉在 **pointerdown 阶段**就拆掉（点遮罩关闭、数字键盘保存键落盘），浏览器随后补发的那一个 click 会落在同一坐标上此刻最顶层的元素——记账保存键正下方是列表、右下角是「+」，于是「点遮罩却打开了别的卡片」「保存完编辑器又自己弹出来」（用户报的 R13 偶发复弹就是它；桌面看不出来：浮层还在原地时 click 已派发给按钮）。`suppressGhostClick({x, y})` 记下触发关闭的指针坐标，500ms 内只吞落在同一点 ±24px 的 click（**按坐标而不是按时间窗无条件吞**：早前无条件吞一个 click，结果「保存后紧接着点齿轮」被吃了）。**只在 pointerdown 驱动的关闭路径武装**——Esc / 返回键 / X 按钮（click 或 keydown 驱动）没有手势在飞，武装了反而吃用户下一次点击。**浮层层级**：从记账面板里唤出的分类/账户管理与钻取面板挂在 `.ledger-view` 里，`.ledger-view .editor-overlay` 抬到 z-index 4200 盖住 App 层的记账面板（4000）；两层的 window keydown 监听谁先注册谁先跑，所以**记账面板的 Esc 处理先让位**（`document.querySelector(".ledger-view .editor-overlay, .ledger-drill-pop")` 存在就直接 return），否则一下 Esc 把底下的面板关了、上面的管理器留着。分类/账户管理器也注册了 `addBackInterceptor`（两段式：表单→列表→关），否则安卓返回键会把底下的页面弹掉而浮层留在原地。

### 安卓退出生命周期（v0.6.9，MainActivity）

返回键退出走 `exitApp()` = `finishAndRemoveTask()` + 250ms 后 `Process.killProcess`。两个动机：① 任务从最近列表移除后，异常死亡不会留下带 savedInstanceState 的任务记录——wry 的恢复路径会拿旧 ACTIVITY_ID 找已经不存在的 webview，「搜索过/全屏过再退出、重开必闪退一次、第三次正常」就是这条恢复链（`onCreate` 因此传 `super.onCreate(null)`，本应用状态全在磁盘与 JS 侧，不需要 Activity 恢复）；② 移动端没有常驻 Host/调度/托盘，垂死进程攥着数据目录锁与同步端口正是重开撞锁白屏的窗口，硬退把窗口压到最小。配套加固：返回回调 400ms 去抖、`isFinishing` 守卫、`onDestroy` 置空 webViewRef。

## 搬去别处的两条（原章节里的位置）

- **记账**（原章节里排在「日记」之后、「全局搜索混排」之前）→ `ledger.md`（数据模型 + 命令面 + 界面约定 + Excel 归档 + 确认门）；它的 v0.7.3/v0.7.4 打磨 → `history/v0.7.0-v0.7.4.md`，v0.7.5–v0.7.8 打磨 → `history/v0.7.5-v0.7.8.md`。
- **同步功能总开关（v0.6.10，`features.sync`）**（原章节里排在「编辑器 markdown 工具栏」之后、「一般卡片的 Markdown 压缩包」之前）→ `sync.md` 的「同步功能总开关」。

## 文件传输助手（v0.8.4 重做，`TransferTool.svelte`）

形态照 LocalSend：内容列 `max-width: 720px` 居中（移动端单列）。自上而下：**身份区**（设备名输入 + 配对口令输入（带眼睛）+ 在线绿点/上线按钮）→ **居中分段滑块**（发送 | 接收）→ 发送侧四个大图标（文件 / 文件夹 / 文本 / 剪贴板；移动端 2×2）→ 已选清单（胶囊，× 可删）→ **单选的设备卡片**（名字 + 选中描边）→ 右下角**胶囊发送按钮**（移动端底部 sticky 避让安全区）；接收侧「待命接收中 + 保存位置 + 更改/打开文件夹 + 自动接收开关」、接收确认卡（对方名字 + 文件清单 + 总大小 + 拒绝/接收）、收到的文本卡片（复制 / 保存为 .txt）、`传输历史` 折叠区（近 50 条 + 常连设备数）。传输中/完成/失败的会话卡：方向图标 + 文件名 + 进度条 + 速度（EMA 平滑）+ 剩余时间 + 取消；完成 5 秒后自动收起，完成时系统通知 + Toast + 「打开文件夹」。空状态文案「对方输完同一句口令，就会出现在这里」+ 端到端加密小盾牌。

桌面拖文件/文件夹进窗口直接进清单（`getCurrentWebview().onDragDropEvent`）；移动端发送走 file input + base64 分片 spool；剪贴板是图片就按文件处理、是文本就进文本清单。
