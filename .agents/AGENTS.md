# KXToDo 开发者指南（入口）

> **在本仓库动任何代码之前，先加载项目 skill：`develop-kxtodo`**（`.agents/skills/develop-kxtodo/SKILL.md`）。
> 那份 skill 是本指南的正文：架构精简版 + 全部铁律 + 「要做什么 → 动哪里」路由表 + 11 份专题 reference
> （architecture / invariants / build-and-release / frontend / ui-patterns / ledger / sync / cli /
> pitfalls-windows / pitfalls-linux / pitfalls-android）+ `references/history/` 下按版本归档的流水账。
>
> **文件布局**：真身是 `.agents/AGENTS.md` 与 `.agents/skills/`；`.qoder/AGENTS.md` 与 `.qoder/skills`
> 是指向它们的软链接（仓库里以 mode 120000 存相对路径），仓库根目录只留一个面向用户的 `README.md`。
> 改文档一律改 `.agents/` 下的真身。`core.symlinks=false` 的 Windows 克隆会把软链接落成
> 一个内容为目标路径的文本文件——那份不可用，直接看 `.agents/`。
>
> 本文件刻意只留「skill 万一没被加载也不能出错」的那几条。**不要把细节写回这里**——
> 它每轮对话都会被完整注入，274 行 / 166KB 的旧版本就是这么长出来的（最长单行 6204 字符，
> 20 行超过 1000 字符，占全文 47%），既拖慢每一轮对话又找不到东西。新经验一律写进 skill
> 对应的 reference（判据见 SKILL.md 的「自我迭代」一节）。

## 这是什么

KXToDo（Todo Note）：本地优先的**待办 + 日记 + 记账**三合一应用。Rust + Tauri 2（桌面壳与后端）、
Svelte 4 + TypeScript + Vite（前端）、CodeMirror 6（编辑器）、marked + DOMPurify + highlight.js（渲染）。
同一份 `kxtodo-core` 跑在三端：**Windows**（`KXToDo.exe` + `kxtodo-cli.exe`）、**Linux**（`KXToDo.AppImage` + `kxtodo-cli`）、
**Android**（`KXToDo.apk`）。GUI 是唯一常驻进程（内嵌 Host：IPC 服务端 + 调度引擎），CLI 不持有状态、经 IPC 找 Host。
数据是五个领域 JSON：`data.json` / `settings.json` / `tasks.json`（调度）/ `diary.json` / `ledger.json`，
外加 `runtime/`（同步水位、设备密钥、配对历史、缓存）与 `history/`（审计与调度历史）。

## 铁律（违反任何一条都算 bug）

1. **写路径只有一条**：GUI / CLI / Agent 三方一律走 Domain Core 的业务命令层（Invocation → 域分发 → envelope）。
   前端永不直接改 JSON，壳层（`src-tauri/src/lib.rs`）永不直接写领域文件，全部过 `Repository::write_*`
   （fs2 文件锁 + 原子写 + revision + 幂等台账 + 审计）。
2. **跨平台审视**：任何改动、新功能、bugfix 都必须过一遍 Windows / Linux / Android。
   底线是不许让任何一端不可用（编译不过、启动白屏、核心路径坏掉都算）；平台差异收敛到
   `src/lib/capabilities.ts` + Rust 的 `#[cfg(desktop)]` / `#[cfg(not(desktop))]` + CSS 的 `.app-shell.mobile`
   命名空间，**不要在业务组件里散落 isMobile 判断**。本地验证不了的平台交给 CI，但必须明说「该端未验证」。
3. **不做兼容**：项目没有需要兼容的旧版用户。数据格式与数据位置的变更一律直接切换，不写迁移代码
   （v0.8.0 已把 v8→v9 的 `migrate.rs` 整个删掉，别再把它加回来）。
4. **版本号只有 git 一个来源**：三个 `build.rs`、`release.sh`、`package.ps1` 共用同一套解析
   （HEAD 上的精确 `v*` tag → 最近 30 条 commit 里第一条 `vX.Y.Z` 主题 → 最近祖先 `v*` tag → 回落 `0.0.0-dev`）。
   **仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）。
5. **commit subject 必须以 `vX.Y.Z` 开头**（小写 v + 恰好三段纯数字），由 `scripts/git-hooks/commit-msg` 强制
   （`npm install` 的 prepare 自动装进本克隆）。merge / revert 用 `--no-verify` 显式绕过。
6. **发布四铁律**：① 本地构建只用 `release.ps1`（win/android/unix）与 `release.sh`（Linux 原生），
   `publish.ps1` 只是离线备用；② **每次推送远程前必须问用户这一版是否打 tag**；
   ③ **同一版本的修复只能 `git commit --amend`，不许新开 commit**（一个版本 = 一笔 commit；
   已打 tag 的先 `git tag -d` 再 amend 再重打）；④ **推送后必须监听远程 CI 直到结束**，红了必须处理。
7. **CSS 三条**：绝不写 `.xxx > *`（造成过两次严重布局事故，要抬层就显式列举正文流子元素）；
   字号一律 `calc(var(--font-*) ± N)`，不许写死像素；按钮只许 `settings-button` / `menu-action-button` 两类，
   菜单项一律走 `MenuItem` 组件。
8. **JS 命令式建出来的全屏/浮层，先问一句「移动端顶部安全区避让了吗」**（这条坑踩过三次）。
9. **记账金额一律整数分**（`amountCents`），余额只有「期初 + 流水」一个数据源；转账不计入收支统计。
   前端 `src/lib/ledger.ts` 与 core `ops_ledger.rs` 是同一套口径的两份实现，**改一边必须改另一边**。
10. **别用管道接构建/测试命令的输出**（`| tail` 会把退出码掩成 0，造成「构建成功」的误报）。要截断就重定向到文件再读。

## 常用命令

```bash
npm install                                  # 依赖（顺带装 commit-msg hook）
npm run desktop:dev                          # 桌面开发；Git Bash 下必须先导入 MSVC 环境：
                                             #   eval "$(grep -E '^(MSVC_|SDK_VER|export )' scripts/cargo-msvc.sh)"
npm run dev                                  # 只起 vite（1420）——跑 Playwright 回归前要先起着
npm run test:unit                            # 前端纯逻辑单测（vitest：资金路径 / 时刻 / normalize，时区无关）
npm run check                                # svelte-check（基线：0 error）
npm run build                                # 前端构建

cd src-tauri && ../scripts/cargo-msvc.sh test -p kxtodo-core       # Rust 测试（Git Bash 下必须用这个包装！
                                                                   #   uutils 的 link 会遮蔽 MSVC link.exe）
cd src-tauri && ../scripts/cargo-msvc.sh check --workspace --all-targets

node scripts/<某版>-fixes-test.mjs           # Playwright 回归（15 套，改动前端后全跑；清单见 SKILL.md）
node scripts/perf-bench.mjs                  # 性能基线：300 任务 / 300 日记 / 3000 账目下的首屏与交互耗时，
                                             #   并断言首屏不做完整 markdown 渲染（window.__kxtodoRenderStats）
.\release.ps1                                # 默认 Windows + Android；win / android / unix / all 可组合
git tag vX.Y.Z && git push origin main vX.Y.Z  # 云端发布：触发 release.yml 构建三平台并发 release
```

## 路由表（要做什么 → 动哪里 → 读哪份 reference）

| 要做什么 | 动哪里 | 先读 |
|---|---|---|
| 加/改 CLI 命令 | `crates/core/src/cli.rs`（clap 树）+ 对应 `ops_*.rs`；`schema.rs` / `skills.rs` 自动跟随 | `references/cli.md` |
| 加 GUI 写操作 | `crates/core/src/ops_gui.rs` 加命令 → `src/lib/actions.ts` 加 coreDispatch 包装 → 组件调 actions | `references/frontend.md` |
| 加设置项 | `model.rs` SettingsFile + `ops_config.rs` 的 KNOWN_FIELDS/get/set + `defaults.ts` normalize + `types.ts` + `SettingsDrawer.svelte`（+ 若该多端一致还要进 `merge.rs` 共享子集） | `references/invariants.md` |
| 改任务 / 日记 / 记账域 | 见 skill 的路由表完整版；记账另需同步动 `ledger_archive.rs`、`ledger_icons.rs` 镜像与 `tests/ledger_icons.rs` | `references/ledger.md` |
| 加一类**要同步的**实体 | 日记（kind `diary`）是现成样板：`model.rs` → `repo.rs` → `merge.rs` → `engine.rs` **五处** → `host.rs` 的事件 match → `core.rs`/`cli.rs` 的 schemaVersions → `lib.rs` 的 `core_snapshot` → 前端 `CoreSnapshot`/`applySnapshot`/`normalize*` | `references/sync.md` |
| 改同步协议 / 加密 / 传输 | `crates/core/src/sync/`（crypto / merge / transport / endpoint / engine / images / discovery / state / credentials）+ `crates/server/src/`。**改「连哪儿」不许动 merge/crypto** | `references/sync.md` |
| 改界面 / 外观 | 全局 CSS 按区域找（`src/styles/*.css`，级联顺序见 `main.ts`）；配色变量在 `base.css` | `references/ui-patterns.md`、`references/frontend.md` |
| 改超链接增强 | `crates/core/src/linkmeta.rs`（改了元数据字段就抬 `CACHE_VERSION`）+ `src/lib/linkPreview.ts` + `markdown-ext.css` | `references/ui-patterns.md` |
| 加原生能力 | Tauri 命令/插件；桌面专有逻辑必须 `#[cfg(desktop)]` 隔离并给移动端空实现，**两个 `generate_handler!` 都要注册** | `references/architecture.md` |
| 构建 / 发版 / CI | `release.ps1`、`scripts/package.ps1`、`release.sh`、`.github/workflows/{ci,release}.yml` | `references/build-and-release.md` |
| 遇到环境怪问题 | Windows（cargo link / pwsh 编码 / 单实例撞车）、Linux（依赖库 / 裸 cargo 出白屏制品 / 托盘）、Android（gradle / NDK / 返回键 / 触摸语义） | `references/pitfalls-*.md` |
| 查某版为什么这么设计 | `references/history/`（按版本归档的界面细节与踩坑） | — |
