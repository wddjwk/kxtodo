# KXToDo release build.
# Usage:
#   .\release.ps1           # Windows（版本号取 git：最近的 v* tag 或 vX.Y.Z 开头的 commit）
#   .\release.ps1 all       # Windows + Android
param(
  [Parameter(Position = 0)]
  [string]$Targets = "windows"
)

# Shorthand mapping
$targetMap = @{ win = "windows"; windows = "windows"; droid = "android"; android = "android"; all = "all" }
if (!$targetMap.ContainsKey($Targets.ToLower())) {
  Write-Error "Unknown target '$Targets'. Use: win, droid, all"
  exit 1
}
$resolvedTargets = $targetMap[$Targets.ToLower()]

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$packageScript = Join-Path $scriptDir "scripts\package.ps1"

& $packageScript -Targets $resolvedTargets
