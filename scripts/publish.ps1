# KXToDo 一键发布：git 取版本 → 本地构建三产物（GUI exe + CLI exe + Android APK）→ gh release 上传；
# release\ 下存在同版本 Linux 双产物（AppImage + CLI -gnu，release.sh 在 Linux 上构建）时一并上传，缺失只警告。
# 用法：
#   .\scripts\publish.ps1            # 构建 + 发布当前 git 版本
#   .\scripts\publish.ps1 -DryRun    # 只打印将要执行的步骤
#
# 前置：gh 已登录（gh auth login）；git 工作区已提交（版本号来自最近的 v* tag
# 或最近一条 vX.Y.Z 开头的 commit message）；构建 APK 还需 ANDROID_HOME/NDK 与 JDK。
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
# APK 固定命名（覆盖安装不留历史包），靠 sidecar 记录构建版本防旧产物误复用
$apk = Join-Path $root "release\KXToDo.apk"
$apkVersionFile = Join-Path $root "release\KXToDo.apk.version"
$apkFresh = (Test-Path -LiteralPath $apk) -and (Test-Path -LiteralPath $apkVersionFile) -and
  ((Get-Content -LiteralPath $apkVersionFile -Raw).Trim() -eq $version)

# Linux 双产物可选：在 Linux 上跑过 release.sh 并把产物放进 release\ 才会存在
# （AppImage 是 tauri bundler 标准命名 productName_version_arch）。
# 缺失只警告一次，不参与三产物的复用/重建判断——publish 通常在没有 Linux 产物的 Windows 上跑。
$linuxAppImage = Join-Path $root "release\KXToDo_${version}_amd64.AppImage"
$linuxCli = Join-Path $root "release\KXToDo-CLI-$version-gnu"
$linuxArtifacts = @()
if ((Test-Path -LiteralPath $linuxAppImage) -and (Test-Path -LiteralPath $linuxCli)) {
  $linuxArtifacts = @($linuxAppImage, $linuxCli)
} else {
  Write-Warning "未找到 Linux 产物（$linuxAppImage / $linuxCli），本次发布不含 Linux 制品"
}

Write-Host "==> 版本：$tag" -ForegroundColor Cyan

# 未提交改动会让构建产物与 git 版本对不上，直接拒绝。
$dirty = git status --porcelain
if ($dirty -and -not $DryRun) {
  throw "工作区有未提交改动，请先提交（版本号以 git 为准）"
}

if ($DryRun) {
  Write-Host "[DryRun] 将构建 $tag 并发布三产物："
  Write-Host "[DryRun]   $guiExe"
  Write-Host "[DryRun]   $cliExe"
  Write-Host "[DryRun]   $apk"
  if ($linuxArtifacts.Count -gt 0) {
    Write-Host "[DryRun] 另将上传 Linux 双产物："
    foreach ($linuxArtifact in $linuxArtifacts) {
      Write-Host "[DryRun]   $linuxArtifact"
    }
  } else {
    Write-Host "[DryRun] Linux 产物缺失，本次发布不含 Linux 制品"
  }
  return
}

# 三产物都已构建过（exe 同名 + APK sidecar 版本匹配）则直接复用，否则只补缺失的部分
# （release.ps1 内部用同一个 git 版本逻辑，产物名与此处一致）
if ((Test-Path -LiteralPath $guiExe) -and (Test-Path -LiteralPath $cliExe) -and $apkFresh) {
  Write-Host "==> 复用已构建产物：$guiExe + $cliExe + $apk（删除后重跑可强制重建）" -ForegroundColor Yellow
} else {
  if (-not (Test-Path -LiteralPath $guiExe) -or -not (Test-Path -LiteralPath $cliExe)) {
    & (Join-Path $root "release.ps1") windows
    if ($LASTEXITCODE -ne 0) { throw "Windows 构建失败" }
  }
  if (-not $apkFresh) {
    & (Join-Path $root "release.ps1") android
    if ($LASTEXITCODE -ne 0) { throw "Android 构建失败" }
  }
}

foreach ($artifact in @($guiExe, $cliExe, $apk)) {
  if (-not (Test-Path -LiteralPath $artifact)) {
    throw "构建产物缺失：$artifact"
  }
}

# tag 不存在就先建（指向 HEAD）；分支与 tag 一律推送——gh release create
# 要求远端已存在该 tag，只推 tag 不推分支会让远端 release 缺少对应提交。
git rev-parse --verify --quiet "refs/tags/$tag" >$null 2>&1
if ($LASTEXITCODE -ne 0) {
  git tag $tag
}
git push origin HEAD
if ($LASTEXITCODE -ne 0) { throw "推送当前分支到 origin 失败" }
git push origin $tag
if ($LASTEXITCODE -ne 0) { throw "推送 tag $tag 到 origin 失败" }

# release 已存在则只补传产物，不存在则创建。
# PS 5.1 下 gh 写 stderr + ErrorActionPreference=Stop 会变终止性 NativeCommandError，局部降级。
$saved = $ErrorActionPreference
$ErrorActionPreference = "Continue"
gh release view $tag *> $null
$releaseExists = $LASTEXITCODE -eq 0
if ($releaseExists) {
  gh release upload $tag $guiExe $cliExe $apk @linuxArtifacts --clobber
} else {
  gh release create $tag $guiExe $cliExe $apk @linuxArtifacts --title $tag --generate-notes
}
$publishOk = $LASTEXITCODE -eq 0
$ErrorActionPreference = $saved
if (-not $publishOk) { throw "gh release 失败" }

$linuxNote = if ($linuxArtifacts.Count -gt 0) { " + Linux 双产物（AppImage + CLI）" } else { "" }
Write-Host "已发布 $tag ：GUI + CLI + APK 三产物$linuxNote" -ForegroundColor Green
