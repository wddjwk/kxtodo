# KXToDo 一键发布：git 取版本 → 本地构建双产物 → gh release 上传。
# 用法：
#   .\scripts\publish.ps1            # 构建 + 发布当前 git 版本
#   .\scripts\publish.ps1 -DryRun    # 只打印将要执行的步骤
#
# 前置：gh 已登录（gh auth login）；git 工作区已提交（版本号来自最近的 v* tag
# 或最近一条 vX.Y.Z 开头的 commit message）。
param(
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $scriptPath "..")
Set-Location $root

# 版本号：最近的 v* tag 优先，其次最近一条 vX.Y.Z 开头的 commit message。
function Get-GitVersion {
  $saved = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  $tag = $null
  $subjects = @()
  try {
    $tag = git describe --tags --match "v*" --abbrev=0 2>$null
    $subjects = @(git log -30 --pretty=%s 2>$null)
  } catch {
  }
  $ErrorActionPreference = $saved
  if ($tag) {
    return $tag.Trim().TrimStart("v")
  }
  foreach ($subject in $subjects) {
    if ($subject -match '^v(\d+\.\d+\.\d+)') {
      return $Matches[1]
    }
  }
  throw "git 里找不到版本号：请打一个 vX.Y.Z 的 tag，或提交一条以 vX.Y.Z 开头的 commit"
}

$version = Get-GitVersion
$tag = "v$version"
$guiExe = Join-Path $root "release\KXToDo-$version.exe"
$cliExe = Join-Path $root "release\KXToDo-CLI-$version.exe"

Write-Host "==> 版本：$tag" -ForegroundColor Cyan

# 未提交改动会让构建产物与 git 版本对不上，直接拒绝。
$dirty = git status --porcelain
if ($dirty -and -not $DryRun) {
  throw "工作区有未提交改动，请先提交（版本号以 git 为准）"
}

if ($DryRun) {
  Write-Host "[DryRun] 将构建 $tag 并发布 $guiExe + $cliExe"
  return
}

# 构建（release.ps1 内部用同一个 git 版本逻辑，产物名与此处一致）
& (Join-Path $root "release.ps1") windows
if ($LASTEXITCODE -ne 0) { throw "构建失败" }

foreach ($artifact in @($guiExe, $cliExe)) {
  if (-not (Test-Path -LiteralPath $artifact)) {
    throw "构建产物缺失：$artifact"
  }
}

# tag 不存在就先建（指向 HEAD），已存在则复用
git rev-parse --verify --quiet "refs/tags/$tag" >$null 2>&1
if ($LASTEXITCODE -ne 0) {
  git tag $tag
  git push origin $tag
}

# release 已存在则只补传产物，不存在则创建
gh release view $tag >$null 2>&1
if ($LASTEXITCODE -eq 0) {
  gh release upload $tag $guiExe $cliExe --clobber
} else {
  gh release create $tag $guiExe $cliExe --title $tag --generate-notes
}
if ($LASTEXITCODE -ne 0) { throw "gh release 失败" }

Write-Host "已发布 $tag ：GUI + CLI 双产物" -ForegroundColor Green
