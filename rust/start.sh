#!/usr/bin/env bash
# 与 Python 版 start.sh 对齐: 先起 API 服务, 再前台跑调度器
# 首次使用需先执行: cargo build --release
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
"$SCRIPT_DIR/target/release/proxy-pool" server &
exec "$SCRIPT_DIR/target/release/proxy-pool" schedule
