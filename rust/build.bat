@echo off
REM 编译 Rust 版 proxy-pool (release)
REM 双击运行或在任意目录执行; 产物: target\release\proxy-pool.exe

cd /d "%~dp0"

echo [build] cargo build --release ...
cargo build --release
if errorlevel 1 (
    echo.
    echo [build] 编译失败, 请检查上方错误信息
    pause
    exit /b 1
)

echo.
echo [build] 编译成功: %~dp0target\release\proxy-pool.exe
if "%~1"=="" (
    REM 双击运行时窗口会一闪而过, 停一下让用户看到结果
    echo [build] 按任意键关闭...
    pause >nul
)
