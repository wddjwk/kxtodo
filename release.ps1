# KXToDo release build.
# Usage:
#   .\release.ps1                # Windows, version from Cargo.toml
#   .\release.ps1 win 9.0.1     # Windows, explicit version
#   .\release.ps1 all            # Windows + Android
param(
  [Parameter(Position = 0)]
  [string]$Targets = "windows",
  [Parameter(Position = 1)]
  [string]$Version = ""
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

if ($Version) {
  & $packageScript -Targets $resolvedTargets -Version $Version
} else {
  & $packageScript -Targets $resolvedTargets
}
