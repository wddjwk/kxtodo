#!/usr/bin/env bash
# 由 Windows 侧 scripts/package.ps1 经 wsl.exe 调用：把 WSL 原生克隆同步到 Windows 仓库的
# 当前 HEAD（含 tag），在原生克隆里跑 release.sh 构建 Linux 制品，再把固定名产物拷回 Windows 仓库的 release/。
#
# 为什么用原生克隆而不是 /mnt/d：AppImage 打包（linuxdeploy 建 AppDir、大量符号链接/小文件 I/O）
# 在 ext4 上更快更稳；/mnt/d（9p/drvfs）虽支持符号链接但慢且未经完整验证。
#
# 用法：wsl-linux-build.sh <win_repo_wsl_path> <native_repo_path> <git_sha>
#   win_repo_wsl_path  Windows 仓库的 WSL 路径（如 /mnt/d/projects/kxtodo），既作 fetch 源也作产物回拷目标
#   native_repo_path   WSL 原生克隆路径（支持 ~ 前缀；默认由调用方给 ~/projects/kxtodo）
#   git_sha            Windows 仓库 HEAD 的完整 SHA，原生克隆 checkout 到它（tag 一并 fetch 以保证版本解析正确）
#
# 退出码：0 成功；非 0 一律被 package.ps1 当作“告警跳过”（环境未就绪或构建失败），不终止其它平台构建。
set -euo pipefail

WIN_REPO="${1:?缺少 Windows 仓库 WSL 路径}"
NATIVE_RAW="${2:?缺少原生克隆路径}"
SHA="${3:?缺少 git SHA}"

# 展开 ~ 前缀（作为参数传入时 bash 不会自动展开）。
NATIVE="${NATIVE_RAW/#\~/$HOME}"

if [[ ! -d "$NATIVE/.git" ]]; then
  printf '跳过 Linux 构建：未找到 WSL 原生克隆 %s（设置 KXTODO_WSL_REPO 指定路径，或先 git clone）\n' "$NATIVE" >&2
  exit 3
fi
if [[ ! -d "$WIN_REPO" ]]; then
  printf '跳过 Linux 构建：Windows 仓库的 WSL 路径不存在 %s\n' "$WIN_REPO" >&2
  exit 3
fi

cd "$NATIVE"

# 原生克隆只是构建从属：跟踪文件必须干净才同步，避免 checkout -f 冲掉用户在原生克隆里的改动。
# 未跟踪文件（-uno 忽略）不影响构建，保留不动。
if [[ -n "$(git status --porcelain -uno)" ]]; then
  printf '跳过 Linux 构建：原生克隆 %s 有未提交的跟踪改动，请先在该克隆里 commit/stash/restore 后重试\n' "$NATIVE" >&2
  exit 4
fi

printf '==> 同步原生克隆到 Windows HEAD %s（fetch heads+tags from %s）...\n' "${SHA:0:12}" "$WIN_REPO"
git fetch --force "$WIN_REPO" "+refs/heads/*:refs/remotes/win/*" "+refs/tags/*:refs/tags/*"
git checkout --force "$SHA"

printf '==> 在原生克隆里构建 Linux 制品（release.sh）...\n'
./release.sh

printf '==> 回拷 Linux 制品到 %s/release ...\n' "$WIN_REPO"
mkdir -p "$WIN_REPO/release"
cp -f release/KXToDo.AppImage "$WIN_REPO/release/KXToDo.AppImage"
cp -f release/kxtodo-cli "$WIN_REPO/release/kxtodo-cli"
chmod +x "$WIN_REPO/release/KXToDo.AppImage" "$WIN_REPO/release/kxtodo-cli"

printf '==> Linux 制品就绪：KXToDo.AppImage + kxtodo-cli\n'
