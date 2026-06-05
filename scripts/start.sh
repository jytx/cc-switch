#!/usr/bin/env bash
# 启动 CC Switch 开发模式（Tauri + Vite）
# 所有日志输出到 logs/dev.log
set -e

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

mkdir -p logs

# 清理可能残留的旧进程
./scripts/stop.sh >/dev/null 2>&1 || true

# 启动 tauri dev，输出到 logs/dev.log
nohup pnpm tauri dev >logs/dev.log 2>&1 &
echo "CC Switch dev 启动中，PID: $!"
echo "日志: tail -f logs/dev.log"
