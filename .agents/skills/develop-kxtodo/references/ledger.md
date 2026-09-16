# 记账域专项：数据模型 / 命令面 / 界面约定 / Excel 归档 / 确认门

> **这份文件是什么**：记账（ledger）这一域的**全部**内容——数据模型与不变式（整数分、余额推导、确定性种子 id）、四个视图、以「天」为组织单位的列表、浮层与编辑器语义、齿轮面板、图标目录、Excel 归档（导入导出）、CLI 确认门、kebab/camel 命令名映射、ledger.css 约定。
> **什么时候读它**：改 `crates/core/src/ops_ledger.rs` / `model.rs` 的 LedgerFile 家族 / `ledger_archive.rs` / `ledger_icons.rs`、改前端 `ledger.ts` 与 `LedgerView.svelte` 及 `ledger/` 下的组件、改 `ledger.css`、加记账命令、排查金额/余额/统计口径。
> **改记账要动哪些文件**（权威清单）见 SKILL.md 路由表的「改记账」行。
> **v0.7.3 / v0.7.4 / v0.7.5 / v0.7.6 / v0.7.7 / v0.7.8 的逐版打磨**（含条目两行卡片、编辑器圆形分类网格与内联二级搁板、统计双段控与结余三曲线、钻取独立配色、自定义账户类型与直设余额、资产趋势图、移动端浮层与滚动修正、条目多图、半屏抽屉、超链接渲染等）在 `history/v0.7.0-v0.7.4.md` 与 `history/v0.7.5-v0.7.8.md`——**改记账界面之前必须先读这两份**，当前界面的很多细节只在那里。**v0.8.0**（非功能版：单趟算法、前后端同口径修复、CLI stats 语义）见本文「前后端同口径与单趟算法」+ `history/v0.8.md`。

## 目录

- [记账（v0.7.0 引入、v0.7.1 卡片化重写、v0.7.2 打磨，`LedgerView.svelte` + `ledger/`）](#记账v070-引入v071-卡片化重写v072-打磨ledgerviewsvelte--ledger)
- [金额一律整数分](#金额一律整数分)
- [账户余额不存现值](#账户余额不存现值)
- [前后端同口径与单趟算法（v0.8.0）](#前后端同口径与单趟算法v080)
- [组织单位是「天」，与日记卡片同一条语言](#组织单位是天与日记卡片同一条语言)
- [所有浮层复用 `.editor-overlay` / `.editor-dialog` 那套语言](#所有浮层复用-editor-overlay--editor-dialog-那套语言)
- [v0.7.2 起只有显式点保存才落盘](#v072-起只有显式点保存才落盘)
- [v0.7.1 踩的三个坑](#v071-踩的三个坑)
- [Excel 归档](#excel-归档)
- [CLI 子命令名与 core 命令名的映射](#cli-子命令名与-core-命令名的映射)
- [ledger.css 的盒子 / 字号 / 按钮约定](#ledgercss-的盒子--字号--按钮约定)
- [CLI 改账本必须先过确认门（v0.7.2）](#cli-改账本必须先过确认门v072)
- [v0.7.3 打磨（① 时刻精确到分钟 … ⑦ 汇总卡金额不许出省略号）](history/v0.7.0-v0.7.4.md#v073-打磨)
- [v0.7.4 界面改写（① 条目两行卡片 … ⑩ 两个管理器都不 autofocus）](history/v0.7.0-v0.7.4.md#v074-界面改写)
- v0.7.5 / v0.7.6 / v0.7.7 / v0.7.8 界面与数据打磨 → `history/v0.7.5-v0.7.8.md`

## 数据模型、视图与界面约定

### 记账（v0.7.0 引入、v0.7.1 卡片化重写、v0.7.2 打磨，`LedgerView.svelte` + `ledger/`）

侧栏「日记」下面一行进入（同日记：不是节点，桌面 `ledgerOpen` / 移动端历史栈 `ledger` 层互斥占主区域）。数据住在**自己的 `ledger.json`**（第五个领域文件）：`accounts` 资金账户 / `categories` 两级分类（大类→子分类，支出与收入各一套，**两级封顶**）/ `entries` 流水（`expense|income|transfer`）。

### 金额一律整数分

（`amountCents`，对外 `amount`/`signed` 是两位小数的元；第三位小数四舍五入）——浮点累加在统计里会 drift。

### 账户余额不存现值

= 期初 + 流水推导（转账改两个账户），改历史账目不用回头修余额；净资产/总资产/总负债（信用卡负余额计入负债）由 `ledger balance` 与前端 `ledger.ts::assetsOverview` 同口径算出。**转账不计入收支统计**。首跑带一套种子账户与两级分类（core `LedgerFile::seed_defaults` 与前端 `seedLedgerBook` 同一套**确定性 id**：`lacc-01` / `lcat-exp-01-01`）。四个视图由头部段控切换，持久化在 `settings.ledger.view`（`list|calendar|stats|assets`，本机偏好）。

### 前后端同口径与单趟算法（v0.8.0）

**同口径的数字必须有测试钉住**（来龙去脉全在 `history/v0.8.md` 批次 3/4/5）：

- **统计分桶门槛 = 62 天，两侧一个数**：前端 `ledger.ts::bucketOf` 的 `days <= 62` ↔ core `ops_ledger.rs` 的 `DAY_GRAIN_MAX_DAYS = 62`，由一条 `include_str!("../../../../src/lib/ledger.ts")` 的测试把前端源码里的数字读出来与常量比对（与 `tests/ledger_icons.rs` 同一路数）。原来前端是 62、core 是「同一个自然月」，32~62 天的自定义区间 GUI 画日桶、CLI 给月桶。**光靠注释说「两边要一致」一定会漂。**
- **金额解析两侧逐条对齐**：`parseYuanToCents` 重写以对齐 core 的 `parse_cents`（收 `.5` 与 `5.`、丢掉第三位以后的小数、`Number.isSafeInteger` 守卫、避免 `-0`）；core `parse_cents` 对 i128 装不下的整数部分（≥39 位）一律**拒绝**（`.ok()?` + checked 算术）——原来 `.unwrap_or(0)` 静默归零，而 `account-modify --initial/--balance` 里 0 是合法值，会真的把期初写成 0。
- **搜索金额匹配的两个洞已堵**（`filterLedgerEntries`）：`amountNeedle` 为空直接不匹配金额（只由逗号/空格组成的查询会让 needle 成空串，`includes("")` 恒真——搜一个「,」把整本账都列出来）；按 kind 补带符号形态（支出 `-`、收入 `+`、转账不带符号，与 `LedgerEntryRow::amountText` 一致——`amountCents` 恒为正，带符号的查询原来永远搜不到）。

**单趟化**（改这些函数时别退回多趟扫描）：

- `accountBalances(book): Map<string,number>` 一趟算全部账户余额，`accountBalance` 与 `assetsOverview` 都委托它（原来每个账户各扫一遍全部流水）。
- `ledgerLookup(book)`：WeakMap 按 book 对象身份缓存 id→对象索引（book 换了自动失效）；`LedgerEntryRow.svelte` 用 `$: lookup = ledgerLookup(book)`，且 `entryName/entryIcon/entryColor` **显式传参查好的对象**——Svelte 不跟踪函数调用内部的依赖，让函数自己去 `$book` 里查，book 变了那条语句一辈子不会重算。
- `filterLedgerEntries` **先筛后排**（原来对全部流水先 `sortEntries` 再 filter：命中 5 条也要把 3000 笔拷贝+排序，而这一步每敲一个键跑一次）；`categoryStats` 用 `groupBy`/`childBy` Map（childKey = `${slot.categoryId}\u0000${category.id}`）。
- `LedgerView.svelte` 的 `$: monthGroups = monthDayGroups(book.entries, cursor)` 只算一次，`sections` 与 `monthTotal` 都从它派生（原来三个 `$:` 各调一次，3000 笔算三遍）。
- core 侧 `ledger stats` 从 O(keys × entries) 改单趟聚合（桶键借 `file` 的 `&str` 零分配；分类聚合先建 `HashMap<&str,&LedgerCategory>` 索引 + `Vec<CategoryGroup>` 槽位表；**JSON 最后才组装**，字段顺序与旧实现逐字节一致——serde_json 开了 `preserve_order`、`sort_by` 稳定排序），输出等价性由 golden 测试钉死 series 与 categories 的序列化文本。grain/series 的 CLI 语义见 `cli.md`「列表分页与合计」。
- 死代码已删（`signedCents`/`signedLabel`/`accountName`/`categoryName`、`ledgerIcons.ts` 的 SIDE_COLOR/SIDE_LABEL）：别再按这些名字找。

### 标签配色（v0.8.1）

标签配色是**全应用共用**的（记账/日记/待办同一套 `TagColor`），十个值：七彩虹（`red` `orange` `yellow` `green` `cyan` `blue` `purple`）+ `pink` + `gray` + `custom`。
- `custom` 必须配 `Tag.hex`（`#rrggbb`），缺失或非法一律按 `gray` 渲染——两端同一口径：前端 `defaults.ts::normalizeTagHex`、Rust `model::tag_hex`，另有 `Tag::effective_color` / `custom_hex` 兜底。
- 色值只定义在 `src/lib/tagColors.ts`（色盘圆点 / 胶囊底色 / 描边 / 文字四组），`workspace.css` 的 `.task-tag.tag-*` 与 `.tag-preset.tag-*` 是同一组色；**自定义色走内联 CSS 变量**（`--tag-bg` / `--tag-fg`，由 `.task-tag.tag-custom` 消费），因为颜色是用户选的、没有对应类名。
- 选择器 `TagColorPicker.svelte` 是**两排胶囊**（5 + 4，各自 flex 均分填满），最后一格是盖在胶囊上的 `<input type="color">`。**别换成圆形色点**：胶囊是用户点名要的样式，而且两排等分在窄面板（右键菜单 216px）里也不会换行。
- 面板 `TagMenuPanel.svelte`（右键菜单与日记条目菜单共用）从上到下三块：预置标签（点=加到条目上、右侧叉=删这条预置、右下角加号=把输入框内容存成预置且**不加到条目**）→ 输入框 + 「存入预置」勾选（默认勾）→ 两排配色。**没有「清除所有标签」按钮**（卡片上点标签就能删单个，为了「一键清空」在菜单里留个危险按钮不值当）。
- 预置标签住在 `appearance.tagPresets`（**跟着设置同步**，上限 64 条，core 侧 `expect_tag_presets` 校验），不是 localStorage——它是用户攒的内容，换台设备也该能用。
- CLI 侧 `--tag` / `--add-tag` / `--replace-tags` 的颜色段接受具名色或 `#rrggbb`（后者自动记成 `custom` + hex）；报错文案里列全了九个名字。

### 组织单位是「天」，与日记卡片同一条语言

列表视图顶部是**标题级的月份行**（`.ledger-month-bar`：‹ 2026年9月 › + 收/支/结余，**只有列表视图渲染**——其余三视图的顶栏自带周期与合计；v0.7.1 把它做成标题下小字，窄屏上会被视图切换图标挡住）+ 月份分隔行 + 一天一张 `LedgerDayCard`：**单列**——标题行 = 主题色日期号 + 日期 + 周几 + 右端当天收/支与「在这天记一笔」，下面每一笔一行左对齐（分类图标圆片 + 名称/备注 + 账户 + 带符号金额），**不折叠**——一天的笔数本来就该一眼看完；滚到底接上一个月、滚到顶接回下一个月（`months` 栈 + `handleScroll`，月份行的 ‹ › 直接换月并把栈重置）。标题行的日期用 `dayTitle`（今天/昨天/前天或「9月10日」）——`relativeDayLabel` 的完整形式自带周几，会和旁边那格重复。**日历** = 日记同款月历（居中），格子里写当天收/支数额（紧凑格式 `compactCents` 去掉无意义的 .00；不做热力图——数额本身就是最直白的信息）；选中日的日头与当天卡片**左对齐铺满内容宽**（与日记日历视图同一条排版，不跟着月历居中）。**统计** = 汇总卡（收/支/结余）+ 手写 SVG 曲线 + 分类占比环 + 排行进度条（子分类金额并进大类；**不引图表库**：包体积与风格都不值，server 管理台的活动曲线是先例）；**收/支是顶部一个总开关**，曲线只画当前侧一条线——两条线共用一根纵轴时一笔工资就能把整月支出压成地板线。统计与资产两块**铺满内容宽**（右缘对齐头部齿轮；v0.7.1 的 900px 上限在正常桌面窗口下右侧留一大片空白）；占比环带**引线标签**（占比 ≥4.5% 的分类才画，同侧上下挨太近的名字推开防叠字），**点一片沿中角拉出来加粗，环心换成它的名字/金额/占比笔数**，再点回总额（`focusId`，换周期或换收支侧时清空）。**资产** = 净资产卡 + 账户行（点行进编辑）+ 添加/转账。

### 所有浮层复用 `.editor-overlay` / `.editor-dialog` 那套语言

（记一笔 `LedgerEditor`、`CategoryManager`、`AccountManager` 三个都是）：桌面居中对话框、移动端底部抽屉（`mobile.css` 把遮罩改成底对齐 + 只圆上角 + 安全区垫在抽屉身上）；记一笔 = 类型页签 → 大字号金额（桌面是内容宽输入框、移动端只读展示 + 4×4 数字键盘，「记一笔」键**上下跨两格**——移动端 `.ledger-key` 的 48px 会压住 `grid-row: span 2` 的拉伸，`.save` 必须收回 `height:auto`，否则键只占第一行、0 键旁边留一个洞）→ 大类 chips + 子分类图标网格 → 日期/账户/备注 → 保存再记 / 记一笔（桌面底栏**右对齐**，`.ledger-foot-spacer`）。

### v0.7.2 起只有显式点保存才落盘

X / 遮罩 / Esc / 移动端返回 / 组件卸载一律丢弃草稿——卡片编辑器「关掉即保存」是防丢正文，账目照抄会凭空多出用户没确认过的记录；**v0.7.3 把三个按钮的语义钉死**：「记一笔 / 保存修改」= 落盘并关掉，「保存再记」= 落盘、清空金额与备注、面板留着连着记，**编辑已有的一笔时不给「保存再记」**（改账与再记一笔是两件事，混在一个按钮上就是用户报的「点了保存又弹出编辑器」的来源之一；另一半原因是幽灵点击，见移动端那条）；**改一笔转账也走 `ledger.modify`**（早先按 `kind` 分派去 `ledger.transfer`，于是「改转账」变成「又记一笔新的转账」，旧那笔原地不动）——core 的 modify 本来就认 kind/转入账户，空串即清除；日期浮层放**整块 DatePicker**（`.ledger-pop.date` 去掉 264px 限高与第二层壳，否则「清除/今天」被裁掉只能滚动看），为此 `.ledger-sheet` 放开 `overflow:hidden`（圆角改由 head/foot/keypad 自己带）且**必须 `min-width:0`**——放开裁剪后它不再是滚动容器，作为遮罩 grid 子项的自动最小尺寸会被分类 chips 那排撑出视口（手机上抽屉比屏幕宽一倍就是它）。分类/账户管理是面板内子层（表单打开时列表让位，不做弹窗套弹窗），分类与账户都能自选图标（`LEDGER_ICON_CHOICES` 六十个 lucide 图标的选择网格）与颜色；名下有账的账户 core 拒删（`LEDGER_ACCOUNT_IN_USE`）。齿轮面板 = 分类管理 / 账户与转账 / 记账菜单（`ListMenu` 的 `ledgerMode`：外观写 `settings.ledger.*` 扁平配置、导出导入走 Excel 压缩包，与 diaryMode 同构，`settingsPrefix` 一个变量分派）。


## 归档、CLI 与 CSS 约定

### v0.7.1 踩的三个坑

① 齿轮下拉面板必须放在 `.header-actions` **里面**（它才是 absolute 包含块）——v0.7.0 把它放在 `.list-header` 下，点「记账菜单」时点击冒泡到 app-shell 的 `closeOverlays`，刚开出来的 ListMenu 立刻被关掉，表现成「三点菜单唤不出」；② **不要 `bind` 到 `{@const f = form}` 指向的对象属性**——改的是对象内部字段，Svelte 不失效 `form`，保存按钮永远停在 disabled（两个管理器因此改成平铺的 draft 变量）；③ 浮层输入框上的 `on:keydown|stopPropagation` 会把 Escape 一起吃掉，两段式关闭（先收表单再关面板）失效——统一走 `shortcuts.ts::fieldKeydown`（吞全局快捷键、放行 Escape）。

### Excel 归档

（`ledger_archive.rs`，rust_xlsxwriter 写 + calamine 读，均纯 Rust）：zip 内含一张 `kxtodo-ledger.xlsx`，四张表 说明（格式标记 `KXTODO_LEDGER_V1` + 合计）/ 账户 / 分类 / 账目（日期|时间|类型|账户|转入账户|大类|分类|金额|备注，金额带符号的元）；导入只认这套表头（zip 或裸 xlsx 都收），账户/分类按名字合并、缺的自动建，日期非法或金额为 0 的行跳过计入 `skipped`，**重复导入会产生重复账目**（确认门）。

### CLI 子命令名与 core 命令名的映射

CLI 子命令是 kebab（`ledger account-add` 等；core 命令名仍 camel，`build_ledger_invocation` 做映射）。

### ledger.css 的盒子 / 字号 / 按钮约定

`ledger.css` 照抄 diary.css 的盒子与字号算法（全部 `calc(var(--font-control) ± N)`，**不许写死像素**——v0.7.0 就是字号各写各的被用户点名；引文是 v0.7.0 原文，**v0.7.3 起 ledger.css 的基变量是 `--font-ledger`**，v0.8.0 起全仓 CSS 除 `base.css` 的变量定义外不再有任何 `font-size: Npx`，见 `invariants.md` 第七节）且**绝不写 `.ledger-view > *`**（只显式列举 `.ledger-month-bar`/`.ledger-scroll` 抬层）；按钮只许 `settings-button`（危险动作加 `.danger` 变体）与 `menu-action-button` 两类。

### CLI 改账本必须先过确认门（v0.7.2）

`ledger_dispatch` 分发层对 add/transfer/modify/accountAdd/accountModify/categoryAdd/categoryModify 统一 `require_confirmation`（remove/accountRemove/categoryRemove/import 保留各自信息量更大的内部门——**一个动作只有一道门**），未带 `--yes` 返回退出码 10，文案明确告诉 Agent「金融数据敏感，先向用户说明这次增删改并得到同意」；只读动作不设门；GUI/Android 桥恒带 `controls.yes=true` 不受影响；`schema.rs::risk_for` 里这些动作全是 high-risk-write。

