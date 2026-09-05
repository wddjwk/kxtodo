param(
  # 默认只构建 Windows + Android；unix（Linux，经 WSL 原生克隆）需要显式指定。
  [string[]]$Targets = @("windows", "android"),
  [switch]$Log
)

$ErrorActionPreference = "Stop"

# 目标规范化：all 展开三平台；linux 是 unix 别名；非法值报错。
$targetAliases = @{ windows = "windows"; android = "android"; unix = "unix"; linux = "unix"; all = "all" }
$targetSet = New-Object System.Collections.Generic.HashSet[string]
foreach ($token in $Targets) {
  $t = "$token".Trim().ToLower()
  if (-not $targetAliases.ContainsKey($t)) {
    Write-Error "Unknown target '$token'. Use: windows, android, unix/linux, all"
    exit 1
  }
  if ($t -eq "all") {
    foreach ($canonical in @("windows", "android", "unix")) { [void]$targetSet.Add($canonical) }
  } else {
    [void]$targetSet.Add($targetAliases[$t])
  }
}

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

$env:CARGO_HOME = Join-Path $root ".cargo-home"
$env:CARGO_TARGET_DIR = Join-Path $root "src-tauri\target"
New-Item -ItemType Directory -Force -Path $env:CARGO_HOME | Out-Null

$buildWindows = $targetSet.Contains("windows")
$buildAndroid = $targetSet.Contains("android")
$buildUnix = $targetSet.Contains("unix")

$built = New-Object System.Collections.Generic.List[string]
$skipped = New-Object System.Collections.Generic.List[string]

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

$effectiveVersion = $script:GitVersion
Write-Host "==> 版本：$effectiveVersion（目标：$($targetSet -join "+")）" -ForegroundColor Cyan

try {
  # -------------------------------------------------------------------------
  # 前端准备（Windows / Android 共用；Linux 在 WSL 原生克隆里自行 npm install/build）
  # -------------------------------------------------------------------------
  if ($buildWindows -or $buildAndroid) {
    $prepOk = $true
    try {
      # 无条件执行 npm install：有 lockfile 时很快，且保证新依赖就位。
      npm install
      if ($LASTEXITCODE -ne 0) { throw "npm install failed" }

      # 从 logo.png 再生成图标（尽力而为：需要 Python + Pillow，缺则沿用已提交图标）。
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
    } catch {
      Write-Warning "前端准备失败，跳过 Windows/Android 构建：$_"
      $prepOk = $false
    }
    if (-not $prepOk) {
      if ($buildWindows) { $skipped.Add("windows") }
      if ($buildAndroid) { $skipped.Add("android") }
      $buildWindows = $false
      $buildAndroid = $false
    }
  }

  # -------------------------------------------------------------------------
  # MSVC 环境自举（仅 Windows）：cl.exe 不在 PATH 时尝试从本机 setup bat 导入。
  # -------------------------------------------------------------------------
  if ($buildWindows -and -not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $msvcSetup = "D:\env\MSVC\msvc\setup_x64.bat"
    if (Test-Path -LiteralPath $msvcSetup) {
      Write-Host "cl.exe not in PATH; importing MSVC environment from $msvcSetup" -ForegroundColor DarkGray
      cmd /c "`"$msvcSetup`" >nul 2>&1 && set" | ForEach-Object {
        if ($_ -match "^([^=]+)=(.*)$") {
          [Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
        }
      }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
      Write-Warning "未找到 MSVC（cl.exe），跳过 Windows 构建"
      $skipped.Add("windows")
      $buildWindows = $false
    }
  }

  # -------------------------------------------------------------------------
  # Windows desktop build（固定名 KXToDo.exe + kxtodo-cli.exe + kxtodo-server.exe）
  # -------------------------------------------------------------------------
  if ($buildWindows) {
    try {
      Write-Host "==> Building Windows GUI (KXToDo.exe)..." -ForegroundColor Cyan
      $binaryPath = Join-Path $root "src-tauri\target\release\kxtodo.exe"
      Remove-Item -LiteralPath $binaryPath -Force -ErrorAction SilentlyContinue
      npx tauri build --no-bundle $verboseFlag
      if ($LASTEXITCODE -ne 0) { throw "Windows GUI build failed" }
      Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $releaseDir "KXToDo.exe") -Force

      Write-Host "==> Building Windows CLI (kxtodo-cli.exe)..." -ForegroundColor Cyan
      $cliBinaryPath = Join-Path $root "src-tauri\target\release\kxtodo-cli.exe"
      Remove-Item -LiteralPath $cliBinaryPath -Force -ErrorAction SilentlyContinue
      cargo build --release -p kxtodo-cli --manifest-path (Join-Path $root "src-tauri\Cargo.toml")
      if ($LASTEXITCODE -ne 0) { throw "Windows CLI build failed" }
      Copy-Item -LiteralPath $cliBinaryPath -Destination (Join-Path $releaseDir "kxtodo-cli.exe") -Force

      Write-Host "==> Building Windows sync server (kxtodo-server.exe)..." -ForegroundColor Cyan
      $serverBinaryPath = Join-Path $root "src-tauri\target\release\kxtodo-server.exe"
      Remove-Item -LiteralPath $serverBinaryPath -Force -ErrorAction SilentlyContinue
      cargo build --release -p kxtodo-server --manifest-path (Join-Path $root "src-tauri\Cargo.toml")
      if ($LASTEXITCODE -ne 0) { throw "Windows kxtodo-server build failed" }
      Copy-Item -LiteralPath $serverBinaryPath -Destination (Join-Path $releaseDir "kxtodo-server.exe") -Force

      $built.Add("windows")
      Write-Host "Built Windows: KXToDo.exe + kxtodo-cli.exe + kxtodo-server.exe" -ForegroundColor Green
    } catch {
      Write-Warning "Windows 构建失败，跳过：$_"
      $skipped.Add("windows")
    }
  }

  # -------------------------------------------------------------------------
  # Android build（固定名 KXToDo.apk，覆盖安装不留历史包）
  # -------------------------------------------------------------------------
  if ($buildAndroid) {
    if (-not $env:ANDROID_HOME) {
      Write-Warning "ANDROID_HOME 未设置，跳过 Android 构建"
      $skipped.Add("android")
    } else {
      try {
        Write-Host "==> Building Android APK..." -ForegroundColor Cyan
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
        Copy-Item -LiteralPath $apk.FullName -Destination (Join-Path $releaseDir "KXToDo.apk") -Force

        $built.Add("android")
        Write-Host "Built Android APK: KXToDo.apk" -ForegroundColor Green
      } catch {
        Write-Warning "Android 构建失败，跳过：$_"
        $skipped.Add("android")
      }
    }
  }

  # -------------------------------------------------------------------------
  # Linux build（经 WSL 在原生克隆里构建，产物回拷 release/KXToDo.AppImage + kxtodo-cli + kxtodo-server）
  # 环境未就绪（无 wsl.exe / 无原生克隆 / 克隆脏）或构建失败 → 告警跳过，不终止其它平台。
  # -------------------------------------------------------------------------
  if ($buildUnix) {
    try {
      $wslCmd = Get-Command wsl.exe -ErrorAction SilentlyContinue
      if (-not $wslCmd) { throw "未找到 wsl.exe（Windows 未启用 WSL）" }

      $savedEap = $ErrorActionPreference
      $ErrorActionPreference = "Continue"
      $probe = (& wsl.exe -e bash -lc "echo __wsl_ok__" 2>$null) -join ""
      $ErrorActionPreference = $savedEap
      if ($probe -notmatch "__wsl_ok__") { throw "WSL 默认发行版不可用" }

      $winRepoWsl = (((& wsl.exe -e wslpath -u "$root") -join "") -replace "\r", "").Trim()
      if (-not $winRepoWsl) { throw "无法解析 Windows 仓库的 WSL 路径" }
      $nativeRepo = if ($env:KXTODO_WSL_REPO) { $env:KXTODO_WSL_REPO } else { "~/projects/kxtodo" }
      $headSha = ((git rev-parse HEAD) -join "").Trim()
      if (-not $headSha) { throw "无法取得 Windows 仓库 HEAD SHA" }
      $linuxScript = "$winRepoWsl/scripts/wsl-linux-build.sh"

      Write-Host "==> Building Linux via WSL（原生克隆 $nativeRepo @ $($headSha.Substring(0, [Math]::Min(12, $headSha.Length)))）..." -ForegroundColor Cyan
      # 用登录 shell（-lc）保证 cargo/npm/node 的 PATH；构建输出直通控制台。
      # 局部降级 ErrorActionPreference：WSL 会经 stderr 输出大量构建日志，避免被当成终止性错误。
      $savedEap2 = $ErrorActionPreference
      $ErrorActionPreference = "Continue"
      & wsl.exe -e bash -lc "bash '$linuxScript' '$winRepoWsl' '$nativeRepo' '$headSha'"
      $wslExit = $LASTEXITCODE
      $ErrorActionPreference = $savedEap2
      if ($wslExit -ne 0) { throw "WSL Linux 构建退出码 $wslExit（详见上方日志）" }

      $appimage = Join-Path $releaseDir "KXToDo.AppImage"
      $linuxCli = Join-Path $releaseDir "kxtodo-cli"
      $linuxServer = Join-Path $releaseDir "kxtodo-server"
      if (-not (Test-Path -LiteralPath $appimage) -or -not (Test-Path -LiteralPath $linuxCli) -or -not (Test-Path -LiteralPath $linuxServer)) {
        throw "Linux 产物缺失（release/KXToDo.AppImage、release/kxtodo-cli 或 release/kxtodo-server）"
      }

      $built.Add("unix")
      Write-Host "Built Linux: KXToDo.AppImage + kxtodo-cli + kxtodo-server" -ForegroundColor Green
    } catch {
      Write-Warning "Linux 构建跳过：$_"
      $skipped.Add("unix")
    }
  }

  # -------------------------------------------------------------------------
  # 汇总
  # -------------------------------------------------------------------------
  Write-Host ""
  $builtLabel = if ($built.Count -gt 0) { $built -join ", " } else { "（无）" }
  Write-Host "构建完成：$builtLabel" -ForegroundColor Green
  if ($skipped.Count -gt 0) {
    Write-Warning "已跳过：$($skipped -join ', ')（环境未就绪或构建失败）"
  }
  Write-Host "产物目录：$releaseDir" -ForegroundColor Green
}
finally {
  if ($transcriptStarted) {
    Stop-Transcript | Out-Null
  }
}
