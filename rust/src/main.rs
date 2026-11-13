//! ProxyPool Rust 版入口。
//!
//! 启动方式:
//! - 双击 exe / `proxy-pool run`  合并模式: 调度 + API 同进程一起跑
//! - `proxy-pool schedule`        只跑调度(采集 + 校验)
//! - `proxy-pool server`          只跑 WebApi

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "proxy-pool",
    version = proxy_pool::settings::VERSION,
    about = "ProxyPool 免费代理IP池(Rust 版)"
)]
struct Cli {
    /// 省略子命令等价于 `run`(双击 exe 即此模式)
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 启动全部: 调度 + WebApi(同进程)
    Run,
    /// 只启动调度程序
    Schedule,
    /// 只启动 WebApi 服务
    Server,
}

#[tokio::main]
async fn main() {
    proxy_pool::logger::init();
    let cli = Cli::parse();
    let result = match cli.cmd {
        Some(Cmd::Schedule) => proxy_pool::scheduler::run().await,
        Some(Cmd::Server) => proxy_pool::api::run().await,
        Some(Cmd::Run) => proxy_pool::runner::run_all().await,
        None => {
            println!(
                "未指定子命令, 默认同时启动 调度+API(等价 `proxy-pool run`), Ctrl-C 停止。帮助: proxy-pool -h"
            );
            proxy_pool::runner::run_all().await
        }
    };
    if let Err(e) = result {
        // 顶层唯一兜底点: 统一落一条错误日志后以非零码退出
        eprintln!("proxy-pool 退出: {e}");
        std::process::exit(1);
    }
}
