# 构建 / 测试 / 发布 / CI

> **这份文件是什么**：全部构建、测试、回归脚本、发布入口、版本号解析规则、commit-msg hook、七个固定名产物、应用内更新的下载通道、GitHub Actions 流水线（ci.yml / release.yml）、Android 签名，以及**发布工作流铁律**。
> **什么时候读它**：要跑测试、要出包、要打 tag、要推送远程、要改版本号解析或 CI、要排查构建失败、要部署或自升级 kxtodo-server。
> **最短路径**（最常用的几条命令）在 SKILL.md 的「构建 / 测试 / 验证的最短路径」。
> Windows 上 cargo 报 `link: extra operand` → `pitfalls-windows.md`；Linux 构建出白屏制品 → `pitfalls-linux.md`；Android 构建 → `pitfalls-android.md`。

## 目录

- [常用命令（构建 / 测试 / 回归脚本）](#常用命令构建--测试--回归脚本)
- [版本号：只有 git 一个来源](#版本号只有-git-一个来源)
- [commit-msg hook 强制 subject 以 vX.Y.Z 开头](#commit-msg-hook-强制-subject-以-vxyz-开头)
- [产物：七个固定名](#产物七个固定名)
- [kxtodo-server 的双平台发布](#kxtodo-server-的双平台发布)
- [应用内更新的下载通道：多通道测速选路](#应用内更新的下载通道多通道测速选路)
- [GitHub Actions 流水线（.github/workflows/）](#github-actions-流水线githubworkflows)
- [发布工作流铁律（Agent 与维护者都要遵守）](#发布工作流铁律agent-与维护者都要遵守)

## 常用命令（构建 / 测试 / 回归脚本）

```bash
npm install                # 依赖
npm run desktop:dev        # 桌面开发（vite + tauri dev）
scripts/cargo-msvc.sh test -p kxtodo-core   # Rust 测试（Git Bash 下必须用这个包装！）
npm run test:unit          # 前端纯逻辑单测（vitest 5，node 环境：资金路径 / 时刻 / normalize；断言必须时区无关，见 frontend.md「前端单测」）
node scripts/perf-bench.mjs # 性能基线（v0.8.0）：往 localStorage 塞 300 任务 / 300 日记 / 3000 账目，量冷启动与页内 rAF 计时的交互，并断言 window.__kxtodoRenderStats 首屏 block 渲染为 0（需先 npm run dev；Node 侧计时会被自己的 waitForTimeout 淹没，所以交互一律页内计时）
.\release.ps1              # 默认 Windows + Android（KXToDo.exe + kxtodo-cli.exe + KXToDo.apk）
.\release.ps1 win / android / unix   # 单平台（unix = 经 WSL 原生克隆构建 KXToDo.AppImage + kxtodo-cli）
.\release.ps1 win,unix     # 逗号组合；.\release.ps1 all = 三平台（环境未就绪告警跳过，不终止其它）
./release.sh               # Linux 构建入口（须在 Linux/WSL 上跑；release.ps1 unix 与 CI release.yml 都复用它）
node scripts/mobile-ux-test.mjs   # 移动端 UX 回归（playwright-core + 系统 Edge，需先 npm run dev）
node scripts/diary-ux-test.mjs    # 日记 + 搜索混排 + 编辑器元数据行回归：桌面三视图/齿轮菜单/主题色背景/全局搜索混排/加号新建，移动端搜索结果面板与日记整页层
node scripts/menu-sweep-test.mjs  # 菜单/浮层巡检：桌面 + 360px 移动模拟下逐个唤起右键/三点/齿轮/编辑器浮层/二级子菜单，断言全部落在视口内且无 pageerror（浮层越界类 bug 的守门员）
node scripts/markdown-ext-test.mjs # markdown 扩展回归：callout/公式/mermaid/markmap/代码折叠/front-matter/链接图标/图全屏与源码切换
node scripts/v068-fixes-test.mjs  # v0.6.8 修复项回归：移动端标签红叉点按露出、日记透明度条跟手、完成/新增其它任务不改变展开集合
node scripts/v069-fixes-test.mjs  # v0.6.9 修复项回归：日记长单行可展开与展开态稳定、移动端编辑器 markdown 工具栏、桌面编辑器宽高比例、一般卡片移动端占满、标签两段式点按、任务编辑器加号按钮可见
node scripts/v0610-fixes-test.mjs # v0.6.10 回归：桌面编辑器工具栏与特性开关、标题光标位置、同步总开关隐藏配置、一般卡片 Markdown 导出/导入菜单项、移动端工具栏常显
node scripts/v0611-fixes-test.mjs # v0.6.11 回归：列表前缀光标、工具栏顺序、移动端触发器只留图标、特性开关灰卡、已完成默认折叠与记忆、分割线细虚线
node scripts/ledger-ux-test.mjs    # 记账回归：桌面四视图/记一笔/转账/分类与账户管理/热力与统计/同步五勾选，移动端整页层与返回键
node scripts/v073-fixes-test.mjs   # v0.7.3 回归：时刻滚轮与落盘/保存语义/改转账不新增/转账浮层不被裁/分类加号/统计钻取/汇总不截断/设置分区重组与固定分组/同步账户不预填/移动端遮罩吞点击
node scripts/v074-fixes-test.mjs   # v0.7.4 回归：两行卡片与左右对齐/条目图片图标与菜单/编辑器圆形分类+内联二级+无框备注+定高抽屉/统计双段控+白块+周与自定义+收支双曲线/钻取独立配色/账户自定义类型与不 autofocus/设置段控等分与子分组分隔
node scripts/v075-fixes-test.mjs   # v0.7.5 回归：灰色汇总与无备注 solo 居中/多图角标与旧单图折叠/加号常驻/编辑器行内搁板与定高五排/统计分平台段控+结余三曲线+NaN修复+读数/饼图动效/自定义账户类型与直设余额/资产趋势图两端/图标选择器分组/桌面工具箱/移动高亮条与表单滚到底
node scripts/v076-fixes-test.mjs   # v0.7.6 回归：移动端去滚动条与卡片等距/左上角返回箭头（特性开关默认关，四个整页都有一支）/编辑器滑块无点击遮罩/移动统计段控 -2px 不超屏/自定义日期点外收起/趋势两段式（卡片带坐标轴、浮层、全屏按钮、标题栏在屏幕内）/分类与账户表单滚到底（flex 抽屉修 grid 撑高）/桌面编辑框正方形且开搁板不变高/日历左右滑动换月（日记+记账）/齿轮与段控等高/固定分组无选中底色/FAB 上移且「今」在加号上方/记账搜索（页内汇总+单条卡+金额命中）与全局混排/账户行带图标（含转入）/图片图标在备注区且等高、列表无角标、列表看图左右可用不退出、滑动翻页
node scripts/v078-fixes-test.mjs   # v0.7.8 回归：移动端输入法跟随（shell 收矮/平移公式，真机才走得到那条分支）/桌面三点菜单 toggle（再点收起、点别处仍关）/日记与记账齿轮面板宽度自适应/记账列表贴边手势换月（装不满一屏也能换）与反复上下换月/移动端转账键盘完整可见/展开全部与收起全部覆盖「单行超长要折行」的卡片/超链接渲染（标题 60 字、卡片铺满内容宽 + 6px 圆角 + 悬浮复制图标 + 标题摘要两行 + 网页图标、桌面悬浮无下划线、设置里合并成一组「超链接渲染样式」）/移动端头部齿轮关掉蓝色点按高亮/同步账户太短时点「开始同步」给提示
node scripts/v077-fixes-test.mjs   # v0.7.7 回归：移动齿轮收起不留底色/编辑器 Esc（标签输入框与内联编辑不再吃掉 Escape，两段式）/桌面记账编辑框高度=宽度−44 且三排贴底、开搁板不跳、搁板带进视野/趋势点图直接读数（桌面悬浮+点击、移动端点按）与只有全屏按钮进全屏（移动直接横屏）/坐标标签互不覆盖/选择图标「常用图标」置顶（最近使用、最多两行、混排）与简笔画区五行自滚/移动端表单图标网格自滚/移动端总资产三块靠右（桌面靠左）/超链接自动标题（30 字截断、手写不动、开关可关）与渲染为卡片（站点+标题+摘要+复制按钮、默认关）
.\scripts\publish.ps1      # 本地一键发布（离线备用路径，基本不再用——日常发布走 tag 触发云端构建；默认 Windows+Android，all = 三平台）
git tag vX.Y.Z; git push origin main vX.Y.Z   # 云端发布：触发 GitHub Actions release.yml 构建三平台并发 release（无需本地构建环境）
```

前端构建的两条 chunk 规则（`vite.config.ts`）：`build.rollupOptions.output.manualChunks` 把 `node_modules/katex/` 拆成独立 chunk（v0.8.0——公式渲染只在预览里用，不该压在首屏 chunk 里）；`optimizeDeps.include: ["mermaid"]` 仍然不能删（原因见 `ui-patterns.md`「markdown 扩展渲染」）。

## 版本号：只有 git 一个来源

**版本号只有 git 一个来源**：三个 `build.rs`（根 crate + `crates/core` + `crates/server`）、`release.sh` 的 `resolve_version`、`package.ps1` 的 `Get-GitVersion` 共用同一套优先级——**HEAD 上的精确 `v*` tag → 最近 30 条 commit 里第一条 `vX.Y.Z` 主题 → 最近的祖先 `v*` tag**，都没有则回落 `0.0.0-dev`。`build.rs` 构建期注入 `KXTODO_VERSION`，GUI 经 `app_version` 命令展示在设置页，CLI 的 `version` 命令同源。**仓库任何文件里都不写版本号**（Cargo.toml 是 0.0.0 占位、tauri.conf.json 无 version 字段、前端无常量）——发版只打 tag/写 commit，永远不要往文件里同步版本号。
改这套解析前必须知道的两个真实 bug（都已修，别改回去）：① **精确 tag 必须排在祖先 tag 之前**——`describe --abbrev=0` 找的是最近祖先 tag，让它优先的话未打 tag 的版本 commit 会静默拿上一个版本号，且 subject 分支永远轮不到；② **subject 校验只能取第一个空白前的 token**——拿整行去 `split('.')`，最后一段必然含中文说明，会让这条回退对所有真实 subject 都失效（等于死代码，一路回落 `0.0.0-dev`）。发版仍建议先 commit 再在 HEAD 打 tag（`publish.ps1` 有同样兜底），但版本号正确性已不再依赖这一步。

## commit-msg hook 强制 subject 以 vX.Y.Z 开头

**这条格式由 commit-msg hook 强制**：源文件版本化在 `scripts/git-hooks/commit-msg`，`npm install` / `npm ci` 的 `prepare` 经 `scripts/install-git-hooks.mjs` 把它拷进本克隆的 hooks 目录（不用 `core.hooksPath`——那会整体接管 hooks 目录，屏蔽掉别的工具挂在 `.git/hooks` 里的 hook）。subject 不以 `vX.Y.Z` 开头直接拒绝提交，判据与上面几处解析器完全一致：**小写 `v` + 恰好三段纯数字**，所以 `v1.2`（两段）、`v0.4.2.1`（四段）、`V0.4.2`（大写）都拒。两个实现细节坑：`package.ps1` 侧必须用 `-cmatch`（PowerShell 的 `-match` 默认大小写不敏感，会放行 `V1.3.0`）；hook 文件在 `.gitattributes` 里钉了 `scripts/git-hooks/** eol=lf`——它没有 `.sh` 扩展名不被原有规则覆盖，`core.autocrlf=true` 下新克隆检出成 CRLF 会把 shebang 变成 `#!/bin/sh\r`，hook 静默失效等于整条强制形同虚设。merge / revert 等自动生成的消息用 `git commit --no-verify` 显式绕过。新克隆跑一次 `npm install` 即自动装上。

## 产物：七个固定名

产物：七个固定名（不带版本号）——Windows `release/KXToDo.exe`（GUI）+ `release/kxtodo-cli.exe`（CLI）+ `release/kxtodo-server.exe`（同步服务端）；Android `release/KXToDo.apk`（覆盖安装不留历史包，已不再写 `.version` sidecar；**v0.8.0 起只含 `aarch64-linux-android` + `armv7-linux-androideabi` 两个 ABI**——x86/x86_64 纯为模拟器服务却占掉 APK 一半体积（实测 71.5MB 里 35.4MB 是这两个 ABI 的 .so），而移动端回归验证走 Playwright + 系统 Edge 不依赖 x86 APK；armv7 刻意保留覆盖真机。`package.ps1` 与 release.yml 都 `rustup target add` 两个 + `npx tauri android build --apk --target aarch64 --target armv7`，产物收集**分档兜底** `universal/` → `arm64-v8a/` → 最新的那个：给了显式 `--target` 后 gradle 可能出的是逐 ABI 的包，而「最新的那个」会在两个 ABI 之间摇摆，只当最后兜底。**别为了「万一有人用模拟器」把 x86 加回来**）；Linux `release/KXToDo.AppImage`（GUI）+ `release/kxtodo-cli`（CLI 裸二进制）+ `release/kxtodo-server`（同步服务端）。**仓库文件仍不写版本号**：GUI/CLI 二进制里的版本由 `build.rs` 构建期解析 git 注入 `KXTODO_VERSION`；AppImage 的版本经 `tauri build --config "{\"version\":\"$VERSION\"}"` 内联 JSON 注入，bundler 仍按官方 `productName_version_arch` 产出 `KXToDo_<版本>_amd64.AppImage`，`release.sh` 校验其含正确版本后改名成固定名 `KXToDo.AppImage`（bundle 由 `src-tauri/tauri.linux.conf.json` 平台覆盖启用，`targets` 仅 `appimage`）。构建入口分工：Windows/Android 走 `release.ps1` → `package.ps1`；Linux 走 `release.ps1 unix`，它经 `wsl.exe` 调 `scripts/wsl-linux-build.sh` 在 **WSL 原生克隆**（默认 `~/projects/kxtodo`，可用环境变量 `KXTODO_WSL_REPO` 覆盖）里同步到 Windows HEAD+tag → 跑 `release.sh` → 把 `KXToDo.AppImage`+`kxtodo-cli`+`kxtodo-server` 回拷到 Windows 的 `release/`（用原生克隆而非 /mnt/d：AppImage 打包在 ext4 上更快更稳；克隆有未提交跟踪改动时拒绝同步并告警跳过）。APK 的 versionName/versionCode 由 package.ps1 注入的 `KXTODO_VERSION` 环境变量进 gradle（versionCode = 900000000 + X*1000000 + Y*1000 + Z，基线高于旧构建的 8002001 保证升级不降级）。`publish.ps1`（离线备用路径，基本不再使用）默认重建 Windows+Android 产物（unix 需显式指定，如 `publish.ps1 all`；先删旧产物避免误传上一版本，缺环境的平台告警跳过），确保 HEAD 带 tag（无则按提交主题当场打）后推送并 `gh release` 上传。日常构建发布走 GitHub Actions（见下节），本地脚本与 CI 复用同一套构建入口（release.sh / tauri CLI / gradle）。

**发布制品体积基线（v0.8.0）**：`src-tauri/Cargo.toml` 的 `[profile.release]` = `lto = "thin"` + `codegen-units = 1` + `strip = true`（cargo 默认无 LTO + 16 codegen units + 不 strip，实测 KXToDo.exe 19.1MB / AppImage 83.5MB / APK 每 ABI 的 .so 约 18MB；预计全线 -25~35%，代价是编译时间明显变长，CI 三个 job 都会变慢）。**不要加 `panic = "abort"`**：GUI 是常驻 Host，调度器用 process_group/killpg 管整组子进程，让一次 panic 直接带走整个常驻进程比 unwind 到边界更糟；Android 的 cdylib 上 abort 也不友好。`opt-level` 保持默认 3——体积靠 LTO 与 strip 拿，不靠降优化档。

## kxtodo-server 的双平台发布

kxtodo-server 双平台发布——Linux `kxtodo-server`（release.sh 构建）+ Windows `kxtodo-server.exe`（package.ps1 / release.yml windows job 构建）；server 自带 `--update`（按平台从 GitHub latest release 下载对应固定名制品原子替换自身+重启）。CI 的 cargo test 已含 `-p kxtodo-server`（spawn 真实 server 的 e2e 收敛测试）。

## 应用内更新的下载通道：多通道测速选路

（v0.5.0 起，v0.6.2 健壮化，v0.7.6 改多代理测速。GUI/CLI/APK 与 server 自升级共用同一份实现。）

制品下载前把**官方直连 + 六个加速代理**一起测一遍速度，挑最快的一条下载，失败按速度顺序换下一条，官方直连留在候选表里兜底（测速失败也留着试一把），全都失败就把每条通道的原因一起报出来。通道清单与测速在 **`crates/core/src/update_fetch.rs`**（GUI/CLI/APK 走 `lib.rs::update_download_file`，server 自升级走 `update.rs::download_to`，两边共用这一份）：代理前缀 `https://gh-proxy.org/`、`v4.`、`v6.`、`cdn.`、`axisnow.gh-proxy.org/`、`https://ghfast.top/`；用法是把**完整的 github.com 制品链接拼在域名后面**（`https://gh-proxy.org/https://github.com/…/KXToDo.apk`）——走的是 github.com，**版本检查仍直连 api.github.com**（这些代理不接受 api 域名，返回 403 `Invalid input.`、也不发 CORS 头，所以这条路只能放在 Rust 侧且只覆盖下载）。测速 = 对同一 URL 发 `Range: bytes=0-262143` 读满 256KB 掐掉、量出字节/秒（单条 5 秒上限、样本 <64KB 当没测到、状态码 ≥300 当失败），七条通道并发探测合计 ~2.5 秒；`rank_routes` 稳定排序（同速/都没测到时保持原序，官方自然垫底）。GUI 进度事件先给一句「正在测速选择最快的下载通道…」，换通道时再给一句带通道名的提示；server `--update` 逐条打印「通道 X：N KB/s」。v0.6.2 的可靠化全部保留：①每次尝试都校验体积下限（`MIN_ARTIFACT_BYTES` 1MB，真实产物最小 20MB 上下）与 `received == content-length`，**体积不对/截断也算失败**——否则一个 200 的错误页会被当成产物；②换通道前清掉上一次留下的 `.part`，进度条从 0 重计，进度事件带 `note` 前端会显示；③agent 只设连接超时 + **读超时 60 秒**，刻意不设总超时（几十上百 MB 会超过任何合理总值，而连接后对端不发数据必须能脱身）；④拿不到 content-length 时按每 1MB 发一次进度；⑤server `--update` 流式写 `<exe>.part` 再原子改名落位，进度每 10% 打一行。实测（2026-09 本机）：六个代理与直连都可用（206 分片、~100–200 KB/s 量级），测速选出的第一名随网络状况变。

## GitHub Actions 流水线（.github/workflows/）

### ci.yml

（push main / PR）：只做编译检查，不构建发布产物、不发 release——前端（svelte-check + **vitest 单测**（v0.8.0 起 `npm run test:unit` 插在 `npm run check` 之后、`npm run build` 之前）+ vite build）、Windows 与 Linux 双平台 Rust（`cargo check --workspace` + core 测试）。Android 交叉编译重，不进 push CI（tag 发布时才构建，与 tauri 官方 test-android.yml 同思路）。Node 固定 22（规避 node 24 在 Windows 的退出期 libuv flake）。**workflow 级 `PYTHONUTF8=1` 不可删**：Windows runner 的 Python 默认 cp1252 stdout，core 调度测试的内联脚本 `print('中文')` 会 UnicodeEncodeError 退出 1 挂掉 3 个 host_scheduler 测试（Linux runner 与开发机 UTF-8 环境无感）。

### release.yml

（push tag `v*`）：三平台并行构建 → release job 汇总七个固定名产物 `gh release create/upload --clobber`（`--generate-notes`）。Windows = `tauri build --no-bundle` + cargo CLI + cargo server；Linux = **直接复用 `release.sh`**（与本地/WSL 同一入口；ubuntu-latest 24.04 与开发机 WSL 一致，AppImage 不打包 glibc，更老发行版需自行评估）；Android = 官方 tauri mobile CI 模式（`nttld/setup-ndk` r29 + `NDK_HOME` + ubuntu 符号链接修复 + temurin 21 配 AGP 8.11/Gradle 8.14；**v0.8.0 起 `targets:` 只有 aarch64-linux-android + armv7-linux-androideabi 两个**，构建命令显式 `--apk --target aarch64 --target armv7`，产物收集分档兜底 universal → arm64-v8a → 最新的那个，理由见「产物：七个固定名」）。某平台失败 → 只发布成功产物并整体标红（与 publish.ps1 告警跳过语义一致）；`workflow_dispatch` 手动触发 = 只构建验证不发布（release job 仅 tag ref 运行）。构建 job 一律 `fetch-depth: 0`（build.rs/release.sh 的版本解析需要 tag 历史）；`Swatinem/rust-cache` 以 `workspaces: src-tauri -> target` 在 CI 与发布间共享缓存（Android 用独立 shared-key）。**产物暂存必须用独立 `staging/`，禁止复用仓库根 `dist/`**（v0.3.3 教训：dist/ 是 vite 输出目录，tauri 的 beforeBuildCommand 会把前端文件灌进去，141 个 js/css 垃圾资产跟着制品上了 release，事后逐个 delete-asset 清理）；release job 另有七固定名白名单硬门控，build job 就算误传别的文件也进不了发布。

### Android 签名

keystore 存于仓库 secret `ANDROID_KEYSTORE_BASE64`（`base64 -w0 release.jks` 生成；轮换：`gh secret set ANDROID_KEYSTORE_BASE64`），CI 解码回 gradle 回退路径 `src-tauri/gen/android/keystore/release.jks`（密码/别名 `kxtodo` 在 build.gradle.kts 回退分支硬编码）——与本地 package.ps1 产物同签名，保证覆盖升级。**本地 keystore 与 secret 同时丢失 = 旧安装升级链断裂**（secret 本身也是一份异地备份）。

- 云端发布流程：commit → `git tag vX.Y.Z` → `git push origin main vX.Y.Z`，Actions 自动构建并发布（release.yml 固定构建全三平台，不做目标裁剪）。

## 发布工作流铁律（Agent 与维护者都要遵守）

  1. 本地构建只用 `release.ps1`（Windows/Android/unix）与 `release.sh`（Linux 原生）；`publish.ps1` 基本不再使用，仅作离线/CI 不可用时的备用路径。
  2. **每次推送远程前必须询问用户：这一版是否需要打 tag（即是否发 release）**。要打则 commit → 在 HEAD 打 `vX.Y.Z` tag → 一并推送分支与 tag；不打则只推分支。
  3. **同一版本的修复只能 amend，不许新开 commit**：用户要求实现某个版本（如 v0.4.2），该版本已经 commit 了，用户又反馈有问题——**在用户没有明确指定"这次修复要升版本号"之前，所有修改（bugfix、文档补充、CI 修复、遗漏的改动）一律 `git commit --amend` 并进那笔 `vX.Y.Z` commit**，保持"一个版本 = 一笔 commit"。已经打了 tag 的按顺序来：`git tag -d vX.Y.Z` → amend → 在 HEAD 重打 `vX.Y.Z`（tag 已推远程则 force-push tag，并按需处理对应的 GitHub release）。另起一笔 `vX.Y.Z 修复…`、或不带版本号前缀的散 commit，都算违规。
  4. **不论是否打 tag，推送后必须监听远程 CI 直到结束**（ci.yml 编译检查；打了 tag 还有 release.yml 三平台构建 + 发布）：确认检查通过、构建/发布成功；任何失败都必须后续处理（修复重推或 re-run failed jobs），不许放着红着不管。
