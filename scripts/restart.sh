#!/usr/bin/env bash
# 重启 CC Switch 开发模式
set -e

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

"$PROJECT_ROOT/scripts/stop.sh"
sleep 1
"$PROJECT_ROOT/scripts/start.sh"
