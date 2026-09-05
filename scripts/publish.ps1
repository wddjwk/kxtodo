# KXToDo 一键发布：git 取版本 → 本地构建三平台五产物（Windows GUI/CLI + Android APK + Linux AppImage/CLI）→ gh release 上传。
# 五个固定名产物（不带版本号）：
#   release\KXToDo.exe        Windows GUI
#   release\kxtodo-cli.exe    Windows CLI
#   release\KXToDo.apk        Android
#   release\KXToDo.AppImage   Linux GUI（经 WSL 原生克隆构建）
#   release\kxtodo-cli        Linux CLI
# 用法：
#   .\scripts\publish.ps1            # 构建 + 发布当前 git 版本
#   .\scripts\publish.ps1 -DryRun    # 只打印将要执行的步骤
#
# 版本号唯一来源是 git，且构建期由 build.rs/release.sh 同源解析（最近的 v* tag 优先）。
# 为避免“HEAD 未打 tag → describe 回落到旧 tag → 产物版本错”的坑：本脚本先确保 HEAD 带有
# 与提交主题一致的 tag（HEAD 已有精确 tag 用之；否则取提交主题的 vX.Y.Z 当场打 tag），再构建。
# 前置：gh 已登录；git 工作区已提交；某一平台环境未就绪（缺 MSVC/ANDROID_HOME/WSL/原生克隆）
# 时该平台告警跳过，发布其余产物。
param(
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $scriptPath "..")
Set-Location $root

# git 非零退出码在 pwsh 7 + ErrorActionPreference=Stop 下会变终止性错误；探测类调用局部降级。
function Invoke-GitQuiet {
  param([Parameter(ValueFromRemainingArguments = $true)][string[]]$GitArgs)
  $saved = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try {
    $out = & git @GitArgs 2>$null
    return [pscustomobject]@{ Exit = $LASTEXITCODE; Out = $out }
  } finally {
    $ErrorActionPreference = $saved
  }
}

# 解析发布版本并确保 HEAD 带对应 tag（构建期 build.rs/release.sh 以最近的 v* tag 为准）。
function Resolve-ReleaseVersion {
  # 1) HEAD 上已有精确 tag → 直接用。
  $exact = Invoke-GitQuiet describe --exact-match --tags HEAD
  if ($exact.Exit -eq 0 -and $exact.Out) {
    $t = ($exact.Out | Select-Object -First 1).ToString().Trim()
    if ($t -match '^v?(\d+\.\d+\.\d+)$') { return $Matches[1] }
  }
  # 2) HEAD 提交主题以 vX.Y.Z 开头 → 用之（真正的打 tag 放到 DryRun 之后、构建之前，避免 DryRun 产生副作用）。
  $subject = (Invoke-GitQuiet log -1 --pretty=%s).Out
  if ($subject) {
    $s = $subject.ToString().Trim()
    if ($s -match '^v(\d+\.\d+\.\d+)') {
      return $Matches[1]
    }
  }
  # 3) 回落到最近可达 tag（并告警 HEAD 与 tag 可能不一致）。
  $nearest = Invoke-GitQuiet describe --tags --match "v*" --abbrev=0
  if ($nearest.Exit -eq 0 -and $nearest.Out) {
    $t = $nearest.Out.ToString().Trim().TrimStart("v")
    Write-Warning "HEAD 无 vX.Y.Z 提交主题，回落最近 tag v$t（请确认这正是你要发布的版本）"
    return $t
  }
  throw "git 里找不到版本号：请在 HEAD 打一个 vX.Y.Z 的 tag，或提交一条以 vX.Y.Z 开头的 commit"
}

$version = Resolve-ReleaseVersion
$tag = "v$version"

# 五个固定名产物。
$artifactNames = @("KXToDo.exe", "kxtodo-cli.exe", "KXToDo.apk", "KXToDo.AppImage", "kxtodo-cli")
$artifactPaths = $artifactNames | ForEach-Object { Join-Path $root "release\$_" }

Write-Host "==> 版本：$tag" -ForegroundColor Cyan

# 未提交改动会让构建产物与 git 版本对不上（且原生克隆按 HEAD SHA 同步），直接拒绝。
$dirty = (Invoke-GitQuiet status --porcelain).Out
if ($dirty -and -not $DryRun) {
  throw "工作区有未提交改动，请先提交（版本号与 Linux 原生克隆同步均以 git HEAD 为准）"
}

if ($DryRun) {
  Write-Host "[DryRun] 将构建 $tag 并发布五个固定名产物（环境未就绪的平台告警跳过）："
  foreach ($p in $artifactPaths) { Write-Host "[DryRun]   $p" }
  return
}

# 构建前确保 HEAD 带 v$version 的 tag：build.rs / release.sh 以最近的 v* tag 解析版本，
# Linux 原生克隆也按 fetch 到的 tag 构建。HEAD 无精确 tag 时当场打（与提交主题一致）。
$exactTag = Invoke-GitQuiet describe --exact-match --tags HEAD
$exactTagOk = ($exactTag.Exit -eq 0 -and $exactTag.Out -and
  (($exactTag.Out | Select-Object -First 1).ToString().Trim()) -eq $tag)
if (-not $exactTagOk) {
  $tagRes = Invoke-GitQuiet tag $tag
  if ($tagRes.Exit -ne 0) {
    throw "无法在 HEAD 打 tag $tag（可能已存在指向其它提交的同名 tag）：$($tagRes.Out)"
  }
  Write-Host "==> 已在 HEAD 打 tag $tag（构建版本解析以此为准）" -ForegroundColor Yellow
}

# 总是重建：先删旧产物，避免某平台被跳过时误传上一版本的同名文件。
foreach ($p in $artifactPaths) {
  Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
}

# 构建三平台（release.ps1 → package.ps1：windows + android + unix，内部对缺环境/失败告警跳过）。
& (Join-Path $root "release.ps1") all
if ($LASTEXITCODE -ne 0) { throw "构建失败（release.ps1 all 退出码 $LASTEXITCODE）" }

# 收集实际产出的产物；缺失只告警（对应平台被跳过），一个都没有才终止。
$present = @($artifactPaths | Where-Object { Test-Path -LiteralPath $_ })
$missing = @($artifactPaths | Where-Object { -not (Test-Path -LiteralPath $_) })
if ($missing.Count -gt 0) {
  Write-Warning "以下产物缺失（对应平台环境未就绪或构建失败），本次发布不含它们：$(($missing | ForEach-Object { Split-Path $_ -Leaf }) -join ', ')"
}
if ($present.Count -eq 0) { throw "没有任何产物构建成功，无法发布" }

# 分支与 tag 一律推送——gh release create 要求远端已存在该 tag，只推 tag 不推分支会让远端 release 缺少对应提交。
# （tag 已在构建前确保存在于 HEAD。）
git push origin HEAD
if ($LASTEXITCODE -ne 0) { throw "推送当前分支到 origin 失败" }
git push origin $tag
if ($LASTEXITCODE -ne 0) { throw "推送 tag $tag 到 origin 失败" }

# release 已存在则补传产物，不存在则创建。
# PS 5.1/pwsh 下 gh 写 stderr + ErrorActionPreference=Stop 会变终止性错误，局部降级。
$saved = $ErrorActionPreference
$ErrorActionPreference = "Continue"
gh release view $tag *> $null
$releaseExists = $LASTEXITCODE -eq 0
if ($releaseExists) {
  gh release upload $tag @present --clobber
} else {
  gh release create $tag @present --title $tag --generate-notes
}
$publishOk = $LASTEXITCODE -eq 0
$ErrorActionPreference = $saved
if (-not $publishOk) { throw "gh release 失败" }

Write-Host "已发布 $tag ：$($present.Count) 个产物（$(($present | ForEach-Object { Split-Path $_ -Leaf }) -join ', ')）" -ForegroundColor Green
