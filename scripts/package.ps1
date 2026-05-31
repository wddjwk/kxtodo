param(
  [string]$Version = "",
  [ValidateSet("all", "windows", "android")]
  [string]$Targets = "all",
  [switch]$Log
)

$ErrorActionPreference = "Stop"

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $scriptPath "..")
Set-Location $root

$env:CARGO_HOME = Join-Path $root ".cargo-home"
$env:CARGO_TARGET_DIR = Join-Path $root "src-tauri\target"
New-Item -ItemType Directory -Force -Path $env:CARGO_HOME | Out-Null

$buildWindows = $Targets -eq "all" -or $Targets -eq "windows"
$buildAndroid = $Targets -eq "all" -or $Targets -eq "android"

$releaseDir = Join-Path $root "release"
New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

# When -Log is passed, mirror the full build output (npm / cargo / gradle) to a
# timestamped log file while still printing it live to the console, and ask the
# Tauri CLI for verbose output so success/failure is easy to diagnose.
$transcriptStarted = $false
$verboseFlag = $null
if ($Log) {
  $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $logFile = Join-Path $releaseDir "build-$stamp.log"
  Start-Transcript -Path $logFile -Append | Out-Null
  $transcriptStarted = $true
  $verboseFlag = "-v"
  Write-Host "Logging build output to $logFile" -ForegroundColor Cyan
}

try {
  # -------------------------------------------------------------------------
  # Version bump (package.json, tauri.conf.json, Cargo.toml, stores.ts APP_VERSION)
  # -------------------------------------------------------------------------
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

    $storesPath = Join-Path $root "src\lib\stores.ts"
    $stores = Get-Content -LiteralPath $storesPath -Raw
    $stores = $stores -replace 'export const APP_VERSION = ".*";', "export const APP_VERSION = `"$Version`";"
    Set-Content -LiteralPath $storesPath -Value $stores -Encoding UTF8
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
  if ($LASTEXITCODE -ne 0) { throw "Frontend build failed" }

  $effectiveVersion = if ($Version.Trim().Length -gt 0) {
    $Version
  } else {
    (Get-Content -LiteralPath (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version
  }

  # -------------------------------------------------------------------------
  # Windows desktop build
  # -------------------------------------------------------------------------
  if ($buildWindows) {
    Write-Host "==> Building Windows desktop binary..." -ForegroundColor Cyan
    $binaryPath = Join-Path $root "src-tauri\target\release\todo-note.exe"
    Remove-Item -LiteralPath $binaryPath -Force -ErrorAction SilentlyContinue
    npx tauri build --no-bundle $verboseFlag
    if ($LASTEXITCODE -ne 0) { throw "Windows build failed" }

    $releaseBinary = Join-Path $releaseDir "KXToDo-$effectiveVersion.exe"
    Copy-Item -LiteralPath $binaryPath -Destination $releaseBinary -Force
    Write-Host "Built Windows binary: $releaseBinary" -ForegroundColor Green
  }

  # -------------------------------------------------------------------------
  # Android build (APK)
  # -------------------------------------------------------------------------
  if ($buildAndroid) {
    Write-Host "==> Building Android APK..." -ForegroundColor Cyan

    if (-not $env:ANDROID_HOME) {
      throw "ANDROID_HOME is not set. Install the Android SDK and set ANDROID_HOME before building Android."
    }
    if (-not $env:NDK_HOME -and -not $env:ANDROID_NDK_HOME) {
      Write-Warning "NDK_HOME is not set; Tauri will try to auto-detect an installed NDK."
    }

    # Ensure the Android Rust targets are installed (idempotent).
    rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android | Out-Null

    # Initialise the Gradle project on first run.
    if (-not (Test-Path -LiteralPath (Join-Path $root "src-tauri\gen\android"))) {
      Write-Host "Android project not found; running 'tauri android init'..." -ForegroundColor Yellow
      npx tauri android init
      if ($LASTEXITCODE -ne 0) { throw "tauri android init failed" }
    }

    npx tauri android build --apk $verboseFlag
    if ($LASTEXITCODE -ne 0) { throw "Android build failed" }

    $apkSearchRoot = Join-Path $root "src-tauri\gen\android\app\build\outputs\apk"
    $apk = Get-ChildItem -Path $apkSearchRoot -Recurse -Filter "*.apk" -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending |
      Select-Object -First 1
    if (-not $apk) {
      throw "Android build completed but no APK was found under $apkSearchRoot"
    }

    $releaseApk = Join-Path $releaseDir "KXToDo-$effectiveVersion.apk"
    Copy-Item -LiteralPath $apk.FullName -Destination $releaseApk -Force
    Write-Host "Built Android APK: $releaseApk" -ForegroundColor Green
  }

  Write-Host "Done. Artifacts are in $releaseDir" -ForegroundColor Green
}
finally {
  if ($transcriptStarted) {
    Stop-Transcript | Out-Null
  }
}
