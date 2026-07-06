#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-8.2.0}"
TARGETS="${2:-all}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

powershell.exe -ExecutionPolicy Bypass -File ".\\scripts\\package.ps1" -Version "$VERSION" -Targets "$TARGETS"
