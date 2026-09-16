# Windows 开发坑位（含 computer-use 调试经验）

> **这份文件是什么**：在本机（Windows + Git Bash + MSVC + pwsh 5.1）开发与调试 KXToDo 会踩的全部坑——cargo 链接失败、taskkill、单实例标识、window-state 插件、pwsh 编码、Android 交叉检查环境、Node 24 libuv flake；以及用 computer-use 驱动 WebView2 应用做 UI 验证的 8 条实战经验。
> **什么时候读它**：在 Windows 上跑 cargo / npm / release 脚本报错；改了代码但「没生效」；窗口尺寸/位置异常；脚本里的中文乱码或语法错误；要用截图+坐标的方式验证桌面 UI。
> Linux 的坑在 `pitfalls-linux.md`，Android 的在 `pitfalls-android.md`。

## 目录

- **环境坑位（Windows 开发必读）**
- [1. Git Bash 里 cargo 链接失败](#1-git-bash-里-cargo-链接失败)
- [2. PowerShell 必须 `-NoProfile` 调脚本](#2-powershell-必须--noprofile-调脚本)
- [3. Git Bash 里 `taskkill /PID` 会被转成 UNC 路径](#3-git-bash-里-taskkill-pid-会被转成-unc-路径)
- [4. 单实例标识 `com.wddjwk.kxtodo` 全局唯一](#4-单实例标识-comwddjwkkxtodo-全局唯一)
- [5. tauri-plugin-window-state 默认管所有窗口](#5-tauri-plugin-window-state-默认管所有窗口)
- [6. pwsh 5.1 的编码坑（两个方向都会咬人）](#6-pwsh-51-的编码坑两个方向都会咬人)
- [7. 裸 cargo 交叉检查 Android 目标需要 NDK clang 环境](#7-裸-cargo-交叉检查-android-目标需要-ndk-clang-环境)
- [8. Node 24 + vite 在 Windows 的退出期 libuv 断言 flake](#8-node-24--vite-在-windows-的退出期-libuv-断言-flake)
- **computer-use 调试经验（WebView2 应用）**
- [1. 截图坐标系](#1-截图坐标系)
- [2. input_revision 是你的朋友](#2-input_revision-是你的朋友)
- [3. hover 无法合成](#3-hover-无法合成)
- [4. vite dev 长开 + HMR 会污染判断](#4-vite-dev-长开--hmr-会污染判断)
- [5. 验证数据的权威来源是磁盘文件](#5-验证数据的权威来源是磁盘文件)
- [6. Esc 是 CU 的取消键](#6-esc-是-cu-的取消键)
- [7. 通知/托盘弹窗是独立窗口](#7-通知托盘弹窗是独立窗口)
- [8. 定时任务触发等时间相关验证](#8-定时任务触发等时间相关验证)

## 环境坑位（Windows 开发必读）

### 1. Git Bash 里 cargo 链接失败

（"link: extra operand"）：uutils-coreutils 的 `link` 遮蔽了 MSVC `link.exe`。**一切 cargo 调用走 `scripts/cargo-msvc.sh`**。**`npm run desktop:dev` 同样中招**（`tauri dev` 内部调裸 cargo，会刷一屏 `link: extra operand` 然后构建失败）：先导出包装脚本里的环境再跑 npm——`eval "$(grep -E '^(MSVC_|SDK_VER|export )' scripts/cargo-msvc.sh)" && npm run desktop:dev`，或直接在 VS 开发者 shell 里跑。另：`tauri dev` 异常退出可能留下孤儿 vite 占着 1420（它的 kill-tree 在本机 PowerShell 5.1 上因缺 CimCmdlets 失效），下次启动报 `Port 1420 is already in use`，`netstat -ano | grep :1420` 找 pid 杀掉即可。

### 2. PowerShell 必须 `-NoProfile` 调脚本

的说法反过来也成立：package.ps1/release.ps1 已内置 MSVC 环境自举（cl.exe 不在 PATH 时从 setup_x64.bat 导入），直接跑即可。

### 3. Git Bash 里 `taskkill /PID` 会被转成 UNC 路径

——杀进程用 `powershell -NoProfile -Command "Stop-Process -Id <pid> -Force"`。

### 4. 单实例标识 `com.wddjwk.kxtodo` 全局唯一

debug、release、旧版本 exe 互相同标识，启动新实例会转发到已运行的旧实例（表现为"改了代码没生效"）。**调试前先把所有 kxtodo/KXToDo 进程杀光（含托盘）**；用户反馈"修复没生效"也优先怀疑旧进程残留。

### 5. tauri-plugin-window-state 默认管所有窗口

动态 label 的窗口（通知窗 `notification-N` 按进程内计数器复用 label）会被恢复历史"不可见/旧位置"状态——曾导致通知窗建好了却看不见。必须用 `with_filter` 按前缀排除；主窗口可见性若由代码接管（conf `visible:false` + 前端 reveal），还要把 `StateFlags::VISIBLE` 从持久化里剥掉，否则恢复逻辑会绕过 reveal 提前显示窗口。**还有一个竞态（v0.6.7 兜底）**：Windows 最小化会把窗口挪到 (-32000,-32000) 并压成极小尺寸，插件「最小化时不记录」的守卫可能赶在系统标记最小化之前失效，脏值进进程内缓存、退出时落盘，下次启动原样恢复——窗口缩成一条 144×19 的标题栏小条。`reveal_main_window` 在 show 之前跑 `sanitize_main_window_geometry`：位置 ≤ -30000 或尺寸小于 400×300 就重置成 1180×820 居中（修好之后退出时落盘的就是好值，自愈）。

### 6. pwsh 5.1 的编码坑（两个方向都会咬人）

① **脚本输出**——5.1 写文件默认 GBK，脚本里写中文文本一律 `[System.IO.File]::WriteAllText` + UTF-8 no-BOM。② **脚本自身**——含中文的 `.ps1` 必须带 UTF-8 BOM，否则 5.1 按 ANSI(936) 读源码，中文注释里的字节会撞坏引号/括号配对，整个文件直接解析失败：实测 `release.ps1` / `scripts/package.ps1` / `scripts/publish.ps1` 各报 4~5 个 parse error，而 `npm run package` 走的正是 `powershell`（5.1）→ 完全跑不起来；pwsh 7 默认按 UTF-8 读、CI 也用 pwsh，所以这个坏法在 7 下和 CI 里都照不出来（`release.ps1` 用 `&` 同进程调 `package.ps1`，跟着父 shell 走）。v0.1.1 补过一次 BOM 后来丢了，v0.4.2 重新补上。**校验**：`powershell -NoProfile -Command '$t=$null;$e=$null;[void][System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path "x.ps1").Path,[ref]$t,[ref]$e); $e.Count'` 必须是 0（注意 `ParseFile` 的 AST 是返回值，第二个 `[ref]` 收的是 tokens）。**补 BOM**：`printf '\xef\xbb\xbf' | cat - f.ps1 > t && mv t f.ps1`，字节级、不动 CRLF。

### 7. 裸 cargo 交叉检查 Android 目标需要 NDK clang 环境

ureq/ring 是共享依赖后，`cargo check --target aarch64-linux-android` 会在 ring 的 cc-rs 构建脚本里找 clang 失败。gradle 的 rust 插件（`tauri android build`）会自己配好；手工 check 需导出：`PATH += $NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin`、`CC/CXX/AR_aarch64_linux_android=clang.exe/clang++.exe/llvm-ar.exe`、`CFLAGS/CXXFLAGS_aarch64_linux_android=--target=aarch64-linux-android24`。**v0.8.2 实测的省事版**：`PATH` 前置 `$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin` + `CC_aarch64_linux_android` 指到 `aarch64-linux-android24-clang.cmd` + `AR_aarch64_linux_android=llvm-ar.exe`，`cargo check --target aarch64-linux-android -p kxtodo-core -p kxtodo-server` 就能过（本机的 `NDK` 环境变量已指向 NDK 根；仍然要经 `scripts/cargo-msvc.sh` 跑，宿主侧的 build script 需要 MSVC 工具链）。

### 8. Node 24 + vite 在 Windows 的退出期 libuv 断言 flake

`npm run build` 可能已打印 `✓ built in Xs` 但进程退出时崩在 `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)`（exit -1073740791），package.ps1 会按约定报 "Frontend build failed"；`tauri android build` 的 beforeBuildCommand 再跑一次 npm 时同样会中。**偶发，直接重跑该目标即可**（产物以 release/ 下文件为准）。另外：**跑 release/publish 脚本不要用 `| tail` 管道**——管道会把退出码掩成 0 造成"构建成功"误报，重定向到日志文件再 tail。

## 另外三条与 Windows 有关的坑（住在 sync.md 的「踩坑记录」里）

原 AGENTS.md 把它们记在同步章的「踩坑记录」①–⑦ 中，全文见 `sync.md` 的「踩坑记录」小节：

- ② **PowerShell 5.1 跑 CLI 测试脚本要 `[Console]::OutputEncoding=UTF8`**，否则 GBK 解码 UTF-8 JSON 炸。
- ④ **Windows CLI 中文参数乱码**（代码页 936）：`std::env::args()` 按系统 ANSI 代码页解码命令行，中文必乱码——cli 入口已改 `args_os` + `embed-manifest` crate 嵌入进程级 UTF-8 代码页 manifest（手写 `/MANIFESTINPUT` 链接参数会造成 side-by-side 启动错误，勿回退）。
- ⑤ **PowerShell 5.1 脚本文件本身必须带 UTF-8 BOM** 否则中文按 GBK 解析直接语法错误。
- ⑥ **`wmi` 版本钉死 0.18.1**（iroh→netwatch→wmi 的版本范围错配，Windows 上必编不过）——也记在 `build-and-release.md`。

## computer-use 调试经验（WebView2 应用）

KXToDo 的窗口内容是 WebView2 渲染的，**UIA 拿不到 DOM 树**（accessibility 为空），只能用像素截图 + 坐标点击。多轮实战总结：

### 1. 截图坐标系

`get_app_state` 返回的坐标是缩放后截图像素，click/scroll/type 直接用；每次操作后必须重新 get_app_state 拿新 snapshot_id 和新坐标，不要复用旧坐标点变了位置的元素。

### 2. input_revision 是你的朋友

两次截图 revision 相同 = 画面没变（操作没生效，或捕获到了缓存帧）。不同 = 真的重绘了。判"点没点上"先看 revision。

### 3. hover 无法合成

CU 没有"移动鼠标不点击"，悬停子菜单、悬停展开这类行为测不了。用 Playwright（`channel: "msedge"` 直接驱动系统 Edge/WebView2 内核）连 vite dev server（`npm run dev` 的 1420 端口）补测 hover 路径——webapp-testing 技能的 `with_server.py` 可管生命周期。

### 4. vite dev 长开 + HMR 会污染判断

dev 实例在多轮源码热更后，运行中的组件可能持有新旧混合的响应式状态，表现出"磁盘数据正确但界面不对""同一帧里两个状态混渲"等灵异现象。**改完前端代码验证前，杀掉 dev 实例重启干净进程**；vite server 本身不用重启（它只serve源码）。

### 5. 验证数据的权威来源是磁盘文件

默认数据目录（Windows `%LOCALAPPDATA%\kxtodo\todo-note-data`）下的 *.json 直接可读。界面存疑时先 `cat` 数据文件区分"写错了"还是"画错了"，能省一半时间。

### 6. Esc 是 CU 的取消键

对目标应用发 Esc 可能被 CU 层拦截/取消会话，测试"Esc 关闭浮窗"这类交互时优先用 Playwright 或改用其他关闭路径验证。

### 7. 通知/托盘弹窗是独立窗口

`list_windows` 里主窗口旁的小窗口（如 `com.wddjwk.kxtodo-siw`、通知窗）要按 hwnd 单独截图。

### 8. 定时任务触发等时间相关验证

用 CLI 造一个 `once` 任务设到 1 分钟后，比改系统时间或注入时钟省事得多；触发后看 `schedule logs` 和磁盘 state 即可闭环。
