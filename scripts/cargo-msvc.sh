#!/usr/bin/env bash
# Run cargo with the MSVC build environment (mirrors D:\env\MSVC\msvc\setup_x64.bat).
# Needed because uutils-coreutils' `link` shadows MSVC link.exe in Git Bash PATH.
set -e
MSVC_ROOT="/d/env/MSVC/msvc"
MSVC_VER="14.44.35207"
SDK_VER="10.0.26100.0"
export PATH="$MSVC_ROOT/VC/Tools/MSVC/$MSVC_VER/bin/Hostx64/x64:$MSVC_ROOT/Windows Kits/10/bin/$SDK_VER/x64:$MSVC_ROOT/Windows Kits/10/bin/$SDK_VER/x64/ucrt:$PATH"
export INCLUDE="D:\\env\\MSVC\\msvc\\VC\\Tools\\MSVC\\$MSVC_VER\\include;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Include\\$SDK_VER\\ucrt;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Include\\$SDK_VER\\shared;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Include\\$SDK_VER\\um;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Include\\$SDK_VER\\winrt;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Include\\$SDK_VER\\cppwinrt"
export LIB="D:\\env\\MSVC\\msvc\\VC\\Tools\\MSVC\\$MSVC_VER\\lib\\x64;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Lib\\$SDK_VER\\ucrt\\x64;D:\\env\\MSVC\\msvc\\Windows Kits\\10\\Lib\\$SDK_VER\\um\\x64"
exec cargo "$@"
