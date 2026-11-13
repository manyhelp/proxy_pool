//! 合并运行模式: 同一进程内同时跑 调度器(fetch/check 周期任务)与 WebApi。
//! 双击 exe(无参数)与 `proxy-pool run` 走这条路径;
//! 需要分开部署时用 `proxy-pool schedule` / `proxy-pool server`。

use tracing::info;

use crate::errors::PoolResult;

/// 同时启动调度器与 API(共享一份配置与 DB 连接), 任一退出即结束进程。
/// Ctrl-C 时调度器先正常收尾, 进程随后退出。
///
/// # Errors
///
/// 配置文件非法、DB 自检失败或 API 端口绑定失败(任一失败即整体退出,
/// 不留半个服务的假运行)。
///
/// # Examples
///
/// ```no_run
/// // `proxy-pool run` 或双击 exe 即此模式:
/// tokio::runtime::Runtime::new()
///     .unwrap()
///     .block_on(proxy_pool::runner::run_all())
///     .unwrap();
/// ```
pub async fn run_all() -> PoolResult<()> {
    let (conf, handler) = crate::handler::bootstrap().await?;
    info!("合并模式: schedule + server 同进程运行, Ctrl-C 停止");
    tokio::select! {
        r = crate::scheduler::run_with(conf.clone(), handler.clone()) => r,
        r = crate::api::run_with(conf, handler) => r,
    }
}
