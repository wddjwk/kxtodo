# Linux 开发坑位

> **这份文件是什么**：在 Linux / WSL 上构建与运行 KXToDo 会踩的坑——apt 依赖清单与 release.sh 的门控（含 libxdo 例外）、**裸 cargo 构建出 dev 模式制品导致整窗白屏**、托盘依赖 appindicator 宿主、AppImage 需要 FUSE、cargo 直接可用、WSLg 的 XWayland 丢光标与 AppImage 强制 x11。
> **什么时候读它**：要在 Linux/WSL 上构建或跑 KXToDo、Linux 制品白屏、托盘不出现、AppImage 跑不起来、光标消失、要改 `release.sh` 的依赖门控。
> Linux 的进程拓扑与 core 内 unix 差异见 `architecture.md`「进程拓扑」的最后一条；Linux 桌面的 UI/通知/托盘降级行为见 `ui-patterns.md`「Linux 桌面」；WSL 原生克隆构建路径见 `build-and-release.md`。

## 目录

- [1. 官方 apt 依赖清单](#1-官方-apt-依赖清单)
- [2. 裸 cargo 构建出 dev 模式制品（整窗白屏的真正根因）](#2-裸-cargo-构建出-dev-模式制品整窗白屏的真正根因)
- [3. 托盘依赖 appindicator 宿主](#3-托盘依赖-appindicator-宿主)
- [4. AppImage 运行需 FUSE](#4-appimage-运行需-fuse)
- [5. cargo 在 Linux 直接可用](#5-cargo-在-linux-直接可用)
- [6. WSLg 的 XWayland 丢光标 + AppImage 强制 x11](#6-wslg-的-xwayland-丢光标--appimage-强制-x11)

## Linux 开发坑位

### 1. 官方 apt 依赖清单

`sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`。release.sh 启动时门控：pkg-config 查 `webkit2gtk-4.1` / `gtk+-3.0` / `librsvg-2.0` / `openssl`，appindicator 接受 `ayatana-appindicator3-0.1` **或** `appindicator3-0.1` 任一；**libxdo 例外**——Debian/Ubuntu 的 `libxdo-dev` 只装 `/usr/include/xdo.h` 不带 `libxdo.pc`，所以改查头文件 + `libxdo.so*`（有 .pc 时优先 pkg-config），否则会在装齐依赖的机器上误报缺库。缺库直接打印上面这行安装命令并退出。

### 2. 裸 cargo 构建出 dev 模式制品（整窗白屏的真正根因）

Tauri 的 dev/prod 由编译期 `custom-protocol` feature 决定（`tauri::is_dev() = !cfg!(feature = "custom-protocol")`），该 feature **只有 tauri CLI 构建时才会启用**。裸 `cargo build --release` 产出的二进制会去加载 `devUrl`（127.0.0.1:1420），没有 vite 服务时整窗空白——webview 的网络日志里能看到连 1420 的痕迹，极易误判为渲染/GPU 问题。Linux 制品一律走 `npx tauri build`（release.sh 已如此）；手工验证构建用同命令或显式 `cargo build --release --features tauri/custom-protocol`。WSLg 下 stderr 的 MESA ZINK / libEGL 警告与 Gtk-CRITICAL scale-factor 告警是渲染栈噪音，与白屏无关；个别 GPU 栈真遇渲染问题才手动 `export WEBKIT_DISABLE_DMABUF_RENDERER=1`（WebKit 官方开关）。

### 3. 托盘依赖 appindicator 宿主

GNOME 默认不显示托盘图标（需装 AppIndicator 扩展），WSLg 下基本不可见——因此 Linux 默认关闭 close-to-tray（关闭按钮直接退出应用），设置项仍可改回。

### 4. AppImage 运行需 FUSE

WSL2 有 `/dev/fuse` 可直接跑；无 FUSE 的环境用 `./KXToDo.AppImage --appimage-extract-and-run`。**首次构建**时 tauri CLI 会自动下载 linuxdeploy，需要网络（之后走缓存）。

### 5. cargo 在 Linux 直接可用

`scripts/cargo-msvc.sh` 只是 Git Bash 下 `link.exe` 被遮蔽的解法，Linux 裸 cargo 即可；构建输出同样不要接 `| tail` 之类管道（掩退出码），release.sh 一律直通终端。

### 6. WSLg 的 XWayland 丢光标 + AppImage 强制 x11

本机 WSLg 实测——任何 X11 客户端（连系统 GTK 探针程序）都看不见鼠标光标，Wayland 原生一切正常；而 AppImage 的 AppRun（linuxdeploy-plugin-gtk 钩子）为绕旧版 WebKitGTK 的 Wayland 崩溃（tauri#8541）强制 `GDK_BACKEND=x11`，正好踩进坏通路（表现：整窗交互正常但光标消失）。

**v0.8.10 修正**：撤销只对 **WSLg** 生效（`WSL_INTEROP` / `WSL_DISTRO_NAME` 存在时），真实 Linux 桌面保持 AppRun 的 x11。v0.8.9 曾在所有 Wayland 会话里撤销，Ubuntu/GNOME 上出现无响应与卡顿——那不是标准 AppImage 路径。NVIDIA 专有驱动另见 `build-and-release.md` 的 AppImage 图形一节。

### 7. AppImage 在老 Ubuntu 报 GLIBC_2.4x not found（v0.9.3）

现象：`kxtodo: /lib/x86_64-linux-gnu/libc.so.6: version GLIBC_2.43 not found (required by /tmp/.mount_.../usr/lib/libwebkit2gtk-4.1.so.0)` 等一串，指向 **AppImage 内部**的打包库。根因不是代码依赖升级，而是 CI runner 镜像漂移 + linuxdeploy 打包构建机系统库（见第 6 条构建基线）。判别：报错路径带 `/tmp/.mount_` = 打包库；不带 = 真的系统库缺失。修：pin 老基底重发，不要试图在用户机器上装新 glibc。

### 8. AppImage 自带旧 GLib 与系统托盘库符号不匹配 → 启动 panic（v0.9.4）

现象：Ubuntu 24.04 上 `./KXToDo.AppImage` 先刷一串 `/usr/lib/.../libgvfscommon.so: undefined symbol: g_task_set_static_name`、`Failed to load module .../libgvfsdbus.so`、canberra-gtk-module 噪音，随后 `thread 'main' panicked at libappindicator-sys-0.9.0/src/lib.rs:41: Failed to load ayatana-appindicator3 or appindicator3 dynamic library`，exit 101。系统明明装了 libayatana-appindicator3-1。

根因：与第 7 条的 glibc 基底**不是同一层**。linuxdeploy-plugin-gtk 把构建机（22.04）的旧 GLib（2.72）打进包，AppRun 的 `LD_LIBRARY_PATH` 让它优先于系统 GLib 加载；24.04 的系统托盘库 libayatana-ido3 按 GLib 2.80 编译，要 `g_once_init_leave_pointer`/`g_task_set_static_name` 这两个新符号，在旧 GLib 里找不到 → dlopen 失败。gvfs/canberra 的 undefined symbol 是同一旧 GLib 的连带噪音（不致命），真正杀进程的是托盘库。判别：报错里 `undefined symbol: g_*` + 托盘 panic = 自带 GLib 问题；`GLIBC_2.xx not found` = 第 7 条 glibc 基底问题。

修（三件齐）：① release.sh 解包删 GLib 家族五库 + gio/modules、sed 删 AppRun 的 GIO_MODULE_DIR、appimagetool 重打包（GLib 各发行版必装且 ABI 向后兼容，22.04 构建的 webkit 链接的符号在 22.04+ 系统 GLib 都有）；② `setup_tray` 包 `catch_unwind`——dlopen 失败是 **panic 不是 Err**，原有 `if let Err` 降级接不住（release 禁 panic=abort，unwind 可用）；③ `scripts/verify-appimage.sh` 双 Ubuntu 容器真启动冒烟，发布前必跑。**不要**试图在用户机器上装/换 GLib，也不要给托盘 panic 加 `panic=abort`。

**v0.9.4 补充（同条根因的第二个库 + 验证判据）**：剔 GLib 后 22.04 冒烟即过；24.04 还会报 `libcurl-gnutls.so.4: undefined symbol: nghttp2_option_set_no_rfc9113_…` + gio 的 libgiolibproxy/libgiognutls 加载失败——包内旧 **nghttp2** 优先于系统新 nghttp2，同属「自带旧库压系统新库」，release.sh 一并剔 `libnghttp2.so*`。另：解包出的 AppDir 目录带 0700（CI umask），appimagetool 会报几十条 permission problem 并 exit 1 拒包，重打包前必须 `chmod -R a+rX`。**verify-appimage.sh 的致命 grep 只认 `panicked|Failed to load ayatana|undefined symbol: g_`**——宽到任意 `undefined symbol` 会把上述非致命 gio 噪音误判成红（第三轮实测）；DRI3/AT-SPI/dbind 警告属容器噪音不致命。
