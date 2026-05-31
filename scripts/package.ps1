param(
  [string]$Version = ""
)

$ErrorActionPreference = "Stop"

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $scriptPath "..")
Set-Location $root

$env:CARGO_HOME = Join-Path $root ".cargo-home"
$env:CARGO_TARGET_DIR = Join-Path $root "src-tauri\target"
New-Item -ItemType Directory -Force -Path $env:CARGO_HOME | Out-Null

if ($Version.Trim().Length -gt 0) {
  if ($Version -notmatch '^\d+\.\d+\.\d+([-.+][0-9A-Za-z.-]+)?$') {
    throw "Version must look like 1.2.3 or 1.2.3-beta.1"
  }

  $packageJsonPath = Join-Path $root "package.json"
  $packageJson = Get-Content -LiteralPath $packageJsonPath -Raw | ConvertFrom-Json
  $packageJson.version = $Version
  $packageJson | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $packageJsonPath -Encoding UTF8

  $tauriConfigPath = Join-Path $root "src-tauri\tauri.conf.json"
  $tauriConfig = Get-Content -LiteralPath $tauriConfigPath -Raw | ConvertFrom-Json
  $tauriConfig.version = $Version
  $tauriConfig | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $tauriConfigPath -Encoding UTF8

  $cargoTomlPath = Join-Path $root "src-tauri\Cargo.toml"
  $cargoToml = Get-Content -LiteralPath $cargoTomlPath -Raw
  $cargoToml = $cargoToml -replace '(?m)^version = ".*"$', "version = `"$Version`""
  Set-Content -LiteralPath $cargoTomlPath -Value $cargoToml -Encoding UTF8
}

if (!(Test-Path -LiteralPath (Join-Path $root "node_modules"))) {
  npm install
}

# Regenerate app icons from logo.png so swapping logo.png + repackaging just works.
# Best-effort: requires Python with Pillow. If unavailable, the committed icons are used.
$logoPath = Join-Path $root "logo.png"
if (Test-Path -LiteralPath $logoPath) {
  $pythonCmd = Get-Command python -ErrorAction SilentlyContinue
  if ($pythonCmd) {
    & $pythonCmd.Source (Join-Path $root "scripts\make-icon.py")
    if ($LASTEXITCODE -ne 0) {
      Write-Warning "Icon regeneration skipped (Pillow not available). Using committed icons."
    }
  } else {
    Write-Warning "Python not found; skipping icon regeneration. Using committed icons."
  }
}

npm run build
$binaryPath = Join-Path $root "src-tauri\target\release\todo-note.exe"
Remove-Item -LiteralPath $binaryPath -Force -ErrorAction SilentlyContinue
npm run tauri -- build --no-bundle

$effectiveVersion = if ($Version.Trim().Length -gt 0) {
  $Version
} else {
  (Get-Content -LiteralPath (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version
}

$releaseDir = Join-Path $root "release"
$releaseBinary = Join-Path $releaseDir "KXToDo-$effectiveVersion.exe"
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
Copy-Item -LiteralPath $binaryPath -Destination $releaseBinary -Force
Write-Host "Built binary: $releaseBinary"
