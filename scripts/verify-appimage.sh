#!/usr/bin/env bash
# scripts/verify-appimage.sh <AppImage 路径>
# 在 ubuntu:22.04 与 ubuntu:24.04 容器里真实启动 AppImage（xvfb + dbus 会话），
# 断言进程活过 30 秒冒烟窗口（timeout 退出码 124 = 一直在跑），且日志里没有
# panic / 托盘库加载失败 / GLib g_* 符号缺失。需要 docker；release.sh 在有 docker
# 时自动调用（CI runner 自带），本地无 docker 时由 release.sh 告警跳过。
#
# 背景：v0.9.2（构建基底漂移，GLIBC_2.4x not found）与 v0.9.3（自带旧 GLib 与
# 系统托盘库符号不匹配，libappindicator panic）都是「构建全绿、用户机器跑不起来」。
# 打包成功不等于能运行，发布前必须在目标发行版上真的跑一次。
# 注意：容器里 WebKit 的渲染管线不保证完整（冒烟只验证进程存活与动态库链路），
# 图形观感仍须真机手测。
set -euo pipefail

if [[ $# -ne 1 ]]; then
  printf '用法：%s <AppImage 路径>\n' "$0" >&2
  exit 1
fi
appimage="$(realpath "$1")"
if [[ ! -f "$appimage" ]]; then
  printf '错误：找不到 AppImage：%s\n' "$appimage" >&2
  exit 1
fi
if ! command -v docker >/dev/null 2>&1; then
  printf '错误：需要 docker 才能运行 AppImage 验证\n' >&2
  exit 1
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
cp -f "$appimage" "$workdir/KXToDo.AppImage"
chmod +x "$workdir/KXToDo.AppImage"

# 容器内冒烟脚本：装运行时依赖（不装 -dev）→ xvfb + dbus 下启动 → 收尸检日志。
# --appimage-extract-and-run 免 FUSE（容器默认没有 /dev/fuse）。
# seccomp=unconfined + shm-size 给 WebKit 的 bubblewrap 沙箱与共享内存留活路。
# 致命判据只认三类：Rust panic、托盘库 dlopen 失败、GLib g_* 符号缺失（v0.9.3 事故根因）。
# 其它库的 undefined symbol / gio 模块加载失败 / DRI3 / AT-SPI 属容器或次要库噪音，
# 进程仍存活即不算失败（宽 grep 会把非致命噪音误判成红，v0.9.4 实测）。
smoke='
set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq --no-install-recommends \
  libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 \
  xvfb dbus-x11 fonts-dejavu-core ca-certificates >/dev/null
cp /w/KXToDo.AppImage /tmp/KXToDo.AppImage
chmod +x /tmp/KXToDo.AppImage
export HOME=/tmp/kxhome
mkdir -p "$HOME"
set +e
timeout 30s xvfb-run -a dbus-run-session -- /tmp/KXToDo.AppImage --appimage-extract-and-run >/tmp/kx.log 2>&1
code=$?
set -e
cat /tmp/kx.log
if grep -Eq "panicked|Failed to load ayatana|undefined symbol: g_" /tmp/kx.log; then
  echo "错误：日志含 panic / 托盘库加载失败 / GLib 符号缺失" >&2
  exit 1
fi
if [ "$code" -ne 124 ]; then
  echo "错误：AppImage 提前退出（退出码 $code，预期 124 = 活过 30 秒冒烟窗口）" >&2
  exit 1
fi
echo "冒烟通过：进程存活且无致命错误"
'

for version in 22.04 24.04; do
  printf '==> 在 ubuntu:%s 容器里运行 AppImage...\n' "$version"
  if ! docker run --rm --shm-size=512m --security-opt seccomp=unconfined \
    -v "$workdir:/w:ro" "ubuntu:$version" bash -c "$smoke"; then
    printf '错误：AppImage 在 ubuntu:%s 上运行验证失败\n' "$version" >&2
    exit 1
  fi
  printf '==> ubuntu:%s 验证通过\n' "$version"
done
printf '==> AppImage 双 Ubuntu 运行验证全部通过\n'