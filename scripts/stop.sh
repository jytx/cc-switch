#!/usr/bin/env bash
# 停止 CC Switch 开发模式（Tauri + Vite + cargo 编译进程）
set -e

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

# 终止 tauri / vite / cargo 相关进程
pkill -f "tauri dev" 2>/dev/null || true
pkill -f "@tauri-apps/cli" 2>/dev/null || true
pkill -f "vite" 2>/dev/null || true
pkill -f "cargo build" 2>/dev/null || true
pkill -f "cargo check" 2>/dev/null || true
pkill -f "cc-switch" 2>/dev/null || true

# 等待进程清理
sleep 1

# 兜底：再检查一次
if pgrep -f "tauri dev" >/dev/null; then
  pkill -9 -f "tauri dev" 2>/dev/null || true
fi
if pgrep -f "cc-switch" >/dev/null; then
  pkill -9 -f "cc-switch" 2>/dev/null || true
fi

echo "CC Switch dev 已停止"
