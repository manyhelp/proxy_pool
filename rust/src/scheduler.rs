//! 调度器: 两个周期任务对齐 Python 版 helper/scheduler.py —
//! fetch 每 10 分钟(采集 + raw 校验), check 每 2 分钟(use 校验; 池子低于
//! `pool_size_min` 时先补采)。启动时先立即执行一轮 fetch(对齐 Python)。
//!
//! 与 Python 版差异: 同名任务用原子标志防重叠(Python `max_instances=10` 允许
//! 多实例并存, 全量校验场景下只会互相挤占连接); 并发数由 CHECK_CONCURRENCY 控制。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::signal;
use tracing::{error, info, warn};

use crate::checker;
use crate::errors::PoolResult;
use crate::fetcher;
use crate::handler::{bootstrap, ProxyHandler};
use crate::settings::{Settings, VERSION};
use crate::validator::{Mode, Validator};

/// 调度主循环入口: 自行装配配置与 DB, 然后进入 [`run_with`]。
///
/// # Errors
///
/// 启动阶段配置文件非法或 DB 连不上(对齐 Python launcher 的快速失败)。
///
/// # Examples
///
/// ```no_run
/// // 由 CLI 的 `proxy-pool schedule` 子命令调用, 需在 tokio 运行时内执行:
/// tokio::runtime::Runtime::new()
///     .unwrap()
///     .block_on(proxy_pool::scheduler::run())
///     .unwrap();
/// ```
pub async fn run() -> PoolResult<()> {
    let (conf, handler) = bootstrap().await?;
    run_with(conf, handler).await
}

/// 调度主循环(使用已装配的组件, 供 [`crate::runner::run_all`] 复用), 常驻直至 Ctrl-C。
pub async fn run_with(conf: Arc<Settings>, handler: Arc<ProxyHandler>) -> PoolResult<()> {
    let validator = Arc::new(Validator::new(conf.clone()));
    info!(
        version = VERSION,
        host = conf.host,
        port = conf.port,
        fetchers = ?conf.proxy_fetcher,
        config = conf.config_file.as_ref().map(|p| p.display().to_string())
            .unwrap_or_else(|| "内置默认(未找到 setting.toml)".into()),
        "ProxyPool scheduler 启动"
    );

    // 对齐 Python: 启动即执行一轮采集
    fetch_once(conf.clone(), handler.clone(), validator.clone()).await;

    let mut fetch_tick = tokio::time::interval(Duration::from_secs(10 * 60));
    let mut check_tick = tokio::time::interval(Duration::from_secs(2 * 60));
    // 进程挂起(如系统休眠)恢复后跳过积压 tick, 防止连发; 任务重叠另有原子标志兜底
    fetch_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    check_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // interval 首个 tick 立即完成, 消费掉, 避免与启动抓取重复执行
    fetch_tick.tick().await;
    check_tick.tick().await;

    let fetch_running = Arc::new(AtomicBool::new(false));
    let check_running = Arc::new(AtomicBool::new(false));

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("收到退出信号, scheduler 结束");
                return Ok(());
            }
            _ = fetch_tick.tick() => {
                // swap false→true 抢占成功才执行: 上一轮未结束时跳过本轮
                if !fetch_running.swap(true, Ordering::AcqRel) {
                    let (conf, handler, validator, flag) = (
                        conf.clone(),
                        handler.clone(),
                        validator.clone(),
                        fetch_running.clone(),
                    );
                    tokio::spawn(async move {
                        fetch_once(conf, handler, validator).await;
                        flag.store(false, Ordering::Release);
                    });
                } else {
                    warn!("上一轮 fetch 尚未完成, 跳过本轮");
                }
            }
            _ = check_tick.tick() => {
                if !check_running.swap(true, Ordering::AcqRel) {
                    let (conf, handler, validator, flag) = (
                        conf.clone(),
                        handler.clone(),
                        validator.clone(),
                        check_running.clone(),
                    );
                    tokio::spawn(async move {
                        check_once(conf, handler, validator).await;
                        flag.store(false, Ordering::Release);
                    });
                } else {
                    warn!("上一轮 check 尚未完成, 跳过本轮");
                }
            }
        }
    }
}

/// 采集 + raw 校验(新代理入库)
async fn fetch_once(conf: Arc<Settings>, handler: Arc<ProxyHandler>, validator: Arc<Validator>) {
    let proxies = fetcher::run(&conf).await;
    checker::run(Mode::Raw, proxies, handler, validator).await;
}

/// 池内存量复检; 低于下限先补采(对齐 Python __runProxyCheck 的内嵌补采顺序,
/// 保证新采代理也参与本轮全量校验)
async fn check_once(conf: Arc<Settings>, handler: Arc<ProxyHandler>, validator: Arc<Validator>) {
    let count = match handler.get_count().await {
        Ok(c) => c.total,
        Err(e) => {
            warn!(error = %e, "获取池子数量失败, 本轮仍执行全量校验");
            0
        }
    };
    if count < conf.pool_size_min {
        info!(count, min = conf.pool_size_min, "池子低于下限, 先补一轮采集");
        fetch_once(conf.clone(), handler.clone(), validator.clone()).await;
    }
    let proxies = match handler.get_all(false).await {
        Ok(list) => list,
        Err(e) => {
            error!(error = %e, "读取池子失败, 本轮校验跳过");
            return;
        }
    };
    checker::run(Mode::Use, proxies, handler, validator).await;
}
