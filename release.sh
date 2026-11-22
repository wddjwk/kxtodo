#!/usr/bin/env bash
# ./release.sh 构建 Linux 制品（AppImage + CLI）；版本号取 git，构建期经 --config 注入，不写入任何文件。
#
# 产物（固定命名，不带版本号；官方 Tauri bundler 标准流程）：
#   release/KXToDo.AppImage   —— npx tauri build --bundles appimage（GUI，bundler 产物校验版本后改固定名）
#   release/kxtodo-cli        —— cargo build --release -p kxtodo-cli（CLI 裸二进制）
#
# 前置（官方 v2.tauri.app Linux 依赖清单，release.sh 会用 pkg-config 门控）：
#   sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
# AppImage 运行需要 FUSE；首次构建 tauri CLI 会自动下载 linuxdeploy，需要网络。
# Linux 应用内更新：GUI 下载新的 KXToDo.AppImage + kxtodo-cli 到 ~/.local/share/kxtodo/bin/ 替换后重启。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# ---------- 工具门控 ----------
for cmd in git cargo npm npx pkg-config file; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    printf '错误：PATH 里缺少 %s，请先安装\n' "$cmd" >&2
    exit 1
  fi
done

# ---------- 版本号解析 ----------
# 版本号唯一来源是 git，解析语义与 src-tauri/build.rs 完全一致，优先级：
# HEAD 上的精确 v* tag → 近 30 条 commit 里第一条 “vX.Y.Z” 主题 → 最近的祖先 v* tag，
# 都没有则回退 0.0.0-dev。
is_dotted_numeric() {
  local candidate="$1" part
  local -a parts
  [[ -n "$candidate" ]] || return 1
  IFS='.' read -ra parts <<< "$candidate"
  [[ "${#parts[@]}" -eq 3 ]] || return 1
  for part in "${parts[@]}"; do
    [[ "$part" =~ ^[0-9]+$ ]] || return 1
  done
}

resolve_version() {
  local tag candidate subject token
  # 精确 tag 必须排在祖先 tag 之前：describe --abbrev=0 在未打 tag 的 commit 上返回的是
  # 上一个 tag，那样主题里写的版本号永远轮不到，制品会静默带旧版本号。
  tag="$(git describe --tags --match 'v*' --exact-match 2>/dev/null || true)"
  candidate="${tag#v}"
  if [[ -n "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  while IFS= read -r subject; do
    # 只取第一个空白前的 token：整段主题拿去校验会被后面的说明文字带崩
    read -r token _ <<<"$subject"
    [[ "$token" == v* ]] || continue
    candidate="${token#v}"
    if is_dotted_numeric "$candidate"; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done < <(git log -30 --pretty=%s 2>/dev/null || true)
  tag="$(git describe --tags --match 'v*' --abbrev=0 2>/dev/null || true)"
  candidate="${tag#v}"
  if [[ -n "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  return 1
}

VERSION="$(resolve_version || true)"
if [[ -z "$VERSION" ]]; then
  VERSION="0.0.0-dev"
  printf '警告：git 里解析不到版本号（无 v* tag，近 30 条 commit 也无 vX.Y.Z 主题），回退 %s\n' "$VERSION" >&2
  printf '警告：build.rs 同源解析——正式发布必须先 commit 再在 HEAD 上打 tag，否则产物版本错且不报错\n' >&2
fi

# ---------- pkg-config 系统库门控（官方 Tauri Linux 前置） ----------
# appindicator 接受 ayatana 或普通版：tray-icon 运行期 dlopen 任一即可。
have_appindicator=false
if pkg-config --exists ayatana-appindicator3-0.1 || pkg-config --exists appindicator3-0.1; then
  have_appindicator=true
fi
missing_libs=()
for lib in webkit2gtk-4.1 gtk+-3.0 librsvg-2.0 openssl; do
  pkg-config --exists "$lib" || missing_libs+=("$lib")
done

# libxdo 例外：Debian/Ubuntu 的 libxdo-dev 只装 /usr/include/xdo.h，不带 libxdo.pc，
# pkg-config 查不到属正常（tauri 侧是 -lxdo 直连），改查头文件 + 共享库；有 .pc 时优先用 pkg-config。
have_libxdo=false
if pkg-config --exists libxdo; then
  have_libxdo=true
else
  shopt -s nullglob
  libxdo_headers=(/usr/include/xdo.h /usr/local/include/xdo.h)
  libxdo_libs=(/usr/lib/libxdo.so* /usr/lib/*/libxdo.so* /usr/local/lib/libxdo.so* /usr/local/lib/*/libxdo.so*)
  shopt -u nullglob
  if [[ ${#libxdo_headers[@]} -gt 0 && ${#libxdo_libs[@]} -gt 0 ]]; then
    have_libxdo=true
  fi
fi
[[ "$have_libxdo" == true ]] || missing_libs+=("libxdo")

if [[ ${#missing_libs[@]} -gt 0 || "$have_appindicator" != true ]]; then
  if [[ ${#missing_libs[@]} -gt 0 ]]; then
    printf '错误：缺少系统库：%s\n' "${missing_libs[*]}" >&2
  fi
  if [[ "$have_appindicator" != true ]]; then
    printf '错误：pkg-config 缺少 appindicator（ayatana-appindicator3-0.1 或 appindicator3-0.1 任一）\n' >&2
  fi
  printf '  官方依赖清单：sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev\n' >&2
  exit 1
fi

printf '==> 版本：%s\n' "$VERSION"

# AppImage 的 glibc 基底告警：linuxdeploy 把构建机的 libglib/webkit2gtk/javascriptcore 等
# 原样打包，它们链接构建时的 glibc——在比构建机老的发行版上会报 GLIBC_2.xx not found
# （v0.9.2 事故：CI ubuntu-latest 漂移）。发布产物一律由 pin 了 ubuntu-22.04 的 CI 出；
# 本地构建只供本机/更新系统使用，这里照实告警而不是静默产出「看起来能分发」的包。
build_glibc="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')"
if [[ -n "$build_glibc" ]] && [[ "$(printf '%s\n2.35\n' "$build_glibc" | sort -V | head -n 1)" != "2.35" ]]; then
  printf '警告：构建机 glibc %s 高于发布基底 2.35（Ubuntu 22.04），产出的 AppImage 只能在 glibc >= %s 的系统运行；对外分发请走 CI（release.yml 已 pin ubuntu-22.04）\n' "$build_glibc" "$build_glibc" >&2
fi

# ---------- npm 依赖（与 package.ps1 一致：无条件执行，有 lockfile 时很快） ----------
if ! npm install; then
  printf '错误：npm install 失败\n' >&2
  exit 1
fi

# 图标再生成尽力而为：缺 python3/Pillow 时沿用仓库已提交的图标（与 package.ps1 一致）。
if command -v python3 >/dev/null 2>&1; then
  python3 scripts/make-icon.py || printf '警告：图标再生成失败，沿用已提交图标\n' >&2
else
  printf '警告：未找到 python3，跳过图标再生成\n' >&2
fi

# 与 package.ps1 共用同一 target 目录，缓存互通。
export CARGO_TARGET_DIR="$SCRIPT_DIR/src-tauri/target"

# ---------- GUI：官方 Tauri bundler 打 AppImage ----------
# 仓库文件不写版本号（tauri.conf.json 无 version 字段），版本经 --config 内联 JSON 在构建期合并注入。
# bundle 由平台覆盖 src-tauri/tauri.linux.conf.json 开启（active=true / targets=["appimage"]，
# tauri CLI 按宿主平台自动合并，无需显式传入）；基础 tauri.conf.json 的 bundle.active 是 false，
# 覆盖缺失时打不出 AppImage——提前告警，最终仍由后面的产物检查兜底。
# tauri build 的 beforeBuildCommand 会跑前端构建，不要另行 npm run build（标准流程）。
# 首次构建 tauri CLI 会自动下载 linuxdeploy，需要网络。
# 构建输出直通终端，不要接 tail/head 管道——管道会把退出码掩成 0。
if [[ ! -f src-tauri/tauri.linux.conf.json ]]; then
  printf '警告：未找到 src-tauri/tauri.linux.conf.json（应由它开启 bundle 并把 targets 限定为 appimage）\n' >&2
fi

printf '==> 构建 Linux GUI（AppImage）...\n'
if ! npx tauri build --bundles appimage --config "{\"version\":\"$VERSION\"}"; then
  printf '错误：AppImage 构建失败\n' >&2
  exit 1
fi

# 定位产物：bundle 目录下 glob *.AppImage（不硬编码架构），多个取最新。
# 目录不存在时 find 会非零退出，加 || true 让下面的空值检查打印明确错误（否则 set -e 静默中止）。
appimage_bundle_dir="$CARGO_TARGET_DIR/release/bundle/appimage"
appimage_src="$(find "$appimage_bundle_dir" -maxdepth 1 -name '*.AppImage' -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -n 1 | cut -d' ' -f2- || true)"
if [[ -z "$appimage_src" ]]; then
  printf '错误：%s 下找不到 *.AppImage 产物\n' "$appimage_bundle_dir" >&2
  exit 1
fi
appimage_name="$(basename "$appimage_src")"
# 官方命名约定 productName_version_arch：版本号必须是 git 解析出的那个，否则说明 --config 注入没生效。
if [[ "$appimage_name" != *"$VERSION"* ]]; then
  printf '错误：AppImage 产物名 %s 不含版本 %s——--config 版本注入未生效，拒绝发布错版产物\n' "$appimage_name" "$VERSION" >&2
  exit 1
fi

# ---------- AppImage 瘦身：剔除自带的 GLib 家族，重打包 ----------
# v0.9.3 事故：linuxdeploy-plugin-gtk 把构建机（22.04）的旧 GLib（2.72）打进包里，
# AppRun 的 LD_LIBRARY_PATH 让它优先于系统 GLib 加载；Ubuntu 24.04 的系统托盘库
# libayatana-ido3 按 GLib 2.80 编译，找 g_once_init_leave_pointer / g_task_set_static_name
# 找不到 → libappindicator dlopen 失败 → 整进程 panic（gvfs/canberra 的 undefined symbol
# 噪音同根因）。GLib 是各发行版必装的基础库且 ABI 向后兼容：构建机 22.04 的 webkit/gtk
# 链接的符号在 22.04+ 的系统 GLib 里都有，剔除自带副本让全进程统一用系统 GLib 即可。
# 同理由剔 libnghttp2：24.04 系统 libcurl-gnutls 要新 nghttp2 符号，包内旧 nghttp2 优先
# 会让 gio 的 libproxy/gnutls 模块加载失败（非致命，但属同类「自带旧库压系统新库」）。
# tauri bundler 没有排除库的配置口，标准做法是解包 → 删库 → chmod -R a+rX → appimagetool 重打包。
printf '==> AppImage 瘦身（剔除自带 GLib 家族，改用系统 GLib）...\n'
appimage_workdir="$(mktemp -d)"
trap 'rm -rf "$appimage_workdir"' EXIT
if ! (cd "$appimage_workdir" && "$appimage_src" --appimage-extract >/dev/null); then
  printf '错误：AppImage 解包失败\n' >&2
  exit 1
fi
appdir="$appimage_workdir/squashfs-root"
removed_libs="$(find "$appdir" \( -name 'libglib-2.0.so*' -o -name 'libgobject-2.0.so*' \
  -o -name 'libgio-2.0.so*' -o -name 'libgmodule-2.0.so*' -o -name 'libgthread-2.0.so*' \
  -o -name 'libnghttp2.so*' \) -print -delete)"
if [[ -z "$removed_libs" ]]; then
  printf '错误：AppDir 里找不到自带的 GLib 家族库——打包链可能变了，拒绝产出未瘦身的包\n' >&2
  exit 1
fi
printf '%s\n' "$removed_libs" | sed "s|$appdir/|  剔除 |"
# 自带的 gio/modules（gvfs 等）是按旧 GLib 编译的，一并剔除；AppRun 里指向它们的
# GIO_MODULE_DIR 导出删掉，回落系统默认模块目录（系统 gio 与系统模块版本天然匹配）。
find "$appdir" -type d -path '*gio/modules' -prune -exec rm -rf {} + 2>/dev/null || true
if [[ -f "$appdir/AppRun" ]]; then
  sed -i '/GIO_MODULE_DIR/d' "$appdir/AppRun"
fi
# 解包出来的目录可能带 0700（CI umask），appimagetool 会因「别的用户读/执行不了」拒包；
# 按它自己的建议统一补读与目录执行位（a+rX 不给普通文件加执行位，AppRun 原有 +x 保留）。
chmod -R a+rX "$appdir"

# appimagetool 缓存复用；GitHub 直连失败回落 ghfast.top 镜像（与 server 自升级同策略）。
appimagetool_cache="${XDG_CACHE_HOME:-$HOME/.cache}/kxtodo-build"
mkdir -p "$appimagetool_cache"
appimagetool="$appimagetool_cache/appimagetool-x86_64.AppImage"
if [[ ! -x "$appimagetool" ]]; then
  appimagetool_url="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
  if ! curl -fL --retry 2 --connect-timeout 20 -o "$appimagetool" "$appimagetool_url"; then
    printf '警告：GitHub 直连下载 appimagetool 失败，尝试 ghfast.top 镜像\n' >&2
    if ! curl -fL --retry 2 --connect-timeout 20 -o "$appimagetool" "https://ghfast.top/$appimagetool_url"; then
      printf '错误：appimagetool 下载失败（重打包必需）\n' >&2
      exit 1
    fi
  fi
  chmod +x "$appimagetool"
fi
if ! ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" "$appdir" "$appimage_workdir/repacked.AppImage" >/dev/null; then
  printf '错误：appimagetool 重打包失败\n' >&2
  exit 1
fi
cp -f "$appimage_workdir/repacked.AppImage" "$appimage_src"
chmod +x "$appimage_src"
printf '==> 瘦身完成：%s（%s 字节）\n' "$appimage_name" "$(stat -c %s "$appimage_src")"

# ---------- CLI：裸二进制 ----------
# workspace default-members 不含 cli，必须显式 -p。
printf '==> 构建 Linux CLI（kxtodo-cli）...\n'
if ! cargo build --release --manifest-path src-tauri/Cargo.toml -p kxtodo-cli; then
  printf '错误：CLI 构建失败\n' >&2
  exit 1
fi

# ---------- 同步服务端（unix 部署） ----------
printf '==> 构建 Linux 同步服务端（kxtodo-server）...\n'
if ! cargo build --release --manifest-path src-tauri/Cargo.toml -p kxtodo-server; then
  printf '错误：kxtodo-server 构建失败\n' >&2
  exit 1
fi

# ---------- 收集产物（固定命名，不带版本号） ----------
mkdir -p release
appimage_out="release/KXToDo.AppImage"
cli_out="release/kxtodo-cli"
server_out="release/kxtodo-server"
cp -f "$appimage_src" "$appimage_out"
cp -f "$CARGO_TARGET_DIR/release/kxtodo-cli" "$cli_out"
cp -f "$CARGO_TARGET_DIR/release/kxtodo-server" "$server_out"
chmod +x "$appimage_out" "$cli_out" "$server_out"

# ---------- 双 Ubuntu 运行验证（有 docker 就跑，CI runner 自带 docker） ----------
# 打包成功 ≠ 跑得起来：v0.9.2/v0.9.3 两版都是构建全绿、用户机器上 panic。
# 在 ubuntu:22.04 与 ubuntu:24.04 容器里真实启动 AppImage（xvfb + dbus），
# 断言进程活过冒烟窗口且无 panic / 托盘库加载失败 / undefined symbol。
if command -v docker >/dev/null 2>&1; then
  if ! bash "$SCRIPT_DIR/scripts/verify-appimage.sh" "$appimage_out"; then
    printf '错误：AppImage 运行验证失败，拒绝发布\n' >&2
    exit 1
  fi
else
  printf '警告：未找到 docker，跳过 AppImage 双 Ubuntu 运行验证（CI 的 release.sh 会自动执行该步骤）\n' >&2
fi

printf '==> 构建完成（版本 %s）：\n' "$VERSION"
printf '  %s/%s（%s 字节）\n' "$SCRIPT_DIR" "$appimage_out" "$(stat -c %s "$appimage_out")"
printf '  %s/%s（%s 字节）\n' "$SCRIPT_DIR" "$cli_out" "$(stat -c %s "$cli_out")"
printf '  %s/%s（%s 字节）\n' "$SCRIPT_DIR" "$server_out" "$(stat -c %s "$server_out")"
