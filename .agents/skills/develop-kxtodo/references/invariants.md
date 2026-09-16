# 不变式与铁律（含每条的「为什么」）

> **这份文件是什么**：KXToDo 的**全部硬约束**——写路径与命令层、跨平台三铁律、不做兼容、版本号只有 git 一个来源、commit-msg hook、发布工作流四铁律、CSS 铁律、浮层与安全区，最后一节是把散落在各处的「不许 / 必须 / 绝不 / 一律 / 别 / 勿」类硬规则汇成一张速查表（每条都给出处）。
> **什么时候读它**：**任何时候**——这是本 skill 里唯一「无论改哪一块都该先扫一眼」的文件。SKILL.md 的第 4 节是它的摘要；动手前拿不准某条约束为什么存在，来这里查。
> 每条速查项后面标的出处文件里有完整的上下文与「为什么」。

## 目录

- [一、写路径永远过 Domain Core 命令层](#一写路径永远过-domain-core-命令层)
- [二、跨平台铁律（三条）](#二跨平台铁律三条)
- [三、不做兼容](#三不做兼容)
- [四、版本号只有 git 一个来源](#四版本号只有-git-一个来源)
- [五、commit-msg hook 强制 subject 以 vX.Y.Z 开头](#五commit-msg-hook-强制-subject-以-vxyz-开头)
- [六、发布工作流铁律（四条）](#六发布工作流铁律四条)
- [七、CSS 铁律](#七css-铁律)
- [八、浮层与安全区铁律](#八浮层与安全区铁律)
- [九、分散在各处的硬约束速查](#九分散在各处的硬约束速查)

## 一、写路径永远过 Domain Core 命令层

**铁律**：写路径永远过 Domain Core 命令层；前端永不直接改 JSON；GUI 桥接默认 `controls.yes = true`（GUI 操作即用户确认，CLI 的确认门不适用于 GUI）。

v0.8.0 起的几条补充（来龙去脉在 `history/v0.8.md` 批次 1）：

- **纯 UI 写命令不进 audit 台账**：`repo.rs::UI_ONLY_COMMANDS` 白名单六个（`gui.select-node` / `set-collapsed` / `set-item-ui` / `set-items-ui` / `set-diary-ui` / `set-schedule-ui`）——点一下树节点就写一行审计日志，台账会被 UI 噪音灌满。**新加纯 UI 写命令要进这个白名单**。
- **备份是五个领域文件全量**（`repo.rs::backup_locked`；此前只备三个，diary/ledger 写坏了没有备份）；损坏提示指向 `backups/` 里最近 5 份全量备份，恢复靠手工拷回覆盖——**没有自动恢复命令**（`restore` 与 LWW 语义冲突，明确不做）。
- **`core_snapshot` 可按域过滤**（`lib.rs::core_snapshot(core, domains)` + `wanted()` 名单 + `put_snapshot_domain`；前端 `refreshFromCore` 只拉脏域、`stores.ts::appliedRevisions` 按 revision 跳过旧事件）：**新加领域文件必须把它加进 `wanted()` 名单与 `put_snapshot_domain` 调用**，否则前端永远拉不到它；前端 `applySnapshot` 各分支必须带 `&& snapshot.X !== undefined` 守卫——按域拉取时缺的域不能被当成空。
- **`gui.apply-tree-order` 里跨父移动（reparent）必须算 changed**——只比 order 的话，换父级但 order 数值相同时改动被当成「没变」而不落盘（v0.8.0 N1 修复，别改回去）。
- **图片入库有 5MB 体积闸**（`lib.rs::shrink_oversized_image`）：超限才动手（JPEG 长边 2560/质量 90；PNG 长边 4096 仍无损——票据长截图以文字为主，宁可省得少也不糊字）；**GIF/WebP 一律原样保留**（可能是动图，解码只拿得到第一帧，重编码等于把动画弄丢）；解码失败 / 重编码失败 / 重编码后没变小三条兜底任何一条不满足都原样返回——**压缩永远不许弄丢或弄坏用户的图**。5 个图片命令一律 `async fn` + `spawn_blocking`（Tauri 的同步命令跑在主线程上，一次几十 MB 的解码+缩放就能把窗口和 IPC 卡死）。

## 二、跨平台铁律（三条）

**跨平台铁律**：KXToDo 是 Windows / Linux / Android 三端应用，**任何更改、新增功能、bugfix 都必须按跨平台审视**：
1. 底线——不能让任何一端变得不可用（编译不过、启动白屏、核心路径坏掉都算）；平台专有代码必须 `#[cfg]` / capabilities 隔离，改共享层时逐端过一遍影响面。
2. 进阶——考虑该功能是否需要适配其它平台：能力差异收敛到 `capabilities.ts` + Rust `#[cfg(desktop)]`/`#[cfg(not(desktop))]`，UI 差异收敛到 CSS `.app-shell.mobile` 命名空间与平台覆盖 conf（如 `tauri.linux.conf.json`），不要在业务组件里散落平台判断。
3. 验证——本地验证不了的平台（如无 WSL 时的 Linux、无真机时的 Android）交给 CI（ci.yml 双平台编译检查 / release.yml 三平台构建），但必须明说“该端未验证”，不许默认没问题。

## 三、不做兼容

- **不做兼容**：项目没有正式 release，不存在需要兼容的旧版用户。数据格式、数据位置等变更一律直接切换，不写迁移/兼容代码；旧数据由用户自行迁移或丢弃。

**唯一的例外**（原文在 `ledger.md`「记账」主描述里）：记账 v0.7.4 的单数 `image` 字段在加载 normalize 时一次性折叠进 `images`（core `fold_legacy_image` + 前端 `normalizeLedgerEntry` 两侧都做）——「用户真实账本里的图不能丢，这是「不做兼容」铁律的唯一例外，重写后旧键不再落盘」。（两侧的折叠判据必须同为「**原始数组为空**」：前端曾拿 `Array.isArray(item.images)` 当分支条件而 core 拿 `images.is_empty()`，`"images": []` 配旧 `image` 时前端丢图、core 保留——v0.8.0 已修，见 `history/v0.8.md` 批次 4 ⑥。）

**v0.8.0 把最后一份迁移代码也删了**：`migrate.rs`（v8→v9 上古迁移）整个删除，连带 `tests/migration.rs`、`repo.rs` 里 12 处 `migrate_if_needed` 调用、`time.rs::migrate_legacy_local_time`、`exec.rs::split_legacy_arguments`、`tests/common/mod.rs` 的 v8 fixture 助手。这条铁律现在**连迁移代码都不留**——数据格式变更一律直接切换，旧数据由用户自行迁移或丢弃；别把 migrate.rs 加回来（`doctor.rs` 里「运行任意命令触发迁移」的过期提示也随本版一并删了）。

## 四、版本号只有 git 一个来源

**版本号只有 git 一个来源**：三个 `build.rs`（根 crate + `crates/core` + `crates/server`）、`release.sh` 的 `resolve_version`、`package.ps1` 的 `Get-GitVersion` 共用同一套优先级——**HEAD 上的精确 `v*` tag → 最近 30 条 commit 里第一条 `vX.Y.Z` 主题 → 最近的祖先 `v*` tag**，都没有则回落 `0.0.0-dev`。`build.rs` 构建期注入 `KXTODO_VERSION`，GUI 经 `app_version` 命令展示在设置页，CLI 的 `version` 命令同源。**仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）——发版只打 tag/写 commit，永远不要往文件里同步版本号。
改这套解析前必须知道的两个真实 bug（都已修，别改回去）：① **精确 tag 必须排在祖先 tag 之前**——`describe --abbrev=0` 找的是最近祖先 tag，让它优先的话未打 tag 的版本 commit 会静默拿上一个版本号，且 subject 分支永远轮不到；② **subject 校验只能取第一个空白前的 token**——拿整行去 `split('.')`，最后一段必然含中文说明，会让这条回退对所有真实 subject 都失效（等于死代码，一路回落 `0.0.0-dev`）。发版仍建议先 commit 再在 HEAD 打 tag（`publish.ps1` 有同样兜底），但版本号正确性已不再依赖这一步。

## 五、commit-msg hook 强制 subject 以 vX.Y.Z 开头

**这条格式由 commit-msg hook 强制**：源文件版本化在 `scripts/git-hooks/commit-msg`，`npm install` / `npm ci` 的 `prepare` 经 `scripts/install-git-hooks.mjs` 把它拷进本克隆的 hooks 目录（不用 `core.hooksPath`——那会整体接管 hooks 目录，屏蔽掉别的工具挂在 `.git/hooks` 里的 hook）。subject 不以 `vX.Y.Z` 开头直接拒绝提交，判据与上面几处解析器完全一致：**小写 `v` + 恰好三段纯数字**，所以 `v1.2`（两段）、`v0.4.2.1`（四段）、`V0.4.2`（大写）都拒。两个实现细节坑：`package.ps1` 侧必须用 `-cmatch`（PowerShell 的 `-match` 默认大小写不敏感，会放行 `V1.3.0`）；hook 文件在 `.gitattributes` 里钉了 `scripts/git-hooks/** eol=lf`——它没有 `.sh` 扩展名不被原有规则覆盖，`core.autocrlf=true` 下新克隆检出成 CRLF 会把 shebang 变成 `#!/bin/sh\r`，hook 静默失效等于整条强制形同虚设。merge / revert 等自动生成的消息用 `git commit --no-verify` 显式绕过。新克隆跑一次 `npm install` 即自动装上。

## 六、发布工作流铁律（四条）

  1. 本地构建只用 `release.ps1`（Windows/Android/unix）与 `release.sh`（Linux 原生）；`publish.ps1` 基本不再使用，仅作离线/CI 不可用时的备用路径。
  2. **每次推送远程前必须询问用户：这一版是否需要打 tag（即是否发 release）**。要打则 commit → 在 HEAD 打 `vX.Y.Z` tag → 一并推送分支与 tag；不打则只推分支。
  3. **同一版本的修复只能 amend，不许新开 commit**：用户要求实现某个版本（如 v0.4.2），该版本已经 commit 了，用户又反馈有问题——**在用户没有明确指定"这次修复要升版本号"之前，所有修改（bugfix、文档补充、CI 修复、遗漏的改动）一律 `git commit --amend` 并进那笔 `vX.Y.Z` commit**，保持"一个版本 = 一笔 commit"。已经打了 tag 的按顺序来：`git tag -d vX.Y.Z` → amend → 在 HEAD 重打 `vX.Y.Z`（tag 已推远程则 force-push tag，并按需处理对应的 GitHub release）。另起一笔 `vX.Y.Z 修复…`、或不带版本号前缀的散 commit，都算违规。
  4. **不论是否打 tag，推送后必须监听远程 CI 直到结束**（ci.yml 编译检查；打了 tag 还有 release.yml 三平台构建 + 发布）：确认检查通过、构建/发布成功；任何失败都必须后续处理（修复重推或 re-run failed jobs），不许放着红着不管。

## 七、CSS 铁律

完整的「为什么」与两个真实事故在 `frontend.md`「层叠坑（v0.6.6 踩、v0.6.7 修透）」与「按钮样式规范（不许再发明新样式）」。

1. **绝不写 `.xxx > *`**。原文：「新增整页面板时照抄 `.workspace` 的盒子样式，就要连这一条一起考虑：**绝不要写 `> *`**。」修法「不是补更高特异性的补丁，而是**把兜底规则换成显式列举正文流子元素**」。记账那一份同一条铁律：「**绝不写 `.ledger-view > *`**（只显式列举 `.ledger-month-bar`/`.ledger-scroll` 抬层）」。**最后一处违例 `.workspace > *` 已在 v0.8.0 删除**，换成显式列举 `.list-subtitle`/`.task-list`/`.add-task-bar`/`.scheduler-panel` 四个正文流子元素：自带定位的（`.list-header` relative/20、`.planned-group-bar` relative/30、`.link-preview-overlay` fixed/9000、`.context-menu` fixed/3600）刻意**不列**；列进来的四个必须抬，因为 `.workspace::before` 是 absolute/z-index 0 的背景层，正文流子元素退回 static 就画在它下面（背景图会盖住卡片与输入栏）。
2. **字号一律 `calc(var(--font-*) ± N)`，不许写死像素**。原文：「`ledger.css` 照抄 diary.css 的盒子与字号算法（全部 `calc(var(--font-control) ± N)`，**不许写死像素**——v0.7.0 就是字号各写各的被用户点名）」。记账与日记 v0.7.3 起吃自己独立的变量 `--font-ledger` / `--font-diary`，不再跟 `--font-control`。**v0.8.0 起全面收编：除 `base.css` 的 5 个 `--*-font-size` 变量定义（它们是变量本身，运行时由 `styles.ts::fontVars` 内联覆盖）外，CSS 里不许再出现 `font-size: Npx`**（100 多处已换成 `calc(var(--font-*) ± N)` 或 `var(--font-*)`，默认设置下逐像素等价）；基变量按区域选——ledger 页 `--font-ledger`、日记页 `--font-diary`、其余 chrome `--font-control`（settings.css 用等价的 `--ui-font-size`）。**唯一例外**：`ledger/AssetsTrend.svelte` 的内联 `axisFont`——SVG 按 viewBox 整体缩放，`axisFont` 是按容器宽度**补**字号的，用 CSS 变量会被二次缩放。
3. **按钮只许 `settings-button` / `menu-action-button` 两类**：「菜单项一律走 `MenuItem` 组件（menu-item-button），不写裸 `<button>`」；「新写任何按钮前先想这两个类能不能用；风格不一致的裸按钮视为 bug」；危险动作加 `.danger` 变体；同步卡片里「按钮样式仍只许 `settings-button` / `menu-action-button` 两类」。
4. **级联顺序固定**：「`main.ts` 按级联顺序导入：base → titlebar → sidebar → workspace → settings → menu → shared → editor → diary → mobile（mobile 必须最后，它覆盖前面所有区域）」。同名类用父选择器区分（如 `.sidebar .collapse-button` vs `.workspace .collapse-button`）。「移动端样式全部收在 `.app-shell.mobile` 下，桌面零副作用」。
5. **别拿全局类名当状态类名**：「base.css 给折叠箭头用的通用 `.collapsed { transform: rotate(-90deg) }` 是全局类名，代码块折叠态也叫 `collapsed` → 整块代码被转 90°「竖起来」，规则已限定 `svg.collapsed`，**新代码别再拿 `collapsed` 当状态类名**」。
6. **夹行只写 `-webkit-` 三件套**（`display:-webkit-box` + `-webkit-box-orient:vertical` + `-webkit-line-clamp:N`）：「无缀 `line-clamp` 在 Chromium 里会把 `display` 的计算值顶成 `flow-root`，夹行直接失效（实测折叠态标题没被夹成两行）；别「两个都写以求兼容」」。
7. **移动端不画滚动条，横向滑动一律不许有**：「`.app-shell.mobile *` 一律 `scrollbar-width: none` + `::-webkit-scrollbar` 隐藏」；四个主滚动区再加 `scrollbar-gutter: auto` 与 `overflow-x: hidden`（「**横向滑动一律不许有**，能横滑通常意味着有东西宽出屏幕」）。
8. **特异性坑**：「`.markdown-body a:hover` 的优先级比单类选择器高，得写成 `.markdown-body .kx-link-card-main:hover`」。
9. **触屏点按蓝罩统一关掉**：「`.diary-view` 内的可点元素一律 `-webkit-tap-highlight-color: transparent`」；v0.7.6 补了 `.app-shell.mobile .list-header button`；v0.7.7 补了移动端齿轮按钮的 `:hover`（「点一下留下的「悬停」底色会一直挂着」，本色改由 `:active` 给）。
10. **grid 的自动最小尺寸压不住**：「img 是 grid 子项，百分比 max-width 压不住 grid 的「自动最小尺寸」（= 内容尺寸），必须显式 `min-width/min-height: 0`」；`.ledger-sheet` 同理「**必须 `min-width:0`**」。
11. **移动端遮罩别用 grid**：「`.ledger-overlay` 在移动端是 **grid**，auto 行会被超高内容撑到内容高度，抽屉的 `max-height:94%` 就按那个被撑大的行算」→ 改成「**flex 纵向 + `justify-content: flex-end`**」+ 抽屉 `min-height: 0`。

## 八、浮层与安全区铁律

- **`createBackGuard()` 自带「组件销毁即摘掉拦截器」**（内部在初始化期注册 `onDestroy`），所以正常写法就够了：`{#if}` 挂载式的浮层用 `$: backGuard(true, close)`，`open` 是 prop 的用 `$: backGuard(open, onClose)`。**别绕过它直接用 `addBackInterceptor`**——那个得手动配对注销，漏一次就是返回键永久失灵的后果。为什么这条这么要紧：菜单 / 图标选择器 / 日期选择器这类由调用方 `{#if}` 挂载的浮层，`createBackGuard` 从头到尾只会用 `true` 调一次，卸载时不走 `active=false` 那条分支——不 dispose 的话拦截器永久留在栈里，返回键被一个已经不可见的浮层吃掉，**用户眼里就是「返回键失灵」**（v0.8.1 踩过，靠 `v081-fixes-test` 的移动端用例抓住）。组件在 `{#if}` 里被整块拆掉（开着浮层时切走页面）同样会被 `onDestroy` 覆盖。
- **`{#await import(...)}` 一律配 `{:catch}`**：懒加载的 chunk 拉不到时，只有 `then` 的写法就是「点开什么都没有，也退不出去」。5 处懒加载（三个编辑器 + 两个图标选择器）统一用 `.lazy-fallback` 兜底块并收掉浮层状态。

1. **任何 JS 命令式建的全屏/浮层都要先问一句：移动端顶部避让了吗？** 原文：「**全屏浮层要吃状态栏安全区**（mobile.css 的 `.app-shell.mobile .md-fullscreen-bar`）：浮层是 JS 建出来挂进 `.app-shell` 的（`markdownControls` 的 `ensureOverlay` 刻意不挂 body，否则拿不到 `--safe-inv`）。**这条安全区坑是第三次踩**（v0.6.8 编辑器全屏、v0.6.8 链接预览标题栏、v0.6.9 图全屏工具栏）」。
2. **不占历史栈的覆盖层要注册返回键拦截器**（`platform.ts` 的 `addBackInterceptor`），否则「返回把历史栈上的编辑器弹掉了，罩子却还留在原地——界面「卡住」」。分类/账户管理器也注册了（两段式：表单→列表→关）。
3. **菜单限高绝不能把菜单顶到视口顶部**：「正确做法：按**锚点向下的空间**限高并保持 `top=锚点`（菜单永远从按钮/触点下方展开、内部滚动），只有向上空间明显更大时才向上翻转」；「重新收敛时别量自己的 rect 高度……内容高度取 `max(rect.height, scrollHeight)`（两者都要除以 uiScale）」。
4. **两段式 Esc 与让位**：「全屏浮层的 Esc 必须 preventDefault+stopPropagation，否则编辑器的 window keydown 接着把编辑器也关掉」；「两层的 window keydown 监听谁先注册谁先跑，所以**记账面板的 Esc 处理先让位**」。
5. **浮层输入框的 keydown 不许吃掉 Escape**：「浮层输入框上的 `on:keydown|stopPropagation` 会把 Escape 一起吃掉，两段式关闭（先收表单再关面板）失效——统一走 `shortcuts.ts::fieldKeydown`（吞全局快捷键、放行 Escape）」。编辑器那侧同一条：「`handleTagKeydown` / 内联标签编辑输入框无条件 `stopPropagation`，把 Escape 连同全局快捷键一起吃掉」。
6. **幽灵点击**：「**只在 pointerdown 驱动的关闭路径武装**——Esc / 返回键 / X 按钮（click 或 keydown 驱动）没有手势在飞，武装了反而吃用户下一次点击」；「**按坐标而不是按时间窗无条件吞**」。
7. **齿轮下拉面板必须放在 `.header-actions` 里面**（它才是 absolute 包含块），否则「点击冒泡到 app-shell 的 `closeOverlays`，刚开出来的 ListMenu 立刻被关掉」。
8. **全屏预览根节点补 `on:click|stopPropagation`**：「App 的 app-shell 挂着「点空白关所有浮层」，翻页按钮的 click 冒泡上去会把整个罩子连同自己一起收掉」。
9. **z-index 层级**：`.ledger-view .editor-overlay` 抬到 4200 盖住 App 层的记账面板（4000）；`LedgerImagePreview` 全屏罩 z-index 4600（「盖住一切记账浮层」）；「**viewer 必须挂 LedgerView 顶层**：挂在 `.ledger-scroll` 里会被它 z-index:1 的 stacking context 困住」。
10. **菜单内有聚焦的可编辑元素时**：「scroll 一律不关，resize 改成重新收敛位置与限高」（软键盘 = resize + scroll 两连击）。
11. **钻入式二级菜单的隐藏要用兜底选择器**：「`sub-open > *:not(.menu-item.has-submenu):not(.submenu-back)` 全隐藏——旧规则逐个列举 `.menu-item/.menu-separator/.menu-section-title`，漏了 ListMenu 下半部的裸 div/label/input」；且「钻入式二级菜单**仅移动端**」。
12. **半屏抽屉里只许一层滚动**：「移动端 `.ledger-sheet-body .ledger-icon-grid` 完全展开」（v0.7.5 的做法，v0.7.7 ⑥ 起简笔画区改回自己滚但**不画滚动条**）；「触屏手势被它吃掉、外层表单永远差最后几行」。
13. **拖动跟手**：透明度条/取色器/背景链接输入在交互期间用本地草稿冻结（`opacityLive/uiColorLive/linkLive`），change/blur 后再跟随已提交值。原文（`ui-patterns.md`「⋯ 列表菜单」的「拖动跟手（v0.6.8）」）：日记模式的外观写入走 `config.set`——**它等 IPC 往返回来才更新 store，滞后的回渲会把「已提交的旧值」写回 range/color input，透明度条不跟手、取色器跳变**（条目页的 `setBackground` 同步改 store 所以没这病）。
14. **`.list-header` 自带 `position:relative; z-index:20`**，新增整页面板时不必再把它列进兜底规则。

## 九、分散在各处的硬约束速查

（每条都是原文照引 + 出处；出处文件里有完整的「为什么」。）

### 数据与写路径

- 「**GUI/CLI/Agent 三方写操作走同一条业务命令层**（Domain Core 的 Invocation → 域分发 → envelope 输出），不存在"读全量 JSON 改完写回"的路径。」——`architecture.md`「进程拓扑」
- 「**新写操作一律加在这里，不要在组件里直接改 store 或 invoke。**」——`frontend.md`「actions.ts」
- 「ID 128-bit 随机 hex（32-bit 会跨设备碰撞）」；「`collapsed`/`expanded` 是本机 UI 状态不参与同步」；「Node/Item 显式 `order: f64` 字段（同级排序唯一来源，数组顺序仅是渲染缓存，`gui.apply-tree-order` 写 order 不再裸排数组）」——`architecture.md`「五个领域文件与数据地基」
- 「**CLI 永不静默创建数据**」——`architecture.md`「数据目录解析」
- 加一类要同步的实体要动 `model.rs` / `repo.rs` / `merge.rs` / `engine.rs` **五处** / `host.rs` 的 `emit_domain_event` match（「新 Domain 变体不补会直接编不过」）/ `core.rs` 与 `cli.rs` 的 schemaVersions / `lib.rs` 的 `core_snapshot`（**`wanted()` 名单与 `put_snapshot_domain` 调用都要加**，它按域过滤）/ 前端 `CoreSnapshot`——SKILL.md 路由表（「**server 一行都不用改**（entities 表没有 kind 列，kind 只在密文里）」）
- 「**纯 UI 写命令不进 audit 台账**（`repo.rs::UI_ONLY_COMMANDS`），新加纯 UI 命令要进这个白名单」；「**备份是五个领域文件全量**（`backup_locked`），恢复靠手工从 `backups/` 拷回，没有自动恢复命令」——本文第一节
- 「**图片入库有 5MB 体积闸**；GIF/WebP 一律不动（可能是动图）；解码/编码失败或没变小一律原样保留——压缩永远不许弄丢或弄坏用户的图」；「图片命令一律 `async fn` + `spawn_blocking`」——本文第一节 + `history/v0.8.md` 批次 6
- 「`is_safe_image_name` 全项目只有 `diary_archive.rs` 一份实现（含 Windows 保留字符 `<>:"|?*`），`lib.rs::safe_image_name` 委托它」——`history/v0.8.md` 批次 1
- 「桌面专有逻辑必须 `#[cfg(desktop)]` 隔离并在移动端给空实现（前端 invoke 不能炸）」——SKILL.md 路由表「加原生能力」
- 「该命令必须同时注册进移动端 invoke_handler，否则前端调用直接失败」（`image_data_url` 命令）——`frontend.md`「capabilities.ts」
- 「`cards_export_zip`/`cards_import_zip`/`cards_import_folder` 两个 invoke_handler 都要注册」；「`ledger_export_zip`/`ledger_import_zip`（**两个 invoke_handler 都要注册**）」——`ui-patterns.md`「一般卡片的 Markdown 压缩包」+ SKILL.md 路由表「改记账」

### 同步

- 「合并永远在客户端。」——`sync.md`「同步语义」
- 「拉取水位更新必须在合并事务**之后**读最新文件对账（早前先更新水位导致刚拉下的实体被误判脏又推回，服务端 seq 暴涨）」——`sync.md`「踩坑记录」①
- 「**改「连哪儿」不许动 merge/crypto**（分层的全部意义）」；「改信封结构要同步动 `merge.rs` 的 SyncEnvelope 与测试，改图片元数据要同步动 `images.rs` 与 `db.rs`/`api.rs`」——SKILL.md 路由表「改同步协议/加密」+ `sync.md`「传输分层」
- 「`sync configure/register/login/unpair` 必须 `emit_domain_event(Domain::Settings)`，否则用 CLI 改间隔时正在运行的 GUI 前端永远等不到 appSettings 变化」——`sync.md`「自动同步真正生效」的「坑」
- 「**入口绝不能用 `coreMode` 门控**——onMount 时水合还没完成、coreMode 恒为 false，v0.4.1 就是这样把整条自动同步写成了死代码」——`frontend.md`「syncRunner.ts」+ `sync.md`
- 「`startMobileRouter()` 只能由 App onMount 调用——模块顶层挂载会因 platform ↔ stores/backend/capabilities 循环依赖 TDZ 白屏」；「订阅类初始化必须封装成函数由 App onMount 调用」——`frontend.md`「platform.ts」+ `pitfalls-android.md` 第 7 条
- 「密钥丢失=数据不可恢复」；「升级密钥/盐算法前想清楚——改了派生参数所有设备全部失配」——`sync.md`「安全模型」+ SKILL.md 路由表「部署/运维 kxtodo-server」
- 「**配对时绝不替本机盖设置时间戳**」：早先的「本机已有 settings.json 就把此刻记成设置最新时间」判据是假的（用户为了配对必然先填过用户名密码，那一下 `config.set` 就把 settings.json 建出来了），于是任何设备配对都会带着默认资料赢得 LWW 并推给服务端——清空数据重装之后**资料永远回不来，还会把别的设备改成默认值**。正确判据是「本地有没有用户改过的共享设置」，而 `settings.syncUpdatedAt` 自己就是答案（只有 `is_shared_settings_path` 里的路径会 `bump_sync_updated_at`）——`history/v0.8.1.md` 一.7
- 「同步过来的设置载荷**逐字段覆盖，缺哪个键就动哪个键**」：整块反序列化会让结构体的 serde 默认值（"Example User" / "example@example.com"）顶掉本机值——「缺字段」只该理解为「这条记录没提它」——同上 一.8
- 「`merge.rs::settings_payload` 里出现的键要在 `ops_config::is_shared_settings_path` 里也有，否则值跟着走但 LWW 时间戳不刷新——改了等于白改」（v0.8.1 补上 `features.linkRender` / `features.dueHighlight` / `appearance.dueColors` / `appearance.tagPresets`）——同上 二
- 「**不写兼容代码**：旧账户就是登不上了，这是用户确认过的取舍。」——`sync.md`「账户模型简化（v0.5.1）」
- 图片：「删除**不**传播（孤儿图片留给后续的「清理无引用图片」功能）」；「**两侧都做穿越校验**（`..`/分隔符/控制字符/前导点/超长一律拒）」；「落盘一律 `.part` + rename 原子写」；「签名一变两条水位归零全量重拉」（`runtime/sync.json` 记 `scopeSignature`）——`sync.md`「图片同步（v0.5.0）」
- 「实际端口一律从 handle / 描述符取，别假定等于配置端口」；「**活着的别的 kxtodo 进程占端口不去杀**（那可能是用户真在用的独立 server），只上移 + 日志说明」——`sync.md`「端口生命周期」
- 「内置主机若写 pidfile，`kxtodo-server --stop` 会误杀宿主 GUI」；「进程级动作（pidfile / `--daemon` / `--stop` / 信号 / 退出码 / `--update`）只留在 `main.rs` 薄壳」；「必须非阻塞：可能在一次命令执行途中被调用」——`sync.md`「内置主机」
- 「重启路径会先**有界等待旧服务循环真正退出**（`ServerHandle::shutdown`，5 秒上限）再 bind——只「请求」停机就接着 bind 会撞上自己还没释放的 socket」——`sync.md`「端口生命周期」
- 「P2P 的 base_url 是一条**本地临时隧道**，`endpoint::Resolution` 带着隧道句柄，调用方必须把它活到本轮结束（句柄一掉地址即作废）」——`sync.md`「传输分层」
- 「硬约束：relay 无 store-and-forward，对方不在线是正常情况（按 reconnectSeconds 静默重试）」——`sync.md`「P2P 同步」
- 「管理路由必须用绝对路径 + `merge`，`nest("/admin", …)` 在 axum 0.8 下只匹配 `/admin`，手输 `/admin/` 会 404」；「日志行的 kind 小标签类名不能叫 `op`」——`sync.md`「管理控制台重做」
- 「`--data-dir` 指定 settings.json/log/server.pid 的根目录——**e2e 测试必须传**，否则 spawn 的真实 server 会覆盖开发机/生产机……里的管理员凭据」——`sync.md`「server 运维（v0.5.0 追加）」
- 「**记账种子 id 必须确定性**（`lacc-01`/`lcat-exp-01-01` 这类，core 的 `LedgerFile::seed_defaults` 与前端 `defaults.ts::seedLedgerBook` 同一套）：……随机 id 会让「加载两次 = 两本不同的账」，流水指向不存在的账户。」——`sync.md`「实体与范围」+ `ledger.md`
- 「**总原则（用户反馈）**：配置界面不堆说明段落——会错乱、遮盖且冗余；细节一律进 `title` 提示，状态与错误只给一句话。」；「按钮样式仍只许 `settings-button` / `menu-action-button` 两类」——`sync.md`「设置面板同步卡片重排」
- 「广播本身失败（容器禁 UDP 之类）按「没发现」处理并报错 `SYNC_LAN_HOST_NOT_FOUND`（Io 类，走静默重连），别把 socket 错误抛给用户」；「广播本身跑不起来则跳过查重，别为环境怪癖挡用户」——`sync.md`「局域网自动发现」「主机身份是名字」

### 前端 / Svelte / 渲染

- 「**首帧可见的东西必须进外观/资料缓存**」：外观缓存收**整个 `appearance`**（早先只白名单了 7 个数字字段，`navLayout`（双列）这类字符串字段从来没缓存过 → 每次冷启动先按单列画一帧再跳）；读回来要过一遍 `normalizeSettings`。资料缓存（名字 / 邮箱 / 头像）**写入失败要退一步只写名字邮箱**——头像在移动端是几 MB 的 dataURL，`setItem` 抛配额错时早先整条放弃，把名字邮箱一起拖去闪默认值。头像上传前先压到 256px（`images.ts::compressAvatarImage`）——`history/v0.8.1.md` 一.1
- 「**页面响应优先于资源节省**」：`setConfig` **先本地生效、再落盘**（等 IPC + settings.json 原子写回来才翻 UI，表现成「点一下顿一下」；失败按原值回滚）；展开态的重活**双 rAF 之后再算**（`lib/deferredMarkdown.ts`，单 rAF 仍在当帧绘制前触发）；懒加载要有预取与失败路径。任何「为了省资源而让点击变慢」的改动都不成立——`history/v0.8.1.md` 一.3/一.4/一.5
- 「**受控输入框不要每键写盘**」：名字/邮箱这类直连 `config.set` 的输入框改成**本地草稿 + 失焦/回车提交**——逐键写盘会让 `value` 回写打断 IME 组合输入（表现成「越打越多、字符乱跳」）——同上 一.2
- 「`lib/markdownTasks.ts` 是「渲染出的第 N 个任务框 ↔ 源码第几行」的唯一实现」，判据照抄 marked 的 `listIsTask`（必须 `- [ ]` + **一个空格**，围栏代码块整段跳过）；渲染器的 `checkbox` 必须**去掉 `disabled`**（disabled 的表单控件不派发 click）——同上 二

- 「无参 Svelte action 的 `update()` 不会被调用，`{@html}` 重渲换掉 canvas 后只有 MutationObserver 能接住」；「`use:fitAmount={value}` 必须传值，无参 action 的 update 不会被调用」——`ui-patterns.md`「markdown 扩展渲染」+ `ledger.md`
- 「DOMPurify 会剥掉值里含 `-->` 的属性（mermaid 箭头必中）→ 源码必须 base64 进属性」——`ui-patterns.md`「markdown 扩展渲染」
- 「`vite.config.ts` 的 `optimizeDeps.include: ["mermaid"]` 不能删」——同上
- 「markmap 全屏不能克隆 DOM——d3 的缩放/平移监听挂在原 svg 上，cloneNode 不带监听，全屏里要拿 `data-source` 重建 view」——同上
- 「**不要 `bind` 到 `{@const f = form}` 指向的对象属性**——改的是对象内部字段，Svelte 不失效 `form`，保存按钮永远停在 disabled」——`ledger.md`「v0.7.1 踩的三个坑」
- 「**注册表必须在响应式语句里直接读**——Svelte 不跟踪函数调用里的依赖，写成 `filter(isExpandable)` 那条语句一辈子不会重算」——`history/v0.7.5-v0.7.8.md` v0.7.8 ⑥
- 「设置没变 Svelte 不会重渲染 checked，得手动拨」——`sync.md`「设置面板同步卡片重排」
- 「**按钮 pointerdown 一律 preventDefault**——一抢焦点输入法就收下去了」；「列表前缀按钮与标题同口径把光标落在前缀之后」——`ui-patterns.md`「编辑器 markdown 工具栏」
- 「测量类 action 一律配 ResizeObserver 观察元素自身」（「首屏量尺寸全是 0」）——`pitfalls-android.md` 第 18 条
- 「rect 是视觉像素、`offsetHeight`/`scrollHeight` 是布局像素……两边别混用」；「所有用 clientX/Y 定位的浮层都要除以 `uiScaleValue()` 换算逻辑坐标」——`pitfalls-android.md` 第 17、6 条
- 「`isMobile` 是 writable store，不是布尔……组件里 `import { isMobile }` 后当布尔用永远为真」——`pitfalls-android.md` 第 14 条
- 「**展开态只认存储值**」：「`isExpanded = task.expanded === true`，`canExpand`（多行或标题溢出，量出来的易失值）只门控手势与按钮」；日记卡片「同样只认存储值 `entry.expanded === true`」——`ui-patterns.md`「任务卡片」「日记」
- 「配套铁律：**红叉隐藏态必须 `pointer-events: none`**……隐藏但可点的话触屏第一下往往正落在它上面，表现成「点一下标签直接删除」」——`ui-patterns.md`「任务卡片」
- 「**不能用 `margin-left:auto`**——auto margin 与 `flex-grow` 抢同一份剩余空间，两者同时存在时标签会停在中间」；「**不要写死偏移量**」——同上
- 「订阅的**初始触发不许 back**（上次会话残留的栈顶 + 空词，挂载期间动历史栈会和 WebView 初始化抢）」；「程序化跳转用 `dropSearchLayer()`（replaceState 原地撤层），否则异步 back 会和随后的 push 抢栈」——`ui-patterns.md`「全局搜索混排」
- 「每条任务的 `cardStyle` 按**它自己所属条目**取（搜索结果混着多个条目，不能拿全局 accent）」；「面板里的卡片接线全部接了（展开/编辑/勾选/日期/标签/表情/菜单），**不留按了没反应的死控件**」——同上
- 「**别拿 `clockOf` 喂裸日期**（长度 <11 的门槛刻意挡掉它：裸日期会被 `new Date()` 按 UTC 解析）」，显示用 `clock.ts::displayClock`。**v0.8.0 起 `clockOf` 用 `new Date()` 解析 + 本地 `getHours/getMinutes`**（与 core 的 `time_of` 同口径）：旧实现按 `slice(11)` 字面切片再交给带尾锚的 `parseClock`，对真实落盘的两种 createdAt（`…T04:20:41.693Z` 与 `…T12:20:00+08:00`）一律返回空串——**日记的写作时刻从来没显示过**，且对 Z 时间戳给出 UTC 钟点（东八区差 8 小时）——`history/v0.8.md` 批次 4 ①
- 「**Svelte 的 `$:` 是急切求值，与模板消不消费无关**——卡片的 `fullHtml` 必须写 `isExpanded ? renderMarkdown(...) : ""`，否则折叠态卡片也白跑完整 markdown 渲染（一屏 300 张折叠卡 = 300 次白渲染，每次列表变化都重来）」——`frontend.md` + `history/v0.8.md` 批次 2
- 「**Svelte 的失效基于赋值，Set/Map 原地改不会触发**——add/delete 之后要 `x = x` 自赋值失效（`Workspace.svelte` 的 `measuredExpandable`）」——同上
- 「测量一律走 `measureBus.ts::observeResize`（全应用共享一个 ResizeObserver + 一个 window resize 监听 + rAF 合帧），**别再每个组件自己 new RO**」——`frontend.md`
- 「`LedgerEntryRow` 的 `entryName/entryIcon/entryColor` **必须显式传参查好的对象**——Svelte 不跟踪函数调用内部的依赖，让函数自己去 `$book` 里查，book 变了那条语句一辈子不会重算」（与「注册表必须在响应式语句里直接读」同根）——`ledger.md`
- 「**用户手写的 `[文字](链接)` 一律不动**」；「图片链接跳过——换卡片等于把图丢了」——`history/v0.7.5-v0.7.8.md` v0.7.7 ⑧
- 「**改了元数据字段就把 `CACHE_VERSION` 抬一格**，否则老缓存命中不到新字段」；「缓存版本抬到 v2，旧条目（没有 icon 字段）整份作废重抓」——SKILL.md 路由表「改超链接增强」+ v0.7.7 ⑦
- 「元数据行的日期/心情/天气/标签浮层**用捕获阶段的 pointerdown+focusin 关**——对话框对 pointerdown/click 做了 stopPropagation，冒泡阶段的 window 监听根本收不到里面的点击」——`ui-patterns.md`「日记」
- 「**新建时标题与正文都空就不落盘**（空草稿关掉即消失）」；「`onDestroy` 只落盘不回调 onClose（组件正在拆）」——同上 +「输入框加号与任务编辑器元数据行」
- 「**日期不设默认值**，填了就按卡片菜单「添加日期」的语义同时写 dueDate+plannedDate」——`ui-patterns.md`「输入框加号与任务编辑器元数据行」
- 「网站拒绝被框不是配置问题……「在系统浏览器打开」按钮是唯一合理的出路」——`ui-patterns.md`「移动端（Android）」
- 「以**内联百分比**作用在 `.editor-dialog` 上——百分比相对 app-shell，窗口全屏/改尺寸时比例不变、尺寸跟着变；移动端固定铺满不给内联值（否则内联样式会压过 mobile.css 的 100%）」（编辑器尺寸比例，v0.6.9，桌面专属）；「配置面六处同步：model.rs 的 AppearanceSettings（含 NewNodeDefaults）+ ops_config.rs 的 KNOWN_FIELDS/get/set/set_default + merge.rs 共享子集 + defaults.ts 归一 + types.ts + SettingsDrawer」——`ui-patterns.md`「设置抽屉」

### 记账

- 「**金额一律整数分**（`amountCents`，对外 `amount`/`signed` 是两位小数的元；第三位小数四舍五入）——浮点累加在统计里会 drift」；「**账户余额不存现值**：= 期初 + 流水推导（转账改两个账户），改历史账目不用回头修余额」；「**转账不计入收支统计**」；「**余额始终只有「期初 + 流水」一个数据源**」——`ledger.md`
- 「**顺带修了 v0.7.0 就在的真 bug**：core `cents_param` 把 JSON 整数按「分」、小数按「元」解释，而 GUI 发 `initialCents/100`——整数元（最常见）被当成分、期初缩小 100 倍；GUI 一律改发整数分（与 amountCents 同口径）」——`history/v0.7.5-v0.7.8.md` v0.7.5 ⑩
- 「**一个动作只有一道门**」；「只读动作不设门；GUI/Android 桥恒带 `controls.yes=true` 不受影响」；「`schema.rs::risk_for` 里这些动作全是 high-risk-write」——`ledger.md`「CLI 改账本必须先过确认门」
- 「**两级封顶**」（分类只有两级，子分类下不能再挂，报 `LEDGER_CATEGORY_DEPTH`）；「名下有账的账户 core 拒删（`LEDGER_ACCOUNT_IN_USE`）」；「名字唯一（`LEDGER_ACCOUNT_TYPE_EXISTS`）」；「删类型不影响已建账户」——`ledger.md`
- 「**v0.7.2 起只有显式点保存才落盘**：X / 遮罩 / Esc / 移动端返回 / 组件卸载一律丢弃草稿」；「**编辑已有的一笔时不给「保存再记」**」；「**改一笔转账也走 `ledger.modify`**」——`ledger.md`
- 「**编辑器分类网格列数写死**（桌面 7 / 移动 5，CSS repeat 与组件里算「行尾」的 cols 必须一致，auto-fill 的不确定列数算不出行尾）」；「展开搁板只滚分类区自己：`scrollIntoView` 会连祖先一起滚」——`history/v0.7.5-v0.7.8.md` v0.7.5 ④ + `history/v0.7.5-v0.7.8.md` v0.7.7 ③
- 「一致性由 `tests/ledger_icons.rs` 守着（include_str! 前端 TS 逐个比对 + 种子账本只用目录里的图标）」；「core 侧 `ledger_icons.rs` 是它的镜像（一次性脚本生成）」——`ledger.md`
- 「**跨语言的同口径数字要有测试钉住**：日粒度门槛 62（core `DAY_GRAIN_MAX_DAYS` ↔ `ledger.ts::bucketOf`，`include_str!` 把前端源码里那个数字读出来与常量比对）、金额解析（`parse_cents` ↔ `parseYuanToCents`）、图标目录（`tests/ledger_icons.rs`）——**只写注释说「两边要一致」一定会漂**」——`history/v0.8.md` 批次 4 ②
- 「`filterLedgerEntries`：`amountNeedle` 为空**直接不匹配金额**（`includes("")` 恒真，搜一个「,」会把整本账列出来）；金额匹配**按 kind 补带符号形态**（支出 `-`/收入 `+`/转账不带符号，与 `LedgerEntryRow::amountText` 一致——`amountCents` 恒为正，带符号的查询原来永远搜不到）」——`history/v0.8.md` 批次 4 ④
- 「core `parse_cents` 对 i128 装不下的整数部分（≥39 位）一律**拒绝**（`.ok()?` + checked 算术），**不许 `.unwrap_or(0)` 静默归零**——`account-modify --initial/--balance` 里 0 是合法值，会真的把期初写成 0」——`history/v0.8.md` 批次 4 ③
- 「结果整页换成**单条卡片**（`LedgerEntryCard`，绝不渲染按天卡片）」——`history/v0.7.5-v0.7.8.md` v0.7.6 ⑩
- 「**点选环心的测试必须等动画完再量坐标**（动画中 bbox 是变换中的位置）」；「首次挂载跑一次、数据变化不重放（keyed 元素复用）」——`history/v0.7.5-v0.7.8.md` v0.7.5 ⑨
- 「**账户类型放开为自由字符串**（core 校验只要求 trim 非空，负债判据 `kind == "credit"`）」；「`defaults.ts` 的 normalize 对 kind 只兜底缺省、对 `image` 原样保留（漏了这两条，浏览器预览与核快照会把自定义类型压回 cash、把图片吃掉）」——`history/v0.7.0-v0.7.4.md` v0.7.4 ⑨
- 「`parseYuanToCents` 放开负数（信用卡）」；「保存发 `balance`（与 `initial` 互斥，同发报 `LEDGER_PARAM_CONFLICT`）」——`history/v0.7.5-v0.7.8.md` v0.7.5 ⑩
- 「**两个管理器打开都不 autofocus 名称输入框**（一进去就拉输入法是打扰）」——`history/v0.7.0-v0.7.4.md` v0.7.4 ⑩
- 「桶类型判定写反（day 键长 10 被 `>7` 判成 month 桶，`Number("09-14")`=NaN）——正确判据 `key.length === 7 ? month : day`」——`history/v0.7.5-v0.7.8.md` v0.7.5 ⑤

### 图片 / 导入导出 / 清理

- 「**清理永远不许弄失败保存本身**」；「IO 错误全吞、目录不存在直接返回 0」；「**真正的不变式是「清理只发生在本地写盘之后、且引用集就是刚写进去的那份 markdown」**」；「同步合并走 `repo::write_*`（`sync/engine.rs`），从不触发清理，所以「图片先到、实体后到」的在途图根本不会被扫到」——`ui-patterns.md`「一般卡片的 Markdown 压缩包」
- 「**v0.7.2：图只跟引用走**——导出本来就只打包正文引用得到的图；导入（zip 与文件夹两条路共用 `finalize_cards`）只落**被任一 md 引用**的图，包/目录里没人指的图直接剔掉，孤儿图从源头不生」——同上
- 「**已存在的同名文件不覆盖**（图片是内容寻址的不可变 blob，重导必须幂等）」；「远程链接与 `data:` 内联图永远不碰」；「包内文件名落盘前过 `is_safe_image_name`（无分隔符/`..`/控制字符/长度有界）」——`ui-patterns.md`「日记」的「导入导出是给人看的 Markdown 树」
- 「**全程在内存解压，不落文件系统，所以没有 zip-slip**，但有解压炸弹护栏（200MB/2万文件/单文件 20MB）」；「导入**两遍扫描**（先收 images/ 再解析 md——包内 images/ 排在 md 之后，单遍扫描引用归一永远看不到图）」——同上
- 「标题过滤掉路径分隔符与 Windows 保留字符、反复剥前导点（「.. .. x」这种点与空格交错的要循环剥）」；「2 月 30 日这种要拒」——同上
- 「**重复导入会产生重复账目**（确认门）」；「导入只认这套表头」——`ledger.md`「Excel 归档」
- 「**同一天已有日记不算冲突**：直接追加成另一篇，不合并正文也不去重（所以同一个包导两遍会得到两份——导入因此走确认门）」——`ui-patterns.md`「日记」
- `storage.clean` 的「**边界写死在模块文档里**：五个域 JSON、`runtime/*`（同步水位/设备密钥/配对历史）、服务器 `settings.json`/`data.db`（账户与 token 哈希住在库里）一概不碰」；「单个 IO 失败进 `warnings` 不硬失败」——`ui-patterns.md`「设置抽屉」
- 「Android 的 dialog save() 只给 content:// URI，所以导出不传 path、让 Rust 落进缓存目录再由 Kotlin `shareFile` 桥拉起分享面板，导入走隐藏 file input + **base64**（不用 Vec<u8>：JSON IPC 会把 1MB 摊成一百万个数字）」——`ui-patterns.md`「日记」
- 「**图片入库三道闸门**（`ImageGate`）：插图（Markdown）只在超 5MB 时动手；**背景**超过 2MB 就压、压完仍超 **10MB 直接拒收**（那种图留着只会每次都卡，不如当场说清楚）；**头像一律收到 256px**（不分来源大小）。三道都只处理 JPEG/PNG，GIF/WebP 可能是动图一律原样，解码失败/重编码失败/没变小也原样返回」——`history/v0.8.1.md` 一.1 与二
- 「**背景图长边上限 `BACKGROUND_MAX_EDGE = 2560` 前后端各有一份**（`src/lib/images.ts` 与 `lib.rs::ImageGate::Background`），改要一起改；上传前前端先压一道，为的是别把几十 MB 的 dataURL 推过 IPC」——同上
- 「**服务器 WAL 要定期 `wal_checkpoint(TRUNCATE)`**：SQLite 的自动检查点只把页挪回主库、**不缩小 WAL 文件**，内置主机在手机上常年只增不减。那个后台任务**必须能被停机叫醒**（watch 通道），否则它握着 SQLite 连接不放，Windows 上主机库目录删不掉」——`history/v0.8.1.md` 二
- 「服务器日志**单文件上限 8MB**（保留策略只管文件数、不管单文件多大，一个卡在错误重试里的客户端一天能堆出几百 MB）；到顶只停写文件、stdout 照旧」——同上
- 「插图复用现成的按条目分目录通道，伪条目 id = `diary`（`diary.ts` 的 `DIARY_IMAGE_NODE`），图片存储与同步一行都不用改」；记账是 `ledger`（`LEDGER_IMAGE_NODE`，「core 的 inventory 是动态 read_dir 所以自动进同步与盘点」）——`ui-patterns.md`「日记」+ `history/v0.7.0-v0.7.4.md` v0.7.4 ⑦

### 构建 / 发布 / CI

- 「**一切 cargo 调用走 `scripts/cargo-msvc.sh`**」；「**`npm run desktop:dev` 同样中招**（`tauri dev` 内部调裸 cargo，会刷一屏 `link: extra operand` 然后构建失败）」——`pitfalls-windows.md` 第 1 条
- 「**跑 release/publish 脚本不要用 `| tail` 管道**——管道会把退出码掩成 0 造成"构建成功"误报，重定向到日志文件再 tail」；「构建输出同样不要接 `| tail` 之类管道（掩退出码），release.sh 一律直通终端」——`pitfalls-windows.md` 第 8 条 + `pitfalls-linux.md` 第 5 条
- 「Linux 制品一律走 `npx tauri build`（release.sh 已如此）」；「裸 `cargo build --release` 产出的二进制会去加载 `devUrl`（127.0.0.1:1420），没有 vite 服务时整窗空白」——`pitfalls-linux.md` 第 2 条
- 「**不要手工跑 gradlew**」；「**版本一律走 package.ps1 注入的 `KXTODO_VERSION` 环境变量**，别信 tauri.properties」；「`generated/`、`tauri.build.gradle.kts`、`build/` 每次构建再生成，别改」——`pitfalls-android.md` 开头与第 1 条
- 「**仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）——发版只打 tag/写 commit，永远不要往文件里同步版本号」——本文第四节
- 「① **精确 tag 必须排在祖先 tag 之前**」；「② **subject 校验只能取第一个空白前的 token**」；「都已修，别改回去」——本文第四节
- 「`package.ps1` 侧必须用 `-cmatch`（PowerShell 的 `-match` 默认大小写不敏感，会放行 `V1.3.0`）」；「hook 文件在 `.gitattributes` 里钉了 `scripts/git-hooks/** eol=lf`」；「merge / revert 等自动生成的消息用 `git commit --no-verify` 显式绕过」——本文第五节
- 「**workflow 级 `PYTHONUTF8=1` 不可删**：Windows runner 的 Python 默认 cp1252 stdout，core 调度测试的内联脚本 `print('中文')` 会 UnicodeEncodeError 退出 1 挂掉 3 个 host_scheduler 测试」——`build-and-release.md`「ci.yml」
- 「**产物暂存必须用独立 `staging/`，禁止复用仓库根 `dist/`**」；「构建 job 一律 `fetch-depth: 0`」；「release job 另有七固定名白名单硬门控」——`build-and-release.md`「release.yml」
- 「**本地 keystore 与 secret 同时丢失 = 旧安装升级链断裂**」；「改公式前先确认单调递增，否则安装器报降级拒装」——`build-and-release.md`「Android 签名」+ `pitfalls-android.md` 第 9 条
- 「**wmi 版本钉死**……`Cargo.toml.lock` 里钉 `wmi 0.18.1`（两个范围都是 ^0.62，内部自洽）」；「升级 iroh 或重新生成 lock 后若见这类错，先 `cargo update -p wmi --precise 0.18.1`」——`sync.md`「踩坑记录」⑥
- 「③agent 只设连接超时 + **读超时 60 秒**，刻意不设总超时」；「①每次尝试都校验体积下限（`MIN_ARTIFACT_BYTES` 1MB，真实产物最小 20MB 上下）与 `received == content-length`，**体积不对/截断也算失败**——否则一个 200 的错误页会被当成产物」；「②换通道前清掉上一次留下的 `.part`」——`build-and-release.md`「应用内更新的下载通道」
- 「手写 `/MANIFESTINPUT` 链接参数会造成 side-by-side 启动错误，勿回退」——`sync.md`「踩坑记录」④
- 「Node 固定 22（规避 node 24 在 Windows 的退出期 libuv flake）」；「**偶发，直接重跑该目标即可**（产物以 release/ 下文件为准）」——`build-and-release.md`「ci.yml」+ `pitfalls-windows.md` 第 8 条
- 「Android 交叉编译重，不进 push CI（tag 发布时才构建，与 tauri 官方 test-android.yml 同思路）」——`build-and-release.md`「ci.yml」
- 「**`[profile.release]` 不许加 `panic = "abort"`**」（GUI 是常驻 Host，一次 panic 直接带走整个常驻进程比 unwind 到边界更糟；Android 的 cdylib 上 abort 也不友好）；`lto="thin"` / `codegen-units=1` / `strip=true` 是 v0.8.0 起的体积基线——`build-and-release.md` + SKILL.md 3.6
- 「**APK 只构建 aarch64 + armv7 两个 ABI**，别为了「万一有人用模拟器」把 x86/x86_64 加回来（纯为模拟器服务却占掉 APK 一半体积；移动端回归验证走 Playwright + 系统 Edge，不依赖 x86 APK）」——`build-and-release.md`
- 「**前端单测 `npm run test:unit`**（vitest 5，node 环境，独立 `vitest.config.ts` 刻意不复用 vite.config.ts）；**断言必须时区无关**（CI 的 ubuntu 是 UTC、开发机是 UTC+8）；ci.yml 的 frontend job 插在 `npm run check` 之后、`npm run build` 之前」——`frontend.md`「前端单测」
- 「**改 `skills/kxtodo/SKILL.md` 必须重跑 `kxtodo-cli skills validate`**（正则把任何 `task|diary|schedule|config|skills` 后跟的小写英文词当命令名、任何 `--xxx` 当参数名去比对目录，引用不存在的子命令或参数直接挂测试）」——`cli.md`「Agent 技能文档」
- 「**文档真身住在 `.agents/`**（`.agents/AGENTS.md` + `.agents/skills/`），`.qoder/AGENTS.md` 与 `.qoder/skills` 是指向它们的软链接，仓库根目录只留面向用户的 `README.md`——改文档一律改 `.agents/` 下的真身。`.gitignore` 里 `.qoder/` 必须写成 `.qoder/*` + `!.qoder/AGENTS.md` + `!.qoder/skills`（**整目录被排除时 gitignore 的 negation 对子路径无效**，否则这两个软链接进不了仓库；`skills` 那条不能带尾斜杠——它现在是软链接不是目录）。`core.symlinks=false` 的 Windows 克隆会把软链接落成一个内容为目标路径的文本文件，那份不可用」——`history/v0.8.md` 批次 7

### 平台与窗口

- 「**调试前先把所有 kxtodo/KXToDo 进程杀光（含托盘）**」；「用户反馈"修复没生效"也优先怀疑旧进程残留」——`pitfalls-windows.md` 第 4 条
- 「必须用 `with_filter` 按前缀排除」；「还要把 `StateFlags::VISIBLE` 从持久化里剥掉，否则恢复逻辑会绕过 reveal 提前显示窗口」——`pitfalls-windows.md` 第 5 条
- 「`setup_tray` 的失败**不再上抛**（曾经直接把整个程序弄崩），只记日志并把 `LifecycleState.tray_available` 置 false」；「`close_to_tray && tray_available` 才隐藏，否则退出」——`ui-patterns.md`「Linux 桌面」
- 「`onCreate` 因此传 `super.onCreate(null)`」；「返回键退出走 `exitApp()` = `finishAndRemoveTask()` + 250ms 后 `Process.killProcess`」；「配套加固：返回回调 400ms 去抖、`isFinishing` 守卫、`onDestroy` 置空 webViewRef」——`ui-patterns.md`「安卓退出生命周期」
- 「**release 开了 minify，必须在 proguard-rules.pro 加 keep 规则**否则桥方法被摇掉」；「installApk 前校验路径 canonical 后落在 cacheDir 内」——`pitfalls-android.md` 第 3 条
- 「移动端导出/落盘一律改 Kotlin 桥（shareText 写 cacheDir + ACTION_SEND）或 `<input type=file>` + dataURL 命令」——`pitfalls-android.md` 第 4 条
- 「嵌套容器（行 + 外层 nav）会各武装一个长按定时器，外层 handler 必须检查抑制标志/内层菜单已开，否则行菜单被空白区菜单顶替」；「长按抬手补发的 click 也要吞掉」——`pitfalls-android.md` 第 5 条
- 「能力门控优先于 isMobile 散落判断」；「新增平台只扩这两处 + CSS 命名空间，不改业务组件」——`pitfalls-android.md` 第 13 条 + `frontend.md`「capabilities.ts」
- 「必须在 `MainActivity.onCreate` 自己注册 `OnBackPressedCallback`（canGoBack→goBack 否则 finish），否则硬件返回直接退应用」——`pitfalls-android.md` 第 2 条
- 「**每一个浮层都要接管系统返回键，而且接管了必须能还回去**」：拦截器是**后进先出的栈**，所以「先注册的在下、后注册的先被问」天然对应「后开的先关」。收放都靠 `platform.ts::createBackGuard`——`{#if}` 挂载式的浮层（菜单 / 选择器 / 日期选择器）在 `onDestroy` 里 `dispose()`，`open` 是 prop 的在 `$: backGuard(open, onClose)` 里自动收放。漏 dispose 的症状是**返回键失灵**；漏注册的症状是**返回键跳过当前浮层去弹下面的页面**——`history/v0.8.1.md` 一.6
- 「tauri-plugin-window-state 默认管所有窗口」；「`reveal_main_window` 在 show 之前跑 `sanitize_main_window_geometry`」——`pitfalls-windows.md` 第 5 条
- 「全局快捷键受插件平台能力限制（X11 可用，纯 Wayland 抓不到），不做会话嗅探特判」；「Linux 首跑默认退出……用户设置后以用户值为准」——`ui-patterns.md`「Linux 桌面」
- 「改成 `is_focused`：已在前台就收起，隐藏/最小化/被挡住一律 show + unminimize + set_focus」——`history/v0.7.5-v0.7.8.md` v0.7.8 ⑪
