---
name: develop-kxtodo
version: 1
description: >-
  开发、修改、调试、重构 KXToDo（Todo Note）项目本身时必须先加载本 skill。
  KXToDo 是一款本地优先的待办加日记加记账三端应用：Rust 与 Tauri 2（桌面壳与后端）、
  Svelte 4 与 TypeScript 与 Vite（前端）、CodeMirror 6（编辑器）、
  marked 加 DOMPurify 加 highlight.js（渲染），同一份 kxtodo-core 跑在
  Windows（exe）、Linux（AppImage）、Android（APK）三端。
  只要在本仓库动任何代码就用它：改前端 Svelte 组件或全局 CSS、改 Rust core
  （model、repo、ops 系列）或 src-tauri 壳、改数据同步（core/src/sync 与 crates/server）、
  加或改 CLI 命令、改任务/日记/记账域、加设置项、排查移动端浮层与手势问题、
  跑测试、构建打包发版（release.ps1、release.sh、GitHub Actions）、打 tag、推送远程、
  以及遇到 Windows、Linux、Android 的环境坑位（Git Bash 里 cargo 报 link extra operand、
  pwsh 5.1 编码、裸 cargo 出白屏制品、gradle 与 NDK、单实例标识撞车）。
  采用渐进披露：本文件常驻（架构精简版、全部铁律、路由表、references 索引），
  细节按需读 references 下的 11 份专题文件与 history 下 9 份版本档案（共 20 份 md）。
  注意：本 skill 是开发本项目用的；要用 KXToDo 的 CLI 记录待办、日记、账目，
  那是另一个 skill（kxtodo）。
---

# 开发 KXToDo

## 0. 怎么用这份 skill（渐进披露）

- **本文件常驻**：项目概览、架构精简版、**全部铁律**、路由表（要做什么 → 动哪里）、references 索引、自我迭代条款、最短构建路径。
- **细节按需读**：`references/` 下 11 份专题文件 + `references/history/` 下 9 份版本档案（见第 5 节的索引表）。动手前按路由表最后一列的指引去读对应文件；拿不准某条约束为什么存在，读 `references/invariants.md`。
- **版本流水账在 `references/history/`**：本文件**不**堆「vX.Y 改了什么」，那是 history 的职责。

## 1. 项目是什么

KXToDo（Todo Note）是一款本地优先的待办 + 快捷笔记桌面应用：左侧分类/条目树、右侧 Markdown 卡片画布（交互参考 Microsoft To Do，品牌与实现完全原创），外加 CLI、定时任务与系统通知。技术栈：Rust + Tauri 2（桌面壳与后端）、Svelte 4 + TypeScript + Vite（前端）、CodeMirror 6（编辑器）、marked + DOMPurify + highlight.js（渲染）。

后来长成的样子：**待办 + 日记 + 记账**三类内容（各住自己的领域文件），**Windows / Linux / Android 三端**同一份 core，外加**多端数据同步**（自建 server / 局域网内置主机 / iroh P2P 三种通信方式，端到端加密）。

- **Windows**：`KXToDo.exe`（GUI）+ `kxtodo-cli.exe` + `kxtodo-server.exe`
- **Linux**：`KXToDo.AppImage` + `kxtodo-cli` + `kxtodo-server`（与 Windows 同拓扑：GUI 常驻 Host，CLI 经 IPC 找 Host）
- **Android**：`KXToDo.apk`（内嵌同一个 kxtodo-core，HostCore 进程内直跑，不启 IPC server / 调度引擎 / 看门狗 / 托盘）

## 2. 系统架构（精简版）

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
   <数据目录>/data.json + settings.json + tasks.json + diary.json + ledger.json
```

- **GUI 是唯一常驻进程**：窗口关闭（默认隐藏到托盘）后 Host 继续跑调度与 IPC；无窗口、无启用任务时看门狗自动退出。
- **CLI 不持有状态**：需要常驻能力时经 IPC 找 Host；Host 不在就拉起 GUI 同目录 exe 的隐藏 Host 模式（`--kxtodo-host`），找不到 GUI 报 `GUI_NOT_FOUND`。
- **Android 同栈**：APK 内嵌同一个 kxtodo-core（`init_mobile_core`），写操作与桌面走完全相同的 core_dispatch 命令层。浏览器 dev 预览（非 Tauri）才走 localStorage legacy 路径。

**路径约定**：文档里写的 `crates/core/src/...`、`crates/server/src/...` 一律**相对 `src-tauri/`**（磁盘上是 `src-tauri/crates/...`）；`src/...`（前端）与 `scripts/...` 相对仓库根；Tauri 壳在 `src-tauri/src/lib.rs`。详见 `references/architecture.md`。

### 五个领域文件

`data.json`（节点与任务）+ `settings.json`（设置）+ `tasks.json`（定时任务）+ `diary.json`（日记）+ `ledger.json`（记账）。每个都有独立的 revision / 幂等台账 / 墓碑 / 域事件；`Domain` 变体、layout 路径、`load_*` / `write_*` 都在 `crates/core/src/repo.rs`。**数据地基（schema v6）**：ID 是 128-bit 随机 hex（32-bit 会跨设备碰撞）；Node/Item 有显式 `order: f64`（同级排序唯一来源，数组顺序仅是渲染缓存）；`collapsed`/`expanded` 是本机 UI 状态不参与同步。

### 同步分层一句话

每台设备持**全量副本**，主机（任一台设备上的 kxtodo-server，或内嵌在 GUI/APK 里）只是**密文中转缓存，不是数据归属者**；逐实体 LWW + 墓碑传播删除，合并**永远在客户端**。代码分层：`crypto`（密钥派生+加解密）/ `merge`（LWW 纯函数）/ `transport`（HTTP 客户端，三方式共用）/ `endpoint`（「这一轮连哪儿」）/ `engine`（编排）——**换通信方式只动 endpoint，不动内核**。细节全在 `references/sync.md`。

## 3. 不变式与铁律（改任何代码前都要过一遍）

完整的「为什么」与其余 100+ 条分散硬约束在 **`references/invariants.md`**。下面是必须常驻的部分。

### 3.1 写路径

- **写路径永远过 Domain Core 命令层；前端永不直接改 JSON；GUI 桥接默认 `controls.yes = true`**（GUI 操作即用户确认，CLI 的确认门不适用于 GUI）。
- 「**GUI/CLI/Agent 三方写操作走同一条业务命令层**（Domain Core 的 Invocation → 域分发 → envelope 输出），不存在"读全量 JSON 改完写回"的路径。」
- 前端 `actions.ts`：「**新写操作一律加在这里，不要在组件里直接改 store 或 invoke。**」
- **纯 UI 写命令不进 audit 台账**（`repo.rs::UI_ONLY_COMMANDS` 六个：`gui.select-node`/`set-collapsed`/`set-item-ui`/`set-items-ui`/`set-diary-ui`/`set-schedule-ui`）——点一下树节点就写一行审计，台账会被 UI 噪音灌满；**新加纯 UI 命令要进这个白名单**。
- **备份是五个领域文件全量**（`repo.rs::backup_locked`）；恢复靠手工从 `backups/` 拷回覆盖，**没有自动恢复命令**（`restore` 与 LWW 语义冲突，明确不做）。
- **`core_snapshot` 可按域过滤**（前端 `refreshFromCore` 只拉脏域）：新加领域文件必须把它加进 `lib.rs::core_snapshot` 的 `wanted()` 名单与 `put_snapshot_domain` 调用，否则前端永远拉不到它；前端 `applySnapshot` 各分支带 `!== undefined` 守卫，缺的域不能被当成空。
- **写命令回来后必须记信封 revision**（`actions.ts`：`noteEnvelopeRevision(envelope.meta)`）：写命令的信封带 `meta.revisionDomain`/`revision`，记下水位后 `applySnapshot` 才能按域判断「这份快照我已经有过了」并跳过 `set`。漏记的症状是**一次写入让 store 换两次身份**（乐观更新一遍、随后的域事件又应用一遍快照）→ 所有 `$:` 重算、图表出场动画重启（v0.8.2 的饼图掉帧与「日历→统计闪一下」就是这个）。`gui.*` 命令一律不发域事件，不用记；带 `expectedUpdatedAt` 冲突检测的写命令（`task.modify` 这类）**刻意不记**，否则自己的下一次写入会被误判成冲突。
- **图片入库有 5MB 体积闸**（`lib.rs::shrink_oversized_image`）：超限才动手（JPEG 长边 2560/质量 90、PNG 长边 4096 保无损）；**GIF/WebP 一律不动**（可能是动图）；解码/编码失败或没变小一律原样保留——**压缩永远不许弄丢或弄坏用户的图**。

### 3.2 跨平台铁律（三条）

KXToDo 是 Windows / Linux / Android 三端应用，**任何更改、新增功能、bugfix 都必须按跨平台审视**：

1. **底线**——不能让任何一端变得不可用（编译不过、启动白屏、核心路径坏掉都算）；平台专有代码必须 `#[cfg]` / capabilities 隔离，改共享层时逐端过一遍影响面。
2. **进阶**——考虑该功能是否需要适配其它平台：能力差异收敛到 `capabilities.ts` + Rust `#[cfg(desktop)]`/`#[cfg(not(desktop))]`，UI 差异收敛到 CSS `.app-shell.mobile` 命名空间与平台覆盖 conf（如 `tauri.linux.conf.json`），不要在业务组件里散落平台判断。
3. **验证**——本地验证不了的平台（如无 WSL 时的 Linux、无真机时的 Android）交给 CI（ci.yml 双平台编译检查 / release.yml 三平台构建），但**必须明说"该端未验证"，不许默认没问题**。

### 3.3 不做兼容

项目没有正式 release，不存在需要兼容的旧版用户。**数据格式、数据位置等变更一律直接切换，不写迁移/兼容代码**；旧数据由用户自行迁移或丢弃。（唯一例外：记账 v0.7.4 的单数 `image` 字段在加载 normalize 时一次性折叠进 `images`——用户真实账本里的图不能丢，重写后旧键不再落盘。）**v0.8.0 已把最后一份迁移代码 `migrate.rs`（v8→v9）整个删掉**（连带 `repo.rs` 的 `migrate_if_needed`、`time.rs::migrate_legacy_local_time`、`exec.rs::split_legacy_arguments`）——这条铁律现在连迁移代码都不留，别把它加回来。

### 3.4 版本号只有 git 一个来源

**仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）——发版只打 tag/写 commit，**永远不要往文件里同步版本号**。解析优先级：HEAD 上的精确 `v*` tag → 最近 30 条 commit 里第一条 `vX.Y.Z` 主题 → 最近的祖先 `v*` tag，都没有回落 `0.0.0-dev`。改这套解析前必须知道的两个真实 bug（都已修，**别改回去**）：① **精确 tag 必须排在祖先 tag 之前**；② **subject 校验只能取第一个空白前的 token**。

### 3.5 commit-msg hook 强制 subject 以 vX.Y.Z 开头

判据与解析器完全一致：**小写 `v` + 恰好三段纯数字**，所以 `v1.2`（两段）、`v0.4.2.1`（四段）、`V0.4.2`（大写）都拒。源文件版本化在 `scripts/git-hooks/commit-msg`，`npm install` / `npm ci` 的 `prepare` 把它拷进本克隆的 hooks 目录（不用 `core.hooksPath`）。`.gitattributes` 钉了 `scripts/git-hooks/** eol=lf`——CRLF 检出会把 shebang 变成 `#!/bin/sh\r`，hook 静默失效等于整条强制形同虚设。`package.ps1` 侧必须用 `-cmatch`（`-match` 默认大小写不敏感会放行 `V1.3.0`）。merge / revert 等自动生成的消息用 `git commit --no-verify` 显式绕过。

### 3.6 发布工作流铁律（四条，Agent 与维护者都要遵守）

1. 本地构建只用 `release.ps1`（Windows/Android/unix）与 `release.sh`（Linux 原生）；`publish.ps1` 基本不再使用，仅作离线/CI 不可用时的备用路径。
2. **每次推送远程前必须询问用户：这一版是否需要打 tag（即是否发 release）**。要打则 commit → 在 HEAD 打 `vX.Y.Z` tag → 一并推送分支与 tag；不打则只推分支。
3. **同一版本的修复只能 amend，不许新开 commit**：在用户没有明确指定"这次修复要升版本号"之前，所有修改（bugfix、文档补充、CI 修复、遗漏的改动）一律 `git commit --amend` 并进那笔 `vX.Y.Z` commit，保持"一个版本 = 一笔 commit"。已经打了 tag 的按顺序来：`git tag -d vX.Y.Z` → amend → 在 HEAD 重打（tag 已推远程则 force-push tag，并按需处理对应的 GitHub release）。另起一笔 `vX.Y.Z 修复…`、或不带版本号前缀的散 commit，都算违规。
4. **不论是否打 tag，推送后必须监听远程 CI 直到结束**（ci.yml 编译检查；打了 tag 还有 release.yml 三平台构建 + 发布）：确认检查通过、构建/发布成功；任何失败都必须后续处理（修复重推或 re-run failed jobs），不许放着红着不管。

体积相关的两条硬约束（v0.8.0 起，理由在 `references/build-and-release.md`）：**APK 只构建 aarch64 + armv7 两个 ABI**——x86/x86_64 纯为模拟器服务却占掉 APK 一半体积，别为了「万一有人用模拟器」加回来；**`[profile.release]` 不许加 `panic = "abort"`**——GUI 是常驻 Host，一次 panic 直接带走整个常驻进程比 unwind 到边界更糟（Android 的 cdylib 上 abort 也不友好）。

### 3.7 CSS 铁律

- **绝不写 `.xxx > *`**（「把直接子元素统一压到 `position:relative; z-index:1`」这类兜底规则）。要抬层就**显式列举正文流子元素**。两种死法都是特异性相同（`*` 不计特异性）谁在后面谁赢，曾让整个日记界面的浮层与浮动按钮位置全错乱。最后一处违例 `.workspace > *` 已在 v0.8.0 删除（显式列举 `.list-subtitle`/`.task-list`/`.add-task-bar`/`.scheduler-panel`，自带定位的浮层与头部刻意不列）。
- **字号一律 `calc(var(--font-*) ± N)`，不许写死像素**（v0.7.0 就是字号各写各的被用户点名）。记账与日记吃自己独立的 `--font-ledger` / `--font-diary`。**v0.8.0 起已全面收编：除 `base.css` 的 5 个 `--*-font-size` 变量定义外，CSS 里不许再出现 `font-size: Npx`**；基变量按区域选（ledger 页 `--font-ledger`、日记页 `--font-diary`、其余 chrome `--font-control`）。唯一例外是 `ledger/AssetsTrend.svelte` 的内联 `axisFont`（SVG 按 viewBox 整体缩放，它是按容器宽度补字号的，用 CSS 变量会被二次缩放）。
- **按钮只许 `settings-button` / `menu-action-button` 两类**（危险动作加 `.danger` 变体）；菜单项一律走 `MenuItem` 组件（menu-item-button），不写裸 `<button>`；「新写任何按钮前先想这两个类能不能用；**风格不一致的裸按钮视为 bug**」。
- **级联顺序固定**：`main.ts` 按 base → titlebar → sidebar → workspace → settings → menu → shared → editor → diary → mobile 导入（**mobile 必须最后**，它覆盖前面所有区域）。同名类用父选择器区分。「移动端样式全部收在 `.app-shell.mobile` 下，桌面零副作用」。
- 全局 CSS 非 Svelte scoped（`{@html}` 渲染的 Markdown 没有 scoped 属性，触及不到）。
- **别拿全局类名当状态类名**（`.collapsed` 曾被代码块折叠态复用，整块代码被转 90°）。**夹行只写 `-webkit-` 三件套**，别「两个都写以求兼容」。

### 3.8 浮层、安全区与返回键

**任何「JS 命令式建的全屏/浮层」都要先问两句**：① 移动端顶部避让了吗？（`env(safe-area-inset-*) × var(--safe-inv)`；浮层要挂进 `.app-shell` 而不是 body，否则拿不到 `--safe-inv`——这条安全区坑**已经踩过三次**：v0.6.8 编辑器全屏、v0.6.8 链接预览标题栏、v0.6.9 图全屏工具栏）；② **系统返回键接管了吗、还回去了吗？**

第二条用 `platform.ts::createBackGuard` 一句话搞定，但**两种写法不能混**：

```svelte
// 由调用方 {#if} 挂载式（菜单 / 图标选择器 / 日期选择器）——「挂着就等于开着」
const backGuard = createBackGuard();
$: backGuard(true, () => close());

// open 是 prop 的（Dropdown / MonthPopover）——收放自动
$: backGuard(open, onClose);
```

**`createBackGuard()` 内部注册了 `onDestroy`**：组件一销毁（不管是因为浮层关了、还是开着浮层时整页被切走）拦截器就自动摘掉，不需要调用点自己记得。**别绕过它直接用 `addBackInterceptor`**——那个要手动配对注销，漏一次的症状是**返回键被一个已经看不见的浮层永久吃掉**（v0.8.1 踩过：菜单开过一次之后，整页返回键再也没反应）；另一个方向的症状是**返回键跳过当前浮层去弹下面的页面**（漏注册，v0.6.9 起一批浮层补过）。两个方向症状完全不同，排查时先问「当前有几层、哪一层该吃掉这一记」。

配套：`{#await import(...)}` 一律配 `{:catch}`（懒加载失败不能让用户「点开什么都没有也退不出去」）；不占历史栈的覆盖层还要注册 `addBackInterceptor`（`createBackGuard` 就是它的封装），否则安卓返回键会把底下的页面弹掉而浮层留在原地。

**多层浮层要逐级退，一层一记返回**（v0.8.2）：`AccountManager` 是三层（账户类型小表单 → 账户表单/转账 → 列表 → 关），拦截器里就按这个顺序判；还要区分「从列表点进去的」与**直达表单**的（`startedOutsideList`）——从资产页点「添加」直接落在表单上时，返回该把整层关掉，而不是退到一个用户从没见过的列表面板。

**懒加载的浮层要加 store 级兜底 guard**（`platform.ts::startMobileRouter` 里对 `taskEmojiPicker` 就是这么做的）：组件挂载后自己的 `createBackGuard` 才注册，chunk 在途的那段窗口（首开、冷缓存）按返回键会直接把底下的整页弹掉。两层不冲突——组件的 guard 注册得更晚，按「后注册的先问」它先被问到。

**测返回键一律走 `window.kxtodoBackHandler()`**：安卓硬件返回键的真实链路是 MainActivity 的 `OnBackPressedCallback` → evaluateJavascript 调这个函数 → 返回 true 就吃掉、false 才让 WebView 退历史/finish。测试里直接 `page.goBack()` 量的是**历史栈**，压根问不到浮层拦截器（`scripts/v082-fixes-test.mjs` 的 `pressBack()` 是现成写法）。同理，左上角返回箭头 `goBackLevel()` 也必须先 `consumeBackInterceptors()` 再 `history.back()`。

### 3.9 测试与一致性地基（v0.8.0 起）

- **前端单测 `npm run test:unit`**（vitest 5，node 环境，独立 `vitest.config.ts`——刻意不复用 vite.config.ts）；**断言必须时区无关**（CI 的 ubuntu 是 UTC、开发机是 UTC+8，一律用 `todayDate()`/`shiftDays()` 相对构造）。地基在 `references/frontend.md`。
- **跨语言的同口径数字要有测试钉住**：日粒度门槛 62（core `DAY_GRAIN_MAX_DAYS` ↔ `ledger.ts::bucketOf`）、图标目录（`tests/ledger_icons.rs`）、金额解析（`parse_cents` ↔ `parseYuanToCents`）。手法是 `include_str!` 前端 TS 源码直接比对——**只写注释说「两边要一致」一定会漂**。
- **core 加字段，前端 `normalize*` 必须同步加**（`defaults.ts` 的 `normalizeTask`/`normalizeNode`/`normalizeSettings` 是逐字段白名单）：漏一个就等于「每次快照刷新都把用户的值抹掉」。`dueTime` 漏了两个大版本才被发现——它只在「设过时刻 + 触发过一次快照刷新」时才现形（症状是日期浮层的「精确到分钟」勾选框勾不上、勾上又弹回来）。
- **改 `skills/kxtodo/SKILL.md` 必须重跑 `kxtodo-cli skills validate`**：`cmd_validate` 的正则会把任何 `task|diary|schedule|config|skills` 后跟的小写英文词当命令名、任何 `--xxx` 当参数名去比对目录，文档里写一个不存在的子命令或参数会直接挂测试。

### 3.10 首帧与响应（v0.8.1–v0.8.2）

三条硬优先级，排在「省资源」前面：

- **首帧不许闪**：一切首帧可见的东西都要进 `localStorage` 缓存——**四件套**：`appearance`（整个对象，别只白名单数字字段——冷启动「先单列再跳双列」就是这么来的）/ `profile`（写失败要**退一步保住能保的**：头像撑爆配额时仍然写名字与邮箱）/ **`features`**（v0.8.2 补：状态缓存让卡片第一帧就画出来，开关若还是默认值，临期底色与链接样式都会「先按默认画一遍再改回来」）/ **`state`**（节点树 + 任务 + 选中节点 + 背景；防抖 800ms、剥 scheduler、超配额三档降级、`visibilitychange`/`pagehide` flush、`isHydrated` 门控 + 水合完成补写一次）。另有头像缩略图缓存（160px，同步种子）。**所有缓存写入一律先 stringify 比对，值不变一个字节都不写**——同步 `setItem` 是同步磁盘 I/O，落在动画帧里就是掉帧。水合是异步的，首帧只能用缓存。
- **点击不许顿**：`setConfig` **先本地生效、再落盘**；展开长卡片走**两阶段渲染**（`renderMarkdownFast` 同步上屏：跳过 hljs、公式摆回转义源码；双 rAF 后完整版升级，两版逐字节相同就跳过第二次 `apply`）——**短文档（< 1000 字）一条老路走到底，不为长文档付代价**；懒加载组件要预取 + 有失败路径。任何「为了省资源而让点击变慢」的改动都不成立。
- **命令式 DOM 增强必须可逆**（v0.8.2）：渲染之后对 DOM 做的破坏性增强（换节点、覆写文字）要留退路（`linkPreview.ts` 用两张 WeakMap 存原节点与原文），并且**落地前重读当前设置**（抓取在途时用户可能已经拨了档）。理由是 markdown 有记忆化：设置变了也不会重渲，靠「等下次重渲纠正」等于永远不纠正。同理，**首帧不许拿默认设置做不可逆的 DOM 决定**——这也是开关必须进首帧缓存的原因。

### 3.11 五条新的（v0.8.3）

- **跨语言的「同一个数字/名单」改一处必须扫全部，并且用 `include_str!` 钉住**。临期配色从三档变四档要改四处（`dueHighlight.ts::DEFAULT_DUE_COLORS`、`defaults.ts::normalizeDueColors`、core 的 `expect_due_colors`、色盘 UI），漏掉 core 那处的症状是「界面能选四个色、保存必失败」。钉子住在 `crates/core/tests/frontend_contract.rs`（TagColor 十色的三处名单 / `BACKGROUND_MAX_EDGE` ↔ 壳的背景闸 / 配色档数 ↔ core 校验）；**新加孪生常量就往那里加一条**，别只写注释说「两边要一致」。
- **乐观更新必须自带回滚**：`actions.ts::withRollback(store, label, mutate, run)`。「先本地生效再落盘」的写路径失败时没有快照来纠正（`gui.*` 干脆不发域事件），不回滚就是界面与盘**永久分叉**。回滚前判「这期间是否又被改过」用**对象身份**比就够（store 是不可变更新），不必深比较。纯 UI 的展开态刻意不接（失败无实质后果，回滚反而与用户连点打架）。
- **`onMount` 里注册的东西要在 cleanup 里全释放**：`return addBackInterceptor(...)` 看着像配对好了，其实只释放了拦截器，同一个 `onMount` 里 `addEventListener` 的一个都没摘——泄漏的 capture 阶段 keydown 会把全应用的 Escape 吃掉。
- **单测钉的必须是「实际行为」而不是「我以为的行为」**：涉及第三方渲染（marked）的断言先跑一遍真实输出再写。v0.8.3 之前有一条 spec 把「有序列表不产任务框」钉成了契约，而 marked 15 明明会产——161 项全绿也照样有 bug。
- **同类修复要做类级扫描**，别修用户报的那一处就收工：`closeOverlays` 漏了第三处、`return addBackInterceptor` 有两处、色盘「活值 vs 落盘」有四处，都是一处修好别处照旧。

### 3.13 六条新的（v0.8.4）

- **Svelte 5 legacy 下，别指望「`$:` 里调用的函数写状态」会重新调度**：`$: if (cond) void openTool(x)` 里 `openTool` 第一行写组件状态，**不会**让别的 `$:` 或模板重跑（`legacy_pre_effect` 把 `active_effect` 指向父分支 + `untrack`）。症状是「路由/条件到了、界面不动」。修法不是换写法，而是**去掉那份影子状态**——让唯一真源（store）直接派生渲染（`ToolboxView` / `toolRoute` 就是这么改的）。
- **收缩包裹（shrink-to-fit）容器 + 内容加粗 = 尺寸漂移**：外层 `width: 100%` 或 max-content 时，日历里「选中日加粗」会改变列宽、点哪天面板尺寸都不一样。修法是**给内容定宽**（`.date-picker-grid` 228px）或给容器定宽，别指望内容不变。
- **子组件监听滚动容器要跟着 prop 挂/摘**：父组件 `bind:this` 的赋值可能晚于子组件 `onMount`，写成 `onMount(() => scroller.addEventListener(...))` 就永远没挂上（症状：能滚动、窗口不动）。
- **`to_z32()` 出来的 id 不能 `FromStr` 回来**（iroh 只认 RFC4648 base32 与 hex）：跨层传递要么带可拨号地址、要么在核心侧查表。
- **预览型 UI 必须自带「作废」路径**：预览值住在一个按 scope 索引的 store（`colorPreview.ts`），菜单关闭即作废——否则会串页（日记页改色染到工作区）或悬空（界面停在没保存的颜色）。
- **列表渲染只优化「挂多少」**：`window.__kxtodoRenderStats` 与 `perf-bench.mjs` 是量它的地方；**数据层一行都别动**（store 里永远全量），动的只有 `VirtualStack` 的窗口。

### 3.14 七条新的（v0.8.5）

- **新增全局 CSS 选择器一律带模块前缀，禁止裸名词类名**——v0.8.5 一版抓出两起：裸 `.transfer` 撞上记账转账行（所有转账条目被竖排）、`.toolbox-sub-title`/`.toolbox-sub-actions` 撞上随机数/人民币工具的同名类（按钮被顶到「数量」行上面）。**改完必须把 `git diff <上版> -- src/styles` 里新增的 `.类名` 逐个 `grep -rl` 回 `src/`，确认只有一个模块在用**。
- **壳上有 `transform: scale(uiScale)`，量尺寸必须分清两套坐标**：`getBoundingClientRect` 是**缩放后的视觉像素**，`scrollTop`/`offsetHeight`/占位高度是**布局像素**。`VirtualStack.measure` 早先用 rect 量高，0.75 缩放下每行欠 25%、越靠后累积误差越大（跳转到某天差好几张卡）。**测量与记账用 `offset*`，命中与绘制用 rect**。
- **`iroh::Endpoint::close()` 对任何 clone 调用都关整个端点**（没有引用计数保活）：复用在线会话端点的传输会话**绝不能持有端点**——取消与收尾关的都是**会话自己的 `Connection`**（v0.8.4 的症状：发完一次整个助手下线）。
- **递归组件里跨层共享的交互状态必须住模块级 store**：`ListTree` 每层一个实例，拖动逻辑跑在按下指针那层、目标行常属于另一层——状态留在实例里时跨层拖动的反馈永远画不出来（功能对、反馈丢）。
- **「渲染为纯函数」的唯一例外要三件齐**（渲染态勾选的手术式更新）：mark/unmark 渲染与点击共用一份 + 卡片重渲按**文本完全一致**豁免（`skipNextRender`）+ 渲染器账本 `adopt`。缺 `adopt` 就会在**文本回退**（写失败回滚/远端同步）时被判「已经渲过」而跳过，界面永久分叉；`{@html}` 只与上一次的值比，同值不重建，回退要靠「先清空再写回」强制重建（同一任务内，不闪）。另外：**勾选框的点击路径不要 `preventDefault`**（取消激活行为会把原生翻转还原），目标态**从源码推**（`markdownTaskChecked`）而不是拿 DOM 取反。
- **预设色块与取色器是两种语义**：预设是离散选择、**单击即落盘**；取色器才有拖动过程，走「草稿预览 + 保存/取消」。
- **传输助手的生命周期三规则**：有活跃任务不断连；已配对（在线）退出工具页保持在线、手动「离线」才下线（并清掉记住的口令）；未配对退出即清理界面状态。状态与事件订阅都在 `transferStore.ts`，工具页只是视图。

### 3.15 七条新的（v0.8.6）

- **搜索的折叠索引按对象身份缓存，但任务折叠串里不许进条目名**：日记/记账的折叠串随对象走（逐条不可变更新 ✓）；任务的归属条目改名只动 `state.nodes`、任务对象身份不变，缓进来的名字会**永久过期**——名字命中走 `matchingNodeIds` 现算。另：记账的**金额串与文字串必须分开缓存**（金额带千分位逗号，混进去会让「搜一个逗号」列出所有四位数的账）。
- **拖动落点判定的两条护栏不能少**（都在 `dragHit.ts`，有单测）：① `contiguousZones` 把行间 2px 缝隙按中点判给邻近行——严格按 span 判会让指针掉进缝里被当「空白区」，落点被清、整棵树跳；② `keepsPreviousDecision`（行动了、指针没动 = 布局在动）= 保持现判——我们的预览让位会把目标行整体挪走，用新几何重算会把「瞄准分组头中部」判成「插到后面」，行来回闪。**判据一律用布局位置**（`settledTop` 把 flip 的 translate 减掉，还要乘壳缩放比），绝不用动画中途的 rect。
- **子树的展开动画用 `grid-template-rows: 0fr → 1fr`，收起态保持挂载**：`{#if}` 挂载会让新内容瞬间占位、兄弟行再慢慢 flip（「先盖住再挪走」）。代价是收起态的行仍在树上——**落点判定必须用 `closest(".tree-children:not(.open)")` 排除它们**，否则命中看不见的行。
- **移动端整页视图用不透明覆盖，不用 `display:none`**（记账/日记/工具箱三组）：`display:none` 让返回主界面整块重排+重绘（闪）；改覆盖后底下两栏 `visibility: hidden` + `inert`（旧 WebKit 不认 inert 时 CSS 兜底），返回零重排、滚动位置也留着。`.view-list`/`.view-content`/`.view-settings` 三组保持不变（它们不是整页覆盖层）。
- **点锚定的浮层几何只有一份（`popover.ts::placePopover`）**：优先向下、放不下翻到锚点上方且**下边缘对齐**、**四边一律钳制**（宁可限高滚动也不溢出）。量尺寸要等**双 rAF**（首帧内容还没定宽，钳制会算错），落位前先 `visibility: hidden` 免得在错位置闪一帧。
- **传输清单的 `rel` 恒为相对路径**：所有取文件入口都要走 `transferManifest.ts` 的四个构造器（`filePick`/`spoolPick`/`folderPick`/`textPick`），`startSend` 按 `root` 分组拆成多次发送。core 侧 `TRANSFER_MANIFEST_ABSOLUTE_PATH` 只是兜底 tripwire——绝对路径进协议在 Windows 上会被接收端整单拒、在 Linux 上会**镜像成一棵目录树**。
- **「拒绝」是用户决定，不是错误**：core 拒绝时记 `rejected` 历史、发信息性事件、**不发 error**（发 error 会让界面在从没建过卡的会话上凭空画一张失败卡）；错误文案只在 `TRANSFER_CONNECTION_LOST` 时才说「对方离线」，本地错误必须原样透出（抽成纯函数 `transferErrorText`，有单测）。
- **懒加载组件的响应式挂载判据不能是「实例是否为真」**：`await import()` 在途时实例还是 null，重跑就再挂一次，而每次 `await tick()` 都是微任务、永远不给 fetch 让路——整页主线程被这个循环饿死（实测点一下色块页面直接卡死）。用「当前请求 key」记账（`ColorPickerPanel` 的 `activeKey`）。
- **挂在 App 层（宿主之外）的浮层要自己 `stopPropagation`**，否则点击会冒到 `.app-shell` 的 `closeOverlays`，把宿主菜单关掉（`ColorPickerPanel` 踩过）；**Esc 要注册在 `window` 捕获**——菜单的 Escape 处理器也在 window 捕获且会 `stopPropagation`，同级 stopPropagation 管不住，挂在 `document` 上会被它拦住。

### 3.12 还有一大批（去 invariants.md 查）

数据与写路径 / 同步 / 前端 Svelte 与渲染 / 记账 / 图片与导入导出与清理 / 构建发布 CI / 平台与窗口——七组共 100+ 条硬约束速查（每条原文照引 + 出处），全在 **`references/invariants.md` 第九节**。

## 4. 路由表：要做什么 → 动哪里

（**全篇最有价值的一张表。**前两列是原 AGENTS.md 的原文，一字未改；第三列「详见」是本 skill 新加的指路。）

| 要做什么 | 动哪里 | 详见 |
|---|---|---|
| 改全局搜索（卡顿 / 命中太多 / 想加字段） | `src/lib/searchScan.ts`（分块扫描 + 封顶 200 + rAF/idle 调度，**改完跑 `perf-bench` 的宽词搜索三条**）+ 三处折叠索引 `diary.ts::diaryFold` / `ledger.ts::ledgerFold` / `nodes.ts::taskFold`（缓存口径见 3.15）+ 两个结果视图 `Workspace` 搜索分支与 `SearchResults.svelte`（都走 `VirtualStack`）；**任务折叠串里不许进条目名** | 本文件 3.15 + `references/frontend.md` + `references/history/v0.8.6.md` 一 |
| 加/改 CLI 命令 | `crates/core/src/cli.rs`（clap 树）+ 对应 `ops_*.rs`；`schema.rs`/`skills.rs` 自动跟随 | `references/cli.md` |
| 扩 CLI 的 `--jq` 子集 | `crates/core/src/jq.rs`（`SUPPORT_SUMMARY` 错误 hint 与 `JQ_SUBSET_DOC` 要同步改，有测试钉住两边） | `references/cli.md` |
| 改列表命令的分页 / 合计 | `ops_task.rs` 的 `Page`/`paginate` + `core.rs` 的 `page_from`/`unbounded_page_from` + `render.rs` 的合计行；`ledger list`/`diary list` 默认返回全部，**金融数据不许静默截断** | `references/cli.md` |
| 加 GUI 写操作 | `crates/core/src/ops_gui.rs` 加命令 → `actions.ts` 加 coreDispatch 包装 → 组件调用 actions；**纯 UI 命令要进 `repo.rs::UI_ONLY_COMMANDS` 白名单**（不进审计台账） | `references/frontend.md`（actions.ts）+ `references/invariants.md` |
| 改渲染性能 / 展开时机 / 测量 / 记忆化 | `src/lib/markdown.ts`（block/inline LRU + `window.__kxtodoRenderStats`）+ `src/lib/deferredMarkdown.ts`（**展开时双 rAF 后再算**）+ `src/lib/measureBus.ts`（全应用共享 RO/resize/rAF 的 `observeResize`）；卡片 `fullHtml` **只在展开时渲染**（`$:` 是急切求值） | `references/frontend.md` + `references/history/v0.8.md` 批次 2 + `references/history/v0.8.1.md` 一.3 |
| 写前端纯逻辑单测 | `src/lib/__tests__/*.spec.ts` + 独立 `vitest.config.ts`（node 环境）；跑 `npm run test:unit`；**断言必须时区无关**。纯逻辑要**单独成模块**（不 import marked/DOMPurify）才跑得进 node——`markdownTasks.ts` / `rmb.ts` / `dueHighlight.ts` 都是这么拆的 | 本文件 3.9 + `references/frontend.md` |
| 加设置项 | `model.rs` SettingsFile + `defaults.ts` 默认值/normalize + `SettingsDrawer.svelte` UI | `references/frontend.md` + `references/ui-patterns.md`（设置抽屉）+ `references/sync.md`（若该项要同步） |
| 加调度触发/动作类型 | `model.rs`（discriminator 分支）+ `ops_schedule.rs` 白名单校验 + `plan.rs`/`scheduler.rs` 执行 + `scheduleAdapter.ts` 适配 + `ScheduledTasksView.svelte` 编辑表单 | `references/ui-patterns.md`（定时任务）+ `references/frontend.md`（scheduleAdapter.ts） |
| 改记账 | 数据与命令：`model.rs`（LedgerFile/LedgerEntry/LedgerAccount/LedgerCategory/LedgerSettings + `seed_defaults` 确定性种子）+ `repo.rs`（Domain::Ledger / load_ledger（缺文件内存种子）/ write_ledger（首写落种子）/ ensure_initialized）+ `ops_ledger.rs`（ledger.add/get/list/modify/remove/transfer/accounts/accountAdd…/categories/categoryAdd…/stats/balance/export/import）+ `ledger_archive.rs`（xlsx 四表 zip 打包与解析）+ `ops_config.rs` 的 `ledger.*` 五个路径 + `cli.rs` 的 Ledger 子命令树（kebab 名）+ `schema.rs` risk_for + `src-tauri/src/lib.rs` 的 `ledger_export_zip`/`ledger_import_zip`（**两个 invoke_handler 都要注册**）；同步：`merge.rs`（Scopes 五 bool / ledger 三种 kind 的 stamp·payload·apply·normalize / settings 共享子集 ledger 块）+ `engine.rs` 五处 + `ops_sync.rs` 与 `cli.rs` 的 `--sync-diary/--sync-ledger`；前端：`ledger.ts`（按天/热力/统计/余额纯逻辑）+ `ledgerIcons.ts`（lucide 白名单与账户类型默认图标）→ `stores.ledgerData` → `LedgerView.svelte`（四视图 + 齿轮 + 段控 + FAB）→ `ledger/LedgerRow|LedgerList|LedgerCalendar|LedgerStats|LedgerAssets|LedgerEditor|LedgerEntryMenu|CategoryManager|AccountManager.svelte` → `actions.ts` 的 ledger 包装 → `ledger.css` + mobile.css 的 `.view-ledger` | `references/ledger.md` + `references/history/v0.7.0-v0.7.4.md` + `references/history/v0.7.5-v0.7.8.md` |
| 加一类**要同步的**实体 | 日记（kind `diary`）是现成样板：`model.rs` 新领域文件结构 + 自己的 SCHEMA_VERSION → `repo.rs`（Domain 变体 + layout 路径 + load_/write_ + `ensure_initialized` 里补一条）→ `merge.rs`（payload 剥本机 UI 态 / extract / `*_entity_stamp` / `apply_*_record` **连删除分支一起** / `normalize_*_orders`）→ `engine.rs` **五处**（拉取分桶、合并事务、对账水位 match、全新设备推送抑制、`resolve_conflict`）→ `host.rs` 的 `emit_domain_event` match（新 Domain 变体不补会直接编不过）→ `core.rs` 与 `cli.rs` 的 schemaVersions → `lib.rs` 的 `core_snapshot`（**`wanted()` 名单与 `put_snapshot_domain` 调用都要加**，它按域过滤）→ 前端 `CoreSnapshot`/`applySnapshot`/`normalize*`/`commit*`。**server 一行都不用改**（entities 表没有 kind 列，kind 只在密文里）。搭现有 scope 的车（日记跟「同步数据」）就不用动 `Scopes`/scopeSignature/三勾选框/CLI 范围参数 | `references/sync.md` + `references/architecture.md` |
| 改外观 | 全局 CSS 文件按区域找；配色变量在 base.css；菜单样式统一在 menu.css | `references/frontend.md`（CSS 全部）+ `references/invariants.md`（CSS 铁律） |
| 改超链接增强（标题 / 预览卡片） | 抓取与解析在 `crates/core/src/linkmeta.rs`（命令 `gui.link-meta`，缓存 `runtime/linkmeta.json`——**改了元数据字段就把 `CACHE_VERSION` 抬一格**，否则老缓存命中不到新字段）；渲染在 `src/lib/linkPreview.ts`（由 `markdownControls.ts::markdownWire` 驱动；**增强是可逆的**——`revertUnwanted` + `cardOriginalAnchor`/`titleOriginalText` 两张 WeakMap，改这块别把退路弄断，否则「设置拨了档、画面不动」，因为 markdown 有记忆化根本不会重渲）+ `markdown-ext.css` 的 `.kx-link-card`；设置项 `features.linkRender`（**三档单选** `off|title|card`，默认 card；设置页三个 radio）走 model.rs/ops_config/defaults.ts/types.ts/SettingsDrawer | `references/ui-patterns.md`（markdown 扩展渲染）+ `references/history/v0.7.5-v0.7.8.md`（v0.7.7 ⑧ / v0.7.8 ⑦） |
| 加浮层 / 弹层 / 全屏查看 | `platform.ts::createBackGuard`（**挂载式必须 `onDestroy(dispose)`**，见 3.8）+ 安全区避让 + **点锚定的几何走 `popover.ts::placePopover`**（优先向下/放不下翻上/四边钳制；量尺寸等双 rAF）；卡片级「点别处关闭」的浮层挂进 `cardOverlays.ts` 的那**一份** document 监听 | 本文件 3.8 + `references/ui-patterns.md` |
| 改日历 / 周起始 / 日期选择器 | `stores.ts` 的 `weekStart` 派生 store（**唯一来源**）+ `diary.ts::leadingBlanks`/`calendarWeekdayHeaders` + `ledger.ts::ledgerCalendarCells`/`weekStartOf`；设置项 `features.weekStart` | 本文件 3.10 + `references/frontend.md` |
| 改临期高亮 / 任务日期展示 | 纯逻辑 `src/lib/dueHighlight.ts`（**四档**：已过期/今天/明天/后天，有单测）+ `currentTime.ts::currentMinute`（驱动「刚过期」翻转）+ `TaskCard.svelte` 的 `due-soon` 类 + `workspace.css`；配色按页存在 `appearance.dueColors[nodeId]`（**四个色**，改档数要同步动 core 的 `expect_due_colors` 与 `normalizeDueColors`，见 3.11），入口在列表三点菜单「临期高亮色」 | `references/frontend.md` + `references/history/v0.8.3.md` 三 |
| 改任务提醒 / 「日期与提醒」面板 | 模型 `model.rs::{Reminder, Item.reminders}`；运行时 `crates/core/src/reminders.rs`（解析 `due-60`/`+1h`/RFC3339、校验、`beforeDue` 折算、时钟跳变、`Engine::poll` + 台账 `runtime/reminders.json`——**台账绝不进同步载荷**）；跑在 `scheduler.rs::run` 的 500ms 循环里（`has_reminder_work` 让看门狗不退出）；通知在移动端经 `MobileBackend::show_notification` → `kxtodo://notification` 交回前端；CLI `task add/modify --reminder`；面板 `src/lib/TaskDateReminderPanel.svelte`（**三个入口共用**：`Workspace` 右键菜单 / `TaskCard` 日期浮层 / `MarkdownEditorModal` 工具栏）+ 抽出来的共用日历 `CalendarGrid.svelte`；前端纯逻辑 `src/lib/reminders.ts` | 本文件 3.12 + `references/history/v0.8.3.md` 一 |
| 改文件传输助手 | **状态与生命周期全在 `src/lib/transferStore.ts`（模块级单例）**：工具页 `src/lib/tools/TransferTool.svelte` 只是它的视图（卡片分区：身份 / 收发 / 传输中 / 已完成 / 历史）；`ensureTransferRuntime()`（App 启动调）订阅 `kxtodo://transfer` + 对账 `transfer_status` + 恢复记住的口令自动上线 + wake lock 的 visibilitychange；三条生命周期规则见 3.14。core 侧 `crates/core/src/transfer.rs`（口令 → pkarr 房间密钥与握手令牌、**常驻在线会话** `go_online`/`go_offline`/`devices`/`decide`（**pending 是 HashMap 多槽**）、设备名 `_name` 记录（**名字有 20 秒缓存**）、文本消息、`runtime/transfer-{identity,code,history}.json`（历史读改写**有锁**）、4 字节长度前缀的 JSON 帧、`safe_join` 守路径、同名自动重命名（**1000 个候选用尽报 `TRANSFER_NAME_EXHAUSTED`，绝不覆盖**）、大文件走 `tokio::fs`/`spawn_blocking`）+ `src-tauri/src/lib.rs` 的命令（**两个 invoke_handler 都要注册**；移动端发送经 `runtime/transfer-outbox`、接收目录两端同一份实现 = `download_dir()` 下的 `kxtodo-transfer`）；清单的 `rel` 语义唯一起点是 `src/lib/transferManifest.ts` 的四个构造器（绝对路径会 tripwire）；拒绝记 `rejected` 且**不发 error**；relay 设置 `transfer.relay`（空 = 跟 `sync.p2pRelay`、`disabled` = 禁用、其它 = 自部署地址；**改了要重新上线才生效**，入口在工具页 ⋯ 菜单的「relay 服务」）；**发送会话绝不持有在线端点**（Endpoint::close 关的是整个端点，见 3.14）；**ALPN `kxtodo-transfer/1` 必须与同步的 `kxtodo-p2p/1` 不同**；**z32 的 id 不要拿去 parse**（在房间条目里按 `to_z32()` 比对） | `references/sync.md`（iroh/pkarr 那一套）+ `references/history/v0.8.5.md` 一.2/二.4 |
| 改渲染态任务勾选（markdown 里的 `- [ ]`） | `src/lib/markdown.ts` 的 `markCheckedItem`/`unmarkCheckedItem`（渲染与点击共用一份）+ `setRenderedTaskBox`（目标态从源码推）+ `src/lib/markdownTasks.ts` 的 `toggleMarkdownTask`/`markdownTaskChecked` + 两张卡片的 `skipNextRender` 豁免与 `fullRender.adopt`（**三件齐，缺一会在文本回退时永久分叉**）；单测 `src/lib/__tests__/taskToggle.spec.ts`（happy-dom，等价性）+ `v084-fixes-test.mjs` 第 33 节（不重渲/回退纠正） | 本文件 3.14 + `references/history/v0.8.5.md` 四 |
| 改侧栏拖动排序（固定区 / 分组树） | 固定区：`Sidebar.svelte::navDropIndex`（**按布局两种算法**：单列比 Y；双列/图标先按 Y 找竖带、带内比 X）+ `navDragOrder` 实时让位 + `animate:flip`。分组树：`nodes.ts::planTreeMove`/`planTreeRootEnd`（**预览与提交同一份 planner**）+ `dragHit.ts`（落点的两条护栏：`contiguousZones` 的缝隙归属与 `keepsPreviousDecision` 的「布局在动不改判」，**判据一律用 `settledTop` 的布局位置**）+ `listTreeDrag.ts` 的共享落点 store（**跨实例反馈，别搬回组件局部**）+ `ListTree.svelte` 的 `.tree-item` 包裹层（`animate:` 要求 each 唯一子元素）；指针落在被拖行自己身上时**保持现落点** | `references/ui-patterns.md` + `references/history/v0.8.5.md` 二.11/29 |
| 改长列表渲染（卡顿 / 首屏 / 内存） | `src/lib/windowing.ts`（纯逻辑：前缀和 / 落点二分 / 窗口范围 / 滚动锚点 + 单测）+ `src/lib/VirtualStack.svelte`（legacy 槽位 `slot="item" let:row`；`fullBelow` 阈值内全量直出；滚动容器跟着 prop 挂监听；**`measure` 用 `offsetHeight`（布局像素）——壳有 `transform: scale()`，rect 是视觉像素，混用会累积误差**；`scrollToIndex` 双 rAF 二次对位；`.virtual-item` 是 `display: flow-root` 让子卡片外边距算进占位）+ `measureBus.observeResize` 跟踪高度；消费端 `DiaryView` 的 `listRows` / `Workspace` 的 `taskRows` / 日记分组视图的 `expanded`。**virtua 用不了**（runes `children` snippet 与 legacy `let:` 不兼容，依赖已删）；**store 里永远全量**，只调窗口 | `references/frontend.md` + `references/history/v0.8.4.md` 二 + `references/history/v0.8.5.md` 一.3 |
| 改取色（主题色 / 背景色 / 临期色） | **12 处取色入口都是 `ColorPickerPanel.svelte` + `colorPickerPanel.ts` 的单例请求**（内嵌 iro，懒加载；RGB/HEX 双向联动 + 拖动按 rAF 合帧）；`src/lib/colorPreview.ts`（草稿预览 store + 三个纯选择器）+ `src/lib/ColorDraftActions.svelte`（取消/保存行）+ 消费端 `Workspace`/`DiaryView`/`LedgerView`/`ToolboxView` 的 `mainStyle` 与 `TaskCard` 的临期配色；工具页外观在 `settings.toolbox.*`（core `ToolboxSettings`，进同步共享子集）；**预设色块单击即落盘；取色器走「草稿 → 预览 → 保存」，菜单关闭 = 作废**；`saveDueColors` 只写变更档、保存后草稿保留 | `references/frontend.md` + `references/history/v0.8.5.md` 二（#5） |
| 加工具箱工具 / 把工具固定进侧栏 | **目录与注册表分开**：`src/lib/tools/catalog.ts`（id/名称/描述，侧栏也要认）与 `registry.ts`（图标 + 懒加载组件）——让 nav 直接 import registry 会把图标与所有工具 chunk 拉进首屏链；固定行 `tools/navigation.ts` + `appearance.navItems` 里的 `tool:<id>`（core 侧 `NAV_TOOL_IDS` 白名单校验）+ `nav.ts` 的 id 类型；页面 `ToolboxView.svelte`（右键固定 / 拖动排序） | `references/ui-patterns.md`（工具箱）+ `references/history/v0.8.3.md` 三 |
| 改 markdown 行首空白 / 任务项映射 | `src/lib/markdownIndent.ts`（缩进换成不间断空格，硬换行补在**上一行行尾**——插在本行行首会让 marked 提前收口列表）+ `src/lib/markdownTasks.ts`（**有序列表同样产任务框**；`listOpen` 决定 ≥4 空格是子列表还是代码块）。两者都是纯模块、都有 node 单测，改前先对着 marked 的真实输出跑一遍 | `references/frontend.md` + `references/history/v0.8.3.md` 四 |
| 改标签（配色 / 预置） | 配色 `src/lib/tagColors.ts` + `workspace.css` 的 `.task-tag.tag-*`；面板 `TagMenuPanel.svelte`（**右键菜单与两个编辑器共用同一个组件**，`domain="task"\|"diary"` 决定读写哪套预置）/ `TagColorPicker.svelte`；`TagColor` 十值 + `Tag.hex`（Rust `model::TagColor`/`tag_hex` 同口径，有 `frontend_contract.rs` 钉住）；预置住在 `appearance.tagPresets` 与 `appearance.diaryTagPresets`（都进同步共享子集） | `references/ledger.md` + `references/frontend.md` |
| 加原生能力 | Tauri 命令/插件，桌面专有逻辑必须 `#[cfg(desktop)]` 隔离并在移动端给空实现（前端 invoke 不能炸） | `references/invariants.md`（跨平台铁律）+ `references/pitfalls-android.md` + `references/frontend.md`（capabilities.ts） |
| 改图片入库（压缩 / 体积闸 / 文件名安全） | `src-tauri/src/lib.rs` 的 `ImageGate`（**三档**：插图 5MB / 背景 2MB 且压完超 10MB 拒收 / 头像一律 256px；只处理 JPEG·PNG，GIF·WebP 可能是动图一律原样；`spawn_blocking`）+ `safe_image_name`（委托 core `diary_archive.rs::is_safe_image_name`——全项目唯一一份实现）；前端上传前的第一道压缩在 `src/lib/images.ts`（`compressAvatarImage` / `compressBackgroundImage`，`BACKGROUND_MAX_EDGE = 2560` 与 Rust 同值） | 本文件 3.1 + `references/history/v0.8.md` 批次 6 + `references/history/v0.8.1.md` 二 |
| 改同步协议/加密 | `crates/core/src/sync/`（crypto=密钥派生+加解密、merge=LWW 纯函数、**transport=HTTP 客户端（三方式共用）**、**endpoint=「连哪儿」（加新通信方式只改这里 + model 的 SyncMode）**、engine=编排、images=图片 blob 通道、discovery=局域网发现客户端、state=runtime/sync.json + sync-host.json、**credentials=明文凭据留档（runtime/sync-credentials.json，配对成功时写，解除配对不清）**）+ `crates/server/src/`（api/db 两侧同步改，discovery=UDP 应答、daemon=后台运行、**host=可嵌入的 serve()/ServerHandle**）；改信封结构要同步动 `merge.rs` 的 SyncEnvelope 与测试，改图片元数据要同步动 `images.rs` 与 `db.rs`/`api.rs`，**改「连哪儿」不许动 merge/crypto**（分层的全部意义） | `references/sync.md` |
| 部署/运维 kxtodo-server | 单二进制 `kxtodo-server --name 家里的服务器 [--listen 0.0.0.0:52177 --db 路径 --data-dir 目录]`；`--daemon` 后台静默运行 + `--stop` 结束；`--update` 自升级（下载失败自动回退 ghfast.top 代理）；升级密钥/盐算法前想清楚——改了派生参数所有设备全部失配 | `references/sync.md`（server 运维 / 管理控制台）+ `references/build-and-release.md` |
| Agent 技能文档 | 只编辑 `skills/kxtodo/SKILL.md`（编译期 include_str! 嵌入，发布 exe 自包含）；**改完必须重跑 `kxtodo-cli skills validate`**（本文件 3.9）。`skills persist` 不指定位置时默认写 `~/.agents/skills/kxtodo/SKILL.md`（v0.6.11）；已存在的 SKILL.md 直接覆盖，路径被同名文件/目录挡住时未加 `--yes` 报 confirmation（退出码 10）询问 y/N；结果里的 `data.path` 就是最终落地路径 | `references/cli.md`（Agent 技能文档） |

## 5. references 索引：何时读哪个文件

| 文件 | 里面是什么 | 什么时候读 |
|---|---|---|
| `references/architecture.md` | 进程拓扑（GUI 常驻 Host / CLI 经 IPC / Android 同栈 / Linux 同拓扑与 core 内 unix 差异）、五个领域文件与数据地基、数据目录解析、同步分层图、**路径约定** | 第一次接触本项目；要理解「谁在跑、数据落在哪」；改 repo / IPC / 移动端宿主；加一类领域文件 |
| `references/invariants.md` | **全部硬约束与铁律 + 每条的「为什么」**（写路径、跨平台三条、不做兼容、版本号、commit-msg hook、发布四条、CSS、浮层与安全区）+ 分散在各处的 100+ 条速查 | **改任何东西之前都该扫一眼**；拿不准某条约束为什么存在；评审自己的改动 |
| `references/frontend.md` | 前端 `src/` 分层（stores / actions / backend / capabilities / platform / longpress / scheduleAdapter / syncRunner / 纯逻辑 / measureBus / 组件 / editor / diary / menu）+ **前端单测（vitest，v0.8.0）** + 全局 CSS 的级联顺序、**层叠坑**、按钮样式规范 | 加/改前端写操作、加设置项、加平台能力、写或改任何 CSS、浮层被盖住/位置错乱/被 overflow 裁掉、要新建按钮、写前端纯逻辑单测、改渲染性能与测量 |
| `references/ui-patterns.md` | 「UI 布局与特性」全章：布局、任务卡片、列表分区与排序、树与图标选择器、⋯ 列表菜单、定时任务、工具箱、我的一天、日记（含 Markdown 压缩包导入导出）、全局搜索混排、markdown 扩展渲染、输入框加号与编辑器元数据行、设置抽屉、Linux 桌面、移动端（Android）、编辑器工具栏、一般卡片压缩包、首帧缩放、返回键拦截器、幽灵点击与浮层层级、安卓退出生命周期 | 改任何 Svelte 组件、改界面行为或手势、加/改右键与三点菜单、改设置抽屉、改移动端交互、改 markdown 渲染扩展 |
| `references/ledger.md` | 记账域专项：数据模型与不变式（整数分 / 余额推导 / 确定性种子 id / 两级分类）、四个视图、以「天」为组织单位、浮层与编辑器语义、齿轮面板、图标目录、**Excel 归档**、**CLI 确认门**、kebab/camel 映射、ledger.css 约定 | 改记账（core 的 `ops_ledger.rs` / `model.rs` LedgerFile 家族 / `ledger_archive.rs` / `ledger_icons.rs`，前端 `ledger.ts` / `LedgerView.svelte` / `ledger/` / `ledger.css`）；排查金额、余额、统计口径 |
| `references/sync.md` | 同步专项：安全模型（Etebase 式）、同步语义（LWW + 墓碑 + OCC）、实体与范围（五个勾选框）、数据地基、配对流程、**踩坑记录 ①–⑦**、server 运维、自动同步（含 v0.4.1 死代码的教训）、局域网发现、图片 blob 通道、掉线不阻塞 UI、账户模型、暂停/恢复、配对历史、设置面板同步卡片、管理控制台、传输分层、内置主机、主机身份是名字、instance epoch、端口生命周期、**P2P**、同步功能总开关 | 改 `core/src/sync/` 或 `crates/server/`；改设置页「数据同步」；排查「同步成功但数据没动」/ 409 一路重试 / 换主机拉不到东西；加一类要同步的实体；部署 kxtodo-server |
| `references/cli.md` | CLI 约定与命令面：加/改命令要动哪里、core camel ↔ CLI kebab、确认门与退出码（3/4/10）、有常驻 Host 时全部经 IPC、`command_needs_data` 白名单、`--jq` 与 `schema`、**列表分页与合计（--cursor/--all/meta.count，v0.8.0）**、Windows 编码坑、**Agent 技能文档 `skills/kxtodo/SKILL.md` 的维护规则** | 加/改任何 CLI 命令或参数；给动作加确认门；排查 CLI 退出码；CLI 中文参数乱码；扩 `--jq` 子集；要更新产品自带的 Agent 技能文档 |
| `references/build-and-release.md` | 构建 / 测试 / 全部回归脚本命令、版本号解析（含两个真实 bug）、commit-msg hook、**七个固定名产物**、kxtodo-server 双平台发布、**应用内更新的多通道测速选路**、GitHub Actions（ci.yml / release.yml / Android 签名）、**发布工作流铁律四条** | 要跑测试、要出包、要打 tag、要推送远程、要改版本号解析或 CI、构建失败、部署或自升级 kxtodo-server |
| `references/pitfalls-windows.md` | Windows 环境坑位 8 条（Git Bash 里 cargo 报 `link: extra operand`、`taskkill` 被转 UNC、**单实例标识撞车**、window-state 插件与窗口几何竞态、**pwsh 5.1 编码两个方向**、Android 交叉检查的 NDK clang 环境、Node 24 libuv flake 与 `\| tail` 掩退出码）+ **computer-use 调试 WebView2 应用的 8 条经验** | 在 Windows 上跑 cargo / npm / release 脚本报错；「改了代码没生效」；窗口尺寸位置异常；脚本中文乱码；要用截图+坐标验证桌面 UI |
| `references/pitfalls-linux.md` | Linux 坑位 6 条（apt 依赖清单与 release.sh 门控含 libxdo 例外、**裸 cargo 构建出 dev 模式制品导致整窗白屏**、托盘依赖 appindicator 宿主、AppImage 需要 FUSE、cargo 直接可用、WSLg XWayland 丢光标与 AppImage 强制 x11） | 在 Linux/WSL 上构建或运行；Linux 制品白屏；托盘不出现；AppImage 跑不起来；光标消失；改 `release.sh` 的依赖门控 |
| `references/pitfalls-android.md` | Android 20 条（gen/android 的所有权、返回键、Kotlin 桥、dialog 的 content:// URI、触摸长按语义、**坐标与 uiScale**、**模块循环 TDZ 白屏**、用 Playwright 模拟移动端、签名与升级、APK 产物策略、图标同步、通知、能力门控优先于 isMobile、**`isMobile` 是 writable store**、夹行三件套、ContextMenu 限高、**transform 缩放影响一切 rect**、首屏量尺寸全是 0、软键盘两连击、菜单限高不能顶到视口顶部） | 构建 APK；改 `src-tauri/gen/android/`；写 Kotlin 桥；改移动端手势/浮层/菜单/测量逻辑；没有真机要验证移动端 UX；签名或升级链出问题 |
| `references/history/README.md` | history 目录的定位与用法 | 想知道「这个目录是什么、该往哪写」 |
| `references/history/v0.4.md`<br>`v0.5.md`<br>`v0.6.md`<br>`v0.7.0-v0.7.4.md`<br>`v0.7.5-v0.7.8.md`<br>`v0.8.md`<br>`v0.8.1.md`<br>`v0.8.2.md`<br>`v0.8.3.md`<br>`v0.8.4.md`<br>`v0.8.5.md` | 按版本归档的**改动索引**（原文粗体小标题 → 现在住在哪），v0.7 那两份还带**逐版流水账全文**（v0.7.3 打磨 ①–⑦、v0.7.4 界面改写 ①–⑩、v0.7.5 ①–⑪、v0.7.6 ①–⑬、v0.7.7 ①–⑧、v0.7.8 ①–⑪，合计 43 条）；`v0.8.md` 是 v0.8.0（**正确性 + 性能 + 卫生版，无新功能**）的七个批次全档 + vitest 挖出的 6 个正确性问题 + **明确决定不做的事清单** + perf-bench 实测数字；`v0.8.1.md` 是 v0.8.0 的回归修复（八个可感问题 + 返回键失灵 + 资料同步不回来）与八项新需求，含 CLI review 十条的逐条处理；`v0.8.2.md` 是 v0.8.1 的收尾（**零新需求**：十二条回归 + 顺手挖出的七条 + 编辑器与任务项渲染口径 + 五条新铁律）；`v0.8.3.md` 是**两个新能力**（日期与提醒 / 文件传输助手）+ 一批短需求 + 前两版 review 11 条的逐条采纳与不采纳理由 + 六条教训；`v0.8.4.md` 是**长列表窗口化**（性能专项，含 virtua 选型否掉的经过）+ **传输助手按 LocalSend 重做**（常驻在线会话 / 设备名 / 接收确认 / 文本消息 / 历史）+ 一批交互一致性收口（取色预览与保存、工具箱子页头部、固定区拖动、编辑器字体与有序列表、日期面板尺寸）+ 6 条 bugfix + 七条教训；`v0.8.5.md` 是 **v0.8.4 review 的 32 条逐条修复**（P0 三条：裸类名踩踏 / 发送关共享端点 / 窗口化跨阈值记账失效——真根因是缩放坐标混用；P1/P2/P3 一片 + 追加需求 33 渲染态勾选的手术式更新与它的三件套）+ 七条新铁律 | `v0.8.6.md` 是**极限数据量三件补课**（搜索分块扫描 + 树拖动落点重做 + 移动端整页改覆盖）与**传输全链路整治**（`rel` 归一 / 拒绝语义 / 设备名 republish）+ **取色盘统一成 iro 组件** + 小修（工具页标题 / 年月面板 / 齿轮 toggle / relay 二级菜单）；改搜索/拖动/移动端视图/传输/取色之前必读 | 追溯「这一版为什么这么改」「某个方案试过又被推翻的经过」「某个方案为什么明确不做」；**改记账界面之前必读 v0.7 那两份**（很多当前界面细节只在那里）；改渲染/性能/CLI 分页/图片入库前读 v0.8.md 对应批次；改首帧缓存/两阶段渲染/返回键层级/超链接增强前读 v0.8.2.md；改提醒/传输/日期面板/工具箱固定/markdown 缩进前读 v0.8.3.md；改长列表渲染/传输交互/取色/编辑器列表前读 v0.8.4.md；改动前先读 v0.8.5.md 的「这一版做对的地方」与三条 P0 的根因（裸类名、Endpoint::close、坐标混用） |

**几条最常用的组合**：

- 改记账界面 → `ledger.md` + `history/v0.7.0-v0.7.4.md` + `history/v0.7.5-v0.7.8.md`（+ `ui-patterns.md` 若涉及共用组件）
- 改渲染 / 排查卡顿 / 动测量逻辑 → `frontend.md` + `history/v0.8.md`（批次 2/3 + 实测数字）+ `history/v0.8.1.md` 一（「点一下顿一下」的四个根因；性能基线跑 `node scripts/perf-bench.mjs`）
- 改任务卡片 / 列表 / 树 / 菜单 → `ui-patterns.md` + `frontend.md`（CSS）+ `invariants.md`
- 改同步 / 排查同步 → `sync.md`（+ `architecture.md` 的分层图）
- 发版 / 打 tag / 推送 → `build-and-release.md` + 本文件 3.4–3.6
- Windows 上 cargo 报 `link: extra operand` → `pitfalls-windows.md` 第 1 条（一切 cargo 调用走 `scripts/cargo-msvc.sh`）
- Linux 制品白屏 → `pitfalls-linux.md` 第 2 条（裸 cargo 出的是 dev 模式制品）
- 移动端浮层跑到屏幕外 / 滚不到底 → `invariants.md` 第七、八节 + `history/v0.7.5-v0.7.8.md` 的 v0.7.5 ⑥⑦ 与 v0.7.6 ②

## 6. 自我迭代条款（每轮开发结束时**必须**执行）

**这份 skill 是活文档。每轮开发结束时，必须把本轮新经验写回本 skill**——AGENTS.md 之所以膨胀成 166KB 的流水账，就是因为经验只往里堆、从不重组。写回时按下面的判据分流，**别一律往 SKILL.md 里塞**。

| 本轮产生了什么 | 写到哪里 |
|---|---|
| **新的不变式 / 铁律**（「绝不 / 必须 / 一律 / 别 / 勿」类，或「这样做会坏，因为……」） | ① 若是**跨领域、每次改动都该知道**的 → 写进本文件**第 3 节**（挑对子节：3.1 写路径 / 3.2 跨平台 / 3.3 不做兼容 / 3.4 版本号 / 3.5 hook / 3.6 发布 / 3.7 CSS / 3.8 浮层与安全区 / 3.9 测试与一致性；确实不属于任何子节才新开一条）；② **同时**把「原文照引 + 为什么 + 出处」写进 `references/invariants.md`（跨领域的进第 1–8 节，领域内的进第九节对应分组） |
| **新的平台坑位**（Windows / Linux / Android 上「这么干会炸」的环境级经验） | 对应的 `references/pitfalls-windows.md` / `pitfalls-linux.md` / `pitfalls-android.md`，**按现有编号列表续一条**（写清现象 + 根因 + 正确做法） |
| **新的界面细节与本版改动**（这一版改了什么 UI / 交互 / 视觉，含被推翻的方案） | `references/history/vX.Y.md`（当前正在进行的那个大版本；没有就新建一个，照 `v0.7.5-v0.7.8.md` 的结构：这一版的主题 → 改动索引 → 逐版流水账全文）。**同时**把「当前生效的规则」写进对应主题文件（`ui-patterns.md` / `ledger.md` / `frontend.md`），因为主题文件才是「现在长什么样」的权威 |
| **架构变化**（进程拓扑、领域文件、数据目录、同步分层、路径约定） | `references/architecture.md`；若同步分层动了，`references/sync.md` 一起改 |
| **新的「要做什么 → 动哪里」**（发现某类改动总是漏掉某个文件） | 本文件**第 4 节路由表**（改对应行，或加一行）；同时更新第 5 节索引表里那份文件的「里面是什么」 |
| **构建 / 发布 / CI 的变化**（新脚本、新回归测试、版本号规则、workflow） | `references/build-and-release.md`；若是**每轮都要跑**的命令，也更新本文件第 7 节 |
| **CLI 命令面或确认门的变化** | `references/cli.md`；产品自带的 `skills/kxtodo/SKILL.md` 是**另一件事**（那是给外部 Agent 用的使用手册，按 `cli.md` 的规则单独维护） |

**硬性要求**：

1. **不要往 SKILL.md 里堆版本流水账**——那是 `references/history/` 的职责。SKILL.md 只放「**当前不变式**」与「**指路**」。
2. **SKILL.md 长度控制在 500 行以内**。超了就把细节下沉到 references，本文件只留结论 + 指路。
3. **写「为什么」，不只写「是什么」**。这份 skill 里每一条血泪经验都带着根因（「否则……」「曾导致……」「实测……」），新写的条目照这个格式来——只写规则不写根因，下一个人就会把它改回去。
4. **引用代码位置用「文件路径 + 函数名 / 结构体名 / CSS 选择器名」，不要用行号**（行号会随代码演进失效，函数名不会）。例：`crates/core/src/repo.rs` 的 `Repository::write_data`、`src-tauri/src/lib.rs` 的 `core_snapshot` 命令、`workspace.css` 的 `.task-tag .tag-delete`。
5. **改完顺手校对指路**：新加了 reference 文件或改了小节标题，就更新第 5 节的索引表与各文件顶部的「什么时候读它」。

## 7. 构建 / 测试 / 验证的最短路径

```bash
npm install                                     # 依赖（新克隆跑一次即自动装上 commit-msg hook）
npm run desktop:dev                             # 桌面开发（vite + tauri dev）
scripts/cargo-msvc.sh test -p kxtodo-core       # Rust 测试（Git Bash 下必须用这个包装！）
npm run test:unit                               # 前端纯逻辑单测（vitest：资金路径 / 时刻 / normalize；断言时区无关）
node scripts/perf-bench.mjs                     # 性能基线（300 任务 / 300 日记 / 3000 账目：冷启动 + 页内 rAF 计时的交互
                                                #   + 断言首屏 block 渲染为 0；需先 npm run dev）
npm run build                                   # 前端构建（svelte-check + vite build 同 CI 口径）
```

**Git Bash 下 `cargo` 会报 `link: extra operand`**（uutils-coreutils 的 `link` 遮蔽了 MSVC `link.exe`）——**一切 cargo 调用走 `scripts/cargo-msvc.sh`**。`npm run desktop:dev` 同样中招（`tauri dev` 内部调裸 cargo）：先 `eval "$(grep -E '^(MSVC_|SDK_VER|export )' scripts/cargo-msvc.sh)"` 再跑，或直接在 VS 开发者 shell 里跑。详见 `references/pitfalls-windows.md` 第 1 条。

**出包**：

```bash
.\release.ps1              # 默认 Windows + Android（KXToDo.exe + kxtodo-cli.exe + KXToDo.apk）
.\release.ps1 win / android / unix     # 单平台（unix = 经 WSL 原生克隆构建 AppImage + CLI + server）
.\release.ps1 win,unix     # 逗号组合；all = 三平台（环境未就绪告警跳过，不终止其它）
./release.sh               # Linux 构建入口（须在 Linux/WSL 上跑）
git tag vX.Y.Z; git push origin main vX.Y.Z   # 云端发布：触发 release.yml 构建三平台并发 release
```

**推送前必须先问用户这一版要不要打 tag；推送后必须盯 CI 到结束**（第 3.6 节）。**跑 release/publish 脚本不要接 `| tail` 管道**（会把退出码掩成 0 造成「构建成功」误报），重定向到日志文件再 tail。

**调试前先把所有 kxtodo/KXToDo 进程杀光（含托盘）**：单实例标识 `com.wddjwk.kxtodo` 全局唯一，debug/release/旧版本 exe 互相转发，用户反馈「修复没生效」优先怀疑旧进程残留。杀进程用 `powershell -NoProfile -Command "Stop-Process -Id <pid> -Force"`（Git Bash 里 `taskkill /PID` 会被转成 UNC 路径）。

**界面存疑时先读磁盘**：默认数据目录（Windows `%LOCALAPPDATA%\kxtodo\todo-note-data`）下的 `*.json` 直接可读，先分清是「写错了」还是「画错了」，能省一半时间。

**回归脚本**（都是 playwright-core + 系统 Edge 连 vite dev，需先 `npm run dev`）：`mobile-ux-test` / `diary-ux-test` / `menu-sweep-test`（浮层越界类 bug 的守门员）/ `markdown-ext-test` / `ledger-ux-test` / `v068…v078-fixes-test`（逐版回归）。改了哪一版的东西就跑哪一份，清单与各自覆盖范围在 `references/build-and-release.md`。
