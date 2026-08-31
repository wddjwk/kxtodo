param(
  [string]$DataDir = ""
)

$ErrorActionPreference = "Stop"

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $scriptPath "..")

if ($DataDir.Trim().Length -eq 0) {
  $DataDir = Join-Path $env:LOCALAPPDATA "kxtodo\todo-note-data"
}

$sample = Get-Content -LiteralPath (Join-Path $root "test-data\sample-export.json") -Raw | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null
$sample.state | ConvertTo-Json -Depth 50 | Set-Content -LiteralPath (Join-Path $DataDir "data.json") -Encoding UTF8
if ($sample.settings) {
  $sample.settings | ConvertTo-Json -Depth 50 | Set-Content -LiteralPath (Join-Path $DataDir "settings.json") -Encoding UTF8
}

Write-Host "Loaded sample data into $DataDir"
