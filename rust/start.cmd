@echo off
REM 与 Python 版 start.bat 对齐: 先起 API 服务, 再前台跑调度器
REM 首次使用需先执行: cargo build --release
start "" "%~dp0target\release\proxy-pool.exe" server
"%~dp0target\release\proxy-pool.exe" schedule
