# KXToDo release build.
# Usage:
#   .\release.ps1                  # 默认：Windows + Android（KXToDo.exe + kxtodo-cli.exe + KXToDo.apk）
#   .\release.ps1 win              # 仅 Windows
#   .\release.ps1 android          # 仅 Android
#   .\release.ps1 unix             # 仅 Linux（经 WSL 原生克隆构建 KXToDo.AppImage + kxtodo-cli）
#   .\release.ps1 all              # Windows + Android + Linux 三平台
#   .\release.ps1 win,unix         # 逗号组合任意平台
# 版本号取 git（最近的 v* tag 或 vX.Y.Z 开头的 commit）。
# 某一平台环境未就绪（缺 MSVC / ANDROID_HOME / WSL / 原生克隆）或构建失败时告警跳过，不终止其它平台。
# 无本地构建环境时也可以直接推 v* tag，由 GitHub Actions 的 release.yml 云端构建并发布。
param(
  [Parameter(Position = 0)]
  [string]$Targets = "windows,android"
)

# Shorthand mapping → canonical targets understood by scripts/package.ps1
$tokenMap = @{
  win     = "windows"
  windows = "windows"
  droid   = "android"
  android = "android"
  unix    = "unix"
  linux   = "unix"
}
$targetSet = New-Object System.Collections.Generic.HashSet[string]
foreach ($token in ($Targets -split "[,+]")) {
  $t = $token.Trim().ToLower()
  if ($t -eq "") { continue }
  if ($t -eq "all") {
    foreach ($canonical in @("windows", "android", "unix")) { [void]$targetSet.Add($canonical) }
  } elseif ($tokenMap.ContainsKey($t)) {
    [void]$targetSet.Add($tokenMap[$t])
  } else {
    Write-Error "Unknown target '$token'. Use: win, android, unix, all（可逗号组合，如 win,unix）"
    exit 1
  }
}
if ($targetSet.Count -eq 0) {
  Write-Error "No target specified. Use: win, android, unix, all"
  exit 1
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$packageScript = Join-Path $scriptDir "scripts\package.ps1"

& $packageScript -Targets @($targetSet)
