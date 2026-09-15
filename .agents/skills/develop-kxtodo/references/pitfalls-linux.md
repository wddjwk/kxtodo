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

本机 WSLg 实测——任何 X11 客户端（连系统 GTK 探针程序）都看不见鼠标光标，Wayland 原生一切正常；而 AppImage 的 AppRun（linuxdeploy-plugin-gtk 钩子）为绕旧版 WebKitGTK 的 Wayland 崩溃（tauri#8541）强制 `GDK_BACKEND=x11`，正好踩进坏通路（表现：整窗交互正常但光标消失，连进程内原生 GTK 对话框也丢光标）。`run()` 在「APPDIR 存在 + WAYLAND_DISPLAY 存在 + GDK_BACKEND==x11」时撤掉该强制值回 Wayland 原生；纯 X11 会话保持 x11。若将来在旧 WebKitGTK 环境遇 Wayland 崩溃，重新评估这段撤销逻辑。
