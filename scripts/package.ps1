param(
  [ValidateSet("all", "windows", "android")]
  [string]$Targets = "all",
  [switch]$Log
)

$ErrorActionPreference = "Stop"

$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $scriptPath "..")
Set-Location $root

# 版本号唯一来源是 git：最近的 v* tag 优先，其次最近一条 vX.Y.Z 开头的 commit message。
function Get-GitVersion {
  # pwsh 7 的 PSNativeCommandUseErrorActionPreference 会让 git 的非零退出码变成
  # 终止性 NativeCommandError；这里局部降级并吞掉，缺 tag 是正常情况。
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
  return "0.0.0-dev"
}
$script:GitVersion = Get-GitVersion

# MSVC 环境自举：cl.exe 不在 PATH 时，尝试从本机已知的 setup bat 导入环境变量。
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
  $msvcSetup = "D:\env\MSVC\msvc\setup_x64.bat"
  if (Test-Path -LiteralPath $msvcSetup) {
    Write-Host "cl.exe not in PATH; importing MSVC environment from $msvcSetup" -ForegroundColor DarkGray
    cmd /c "`"$msvcSetup`" >nul 2>&1 && set" | ForEach-Object {
      if ($_ -match "^([^=]+)=(.*)$") {
        [Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
      }
    }
  } else {
    Write-Warning "cl.exe not found in PATH and no known MSVC setup script; the Rust build may fail."
  }
}

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
  # 无条件执行 npm install：有 lockfile 时很快，且保证新依赖（如 @tauri-apps/plugin-notification）就位。
  npm install
  if ($LASTEXITCODE -ne 0) { throw "npm install failed" }

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

  $effectiveVersion = $script:GitVersion

  # -------------------------------------------------------------------------
  # Windows desktop build
  # -------------------------------------------------------------------------
  if ($buildWindows) {
    Write-Host "==> Building Windows GUI binary (kxtodo.exe)..." -ForegroundColor Cyan
    $binaryPath = Join-Path $root "src-tauri\target\release\kxtodo.exe"
    Remove-Item -LiteralPath $binaryPath -Force -ErrorAction SilentlyContinue
    npx tauri build --no-bundle $verboseFlag
    if ($LASTEXITCODE -ne 0) { throw "Windows GUI build failed" }

    $releaseBinary = Join-Path $releaseDir "KXToDo-$effectiveVersion.exe"
    Copy-Item -LiteralPath $binaryPath -Destination $releaseBinary -Force
    Write-Host "Built Windows GUI binary: $releaseBinary" -ForegroundColor Green

    Write-Host "==> Building Windows CLI binary (kxtodo-cli.exe)..." -ForegroundColor Cyan
    $cliBinaryPath = Join-Path $root "src-tauri\target\release\kxtodo-cli.exe"
    Remove-Item -LiteralPath $cliBinaryPath -Force -ErrorAction SilentlyContinue
    cargo build --release -p kxtodo-cli --manifest-path (Join-Path $root "src-tauri\Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "Windows CLI build failed" }

    $releaseCliBinary = Join-Path $releaseDir "KXToDo-CLI-$effectiveVersion.exe"
    Copy-Item -LiteralPath $cliBinaryPath -Destination $releaseCliBinary -Force
    Write-Host "Built Windows CLI binary: $releaseCliBinary" -ForegroundColor Green
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

    # Ensure Android signing keystore exists (auto-generate on first run).
    $keystoreDir = Join-Path $root "src-tauri\gen\android\keystore"
    $keystoreFile = Join-Path $keystoreDir "release.jks"
    if (-not (Test-Path -LiteralPath $keystoreFile)) {
      New-Item -ItemType Directory -Force -Path $keystoreDir | Out-Null
      $keytool = Get-Command keytool -ErrorAction SilentlyContinue
      if (-not $keytool) {
        throw "keytool not found. Install the JDK and ensure JAVA_HOME or keytool is in PATH."
      }
      & $keytool.Source -genkey -v `
        -keystore $keystoreFile `
        -storepass kxtodo `
        -alias kxtodo `
        -keypass kxtodo `
        -keyalg RSA -keysize 2048 -validity 10000 `
        -dname "CN=KXToDo, O=KXToDo, C=CN" 2>&1 | Out-Null
      Write-Host "Generated Android signing keystore: $keystoreFile" -ForegroundColor Green
    }

    # 版本号唯一来源是 git：gradle 读取 KXTODO_VERSION 环境变量生成 versionName/versionCode。
    $env:KXTODO_VERSION = $effectiveVersion

    npx tauri android build --apk $verboseFlag
    if ($LASTEXITCODE -ne 0) { throw "Android build failed" }

    $apkSearchRoot = Join-Path $root "src-tauri\gen\android\app\build\outputs\apk"
    $apk = Get-ChildItem -Path $apkSearchRoot -Recurse -Filter "*.apk" -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending |
      Select-Object -First 1
    if (-not $apk) {
      throw "Android build completed but no APK was found under $apkSearchRoot"
    }

    # 发布资产固定命名 KXToDo.apk（安卓覆盖安装，不留历史版本包）；
    # sidecar 记录本次构建版本，供 publish.ps1 识别同名旧产物。
    $releaseApk = Join-Path $releaseDir "KXToDo.apk"
    Copy-Item -LiteralPath $apk.FullName -Destination $releaseApk -Force
    [System.IO.File]::WriteAllText(
      (Join-Path $releaseDir "KXToDo.apk.version"),
      $effectiveVersion,
      [System.Text.UTF8Encoding]::new($false)
    )
    Write-Host "Built Android APK: $releaseApk" -ForegroundColor Green
  }

  Write-Host "Done. Artifacts are in $releaseDir" -ForegroundColor Green
}
finally {
  if ($transcriptStarted) {
    Stop-Transcript | Out-Null
  }
}
