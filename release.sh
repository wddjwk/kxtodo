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
# 版本号唯一来源是 git，解析语义与 src-tauri/build.rs 完全一致：
# v* tag 优先（去 v 前缀，不校验格式），其次近 30 条 commit 里第一条
# “v + 点分纯数字”的主题，都没有则回退 0.0.0-dev。
is_dotted_numeric() {
  local candidate="$1" part
  local -a parts
  [[ -n "$candidate" ]] || return 1
  IFS='.' read -ra parts <<< "$candidate"
  for part in "${parts[@]}"; do
    [[ "$part" =~ ^[0-9]+$ ]] || return 1
  done
}

resolve_version() {
  local tag candidate subject
  tag="$(git describe --tags --match 'v*' --abbrev=0 2>/dev/null || true)"
  candidate="${tag#v}"
  if [[ -n "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  while IFS= read -r subject; do
    subject="${subject#"${subject%%[![:space:]]*}"}"
    subject="${subject%"${subject##*[![:space:]]}"}"
    [[ "$subject" == v* ]] || continue
    candidate="${subject#v}"
    if is_dotted_numeric "$candidate"; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done < <(git log -30 --pretty=%s 2>/dev/null || true)
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

# ---------- CLI：裸二进制 ----------
# workspace default-members 不含 cli，必须显式 -p。
printf '==> 构建 Linux CLI（kxtodo-cli）...\n'
if ! cargo build --release --manifest-path src-tauri/Cargo.toml -p kxtodo-cli; then
  printf '错误：CLI 构建失败\n' >&2
  exit 1
fi

# ---------- 收集产物（固定命名，不带版本号） ----------
mkdir -p release
appimage_out="release/KXToDo.AppImage"
cli_out="release/kxtodo-cli"
cp -f "$appimage_src" "$appimage_out"
cp -f "$CARGO_TARGET_DIR/release/kxtodo-cli" "$cli_out"
chmod +x "$appimage_out" "$cli_out"

printf '==> 构建完成（版本 %s）：\n' "$VERSION"
printf '  %s/%s（%s 字节）\n' "$SCRIPT_DIR" "$appimage_out" "$(stat -c %s "$appimage_out")"
printf '  %s/%s（%s 字节）\n' "$SCRIPT_DIR" "$cli_out" "$(stat -c %s "$cli_out")"
