//! 调度器: 两个周期任务对齐 Python 版 helper/scheduler.py —
//! fetch 每 10 分钟(采集 + raw 校验), check 每 2 分钟(use 校验; 池子低于
//! `pool_size_min` 时先补采)。启动时先立即执行一轮 fetch(对齐 Python)。
//!
//! 在 Python 版语义之上补了三个流程优化(详见 README 行为差异):
//! 1. 同名任务防重叠(原子标志);
//! 2. 定时抓取与 check 内嵌补采**共用互斥**, 防止并发双跑造成重复抓取/校验
//!    (获取频率不做限制: 池子低于下限时每轮 check 都会补采);
//! 3. 采集结果先与池内 key 比对, **已在池中的代理不再重复 raw 校验**;
//!    刚入库的代理在随后的 use 复检中跳过。

use std::collections::HashSet;
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

/// 抓取互斥: 定时任务与 check 内嵌补采共用, 同一时刻只允许一轮采集。
/// 只防"并发双跑"这种重复劳动, 不限制抓取频率。
#[derive(Default)]
struct FetchState {
    /// 拿到执行权期间为 true
    running: AtomicBool,
}

impl FetchState {
    /// 尝试占用执行权; true = 占到(执行完必须调用 [`Self::finish`]), false = 已有采集在跑。
    fn try_acquire(&self) -> bool {
        !self.running.swap(true, Ordering::AcqRel)
    }

    /// 采集结束, 释放执行权。
    fn finish(&self) {
        self.running.store(false, Ordering::Release);
    }
}

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

    let fetch_state = Arc::new(FetchState::default());

    // 对齐 Python: 启动即执行一轮采集
    let ingested = fetch_round(&conf, &handler, &validator).await;
    fetch_state.finish();
    info!(ingested = ingested.len(), "启动采集完成");

    let mut fetch_tick = tokio::time::interval(Duration::from_secs(10 * 60));
    let mut check_tick = tokio::time::interval(Duration::from_secs(2 * 60));
    // 进程挂起(如系统休眠)恢复后跳过积压 tick, 防止连发; 任务重叠另有原子标志兜底
    fetch_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    check_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // interval 首个 tick 立即完成, 消费掉, 避免与启动抓取重复执行
    fetch_tick.tick().await;
    check_tick.tick().await;

    let check_running = Arc::new(AtomicBool::new(false));

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("收到退出信号, scheduler 结束");
                return Ok(());
            }
            _ = fetch_tick.tick() => {
                if fetch_state.try_acquire() {
                    let (conf, handler, validator, state) = (
                        conf.clone(),
                        handler.clone(),
                        validator.clone(),
                        fetch_state.clone(),
                    );
                    tokio::spawn(async move {
                        fetch_round(&conf, &handler, &validator).await;
                        state.finish();
                    });
                } else {
                    warn!("定时抓取被跳过(上一轮采集尚未完成)");
                }
            }
            _ = check_tick.tick() => {
                if !check_running.swap(true, Ordering::AcqRel) {
                    let (conf, handler, validator, state, flag) = (
                        conf.clone(),
                        handler.clone(),
                        validator.clone(),
                        fetch_state.clone(),
                        check_running.clone(),
                    );
                    tokio::spawn(async move {
                        check_once(&conf, &handler, &validator, &state).await;
                        flag.store(false, Ordering::Release);
                    });
                } else {
                    warn!("上一轮 check 尚未完成, 跳过本轮");
                }
            }
        }
    }
}

/// 一轮采集: 抓取 → 与池内 key 比对预过滤 → 只 raw 校验新代理。
/// 返回本轮新入库的 key 集合(供 check 跳过刚入库代理的复检)。
async fn fetch_round(
    conf: &Arc<Settings>,
    handler: &Arc<ProxyHandler>,
    validator: &Arc<Validator>,
) -> HashSet<String> {
    let proxies = fetcher::run(conf).await;
    let fetched = proxies.len();

    // 已在池中的代理跳过 raw 校验: use 周期任务覆盖它们,
    // 否则每个常驻源代理每轮抓取都被重复验证一次(Python 版的浪费点)
    let existing: HashSet<String> = match handler.get_all_keys().await {
        Ok(keys) => keys.into_iter().collect(),
        Err(e) => {
            warn!(error = %e, "读取池内 key 失败, 退化为全量校验");
            HashSet::new()
        }
    };
    let (to_validate, skipped): (Vec<_>, Vec<_>) = proxies
        .into_iter()
        .partition(|p| !existing.contains(&p.proxy));

    info!(
        fetched,
        already_in_pool = skipped.len(),
        to_validate = to_validate.len(),
        "ProxyFetch 预过滤完成"
    );

    let stats = checker::run(Mode::Raw, to_validate, handler.clone(), validator.clone()).await;
    stats.ingested
}

/// 池内存量复检; 池子低于下限时先补采(共用抓取互斥)。
/// 刚入库的代理在补采中刚校验过, 本轮 use 复检跳过。
async fn check_once(
    conf: &Arc<Settings>,
    handler: &Arc<ProxyHandler>,
    validator: &Arc<Validator>,
    fetch_state: &FetchState,
) {
    let count = match handler.get_count().await {
        Ok(c) => c.total,
        Err(e) => {
            warn!(error = %e, "获取池子数量失败, 本轮仍执行全量校验");
            0
        }
    };

    let mut ingested = HashSet::new();
    if count < conf.pool_size_min {
        if fetch_state.try_acquire() {
            info!(count, min = conf.pool_size_min, "池子低于下限, 先补一轮采集");
            ingested = fetch_round(conf, handler, validator).await;
            fetch_state.finish();
        } else {
            info!(count, "池子低于下限, 但采集正在进行, 本轮跳过补采");
        }
    }

    let proxies = match handler.get_all(false).await {
        Ok(list) => list,
        Err(e) => {
            error!(error = %e, "读取池子失败, 本轮校验跳过");
            return;
        }
    };
    let (to_check, skipped): (Vec<_>, Vec<_>) = proxies
        .into_iter()
        .partition(|p| !ingested.contains(&p.proxy));
    if !skipped.is_empty() {
        info!(skip = skipped.len(), "刚入库的代理本轮免复检");
    }
    checker::run(Mode::Use, to_check, handler.clone(), validator.clone()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_state_is_a_simple_mutex() {
        let st = FetchState::default();
        assert!(st.try_acquire());
        assert!(!st.try_acquire()); // 持有期间再取: 互斥
        st.finish();
        assert!(st.try_acquire()); // 释放后可再取
        st.finish();
    }
}
