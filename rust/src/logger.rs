//! 日志: tracing 输出到 stdout, 级别由 `RUST_LOG` 控制(默认 info)。
//!
//! Python 版在非 Windows 平台写 log/ 目录文件; Rust 版统一不落盘,
//! 长期运行建议交给 systemd/journald 或进程管理器采集。

pub fn init() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
