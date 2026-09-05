# KXToDo release build.
# Usage:
#   .\release.ps1           # Windows（版本号取 git：最近的 v* tag 或 vX.Y.Z 开头的 commit）
#   .\release.ps1 win       # Windows（KXToDo.exe + kxtodo-cli.exe）
#   .\release.ps1 android   # Android（KXToDo.apk）
#   .\release.ps1 unix      # Linux（经 WSL 原生克隆构建 KXToDo.AppImage + kxtodo-cli）
#   .\release.ps1 all       # Windows + Android + Linux
# 某一平台环境未就绪（缺 MSVC / ANDROID_HOME / WSL / 原生克隆）或构建失败时告警跳过，不终止其它平台。
param(
  [Parameter(Position = 0)]
  [string]$Targets = "windows"
)

# Shorthand mapping → canonical target understood by scripts/package.ps1
$targetMap = @{
  win     = "windows"
  windows = "windows"
  droid   = "android"
  android = "android"
  unix    = "unix"
  linux   = "unix"
  all     = "all"
}
$key = $Targets.ToLower()
if (!$targetMap.ContainsKey($key)) {
  Write-Error "Unknown target '$Targets'. Use: win, android, unix, all"
  exit 1
}
$resolvedTargets = $targetMap[$key]

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$packageScript = Join-Path $scriptDir "scripts\package.ps1"

& $packageScript -Targets $resolvedTargets
