param(
  [string]$Version = "8.2.0",
  [ValidateSet("all", "windows", "android")]
  [string]$Targets = "windows",
  [switch]$Log
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Version.Trim()) {
  $Version = (Get-Content -LiteralPath (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version
}

$packageScript = Join-Path $root "scripts\package.ps1"
$arguments = @("-ExecutionPolicy", "Bypass", "-File", $packageScript, "-Version", $Version, "-Targets", $Targets)
if ($Log) {
  $arguments += "-Log"
}

& powershell.exe @arguments
exit $LASTEXITCODE
