//! 校验执行器: 有界并发消费一批代理, 逐个校验并写回/淘汰。
//! 入库/淘汰判定对齐 Python 版 helper/check.py 的 `_ThreadChecker`。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::stream::{self, StreamExt};
use tracing::{info, warn};

use crate::errors::PoolResult;
use crate::handler::ProxyHandler;
use crate::proxy::Proxy;
use crate::validator::{Mode, Validator};

/// 一轮校验的结果计数。
#[derive(Debug, Clone, Copy)]
pub struct CheckStats {
    /// 参与校验的代理数
    pub total: usize,
    /// 写回/入库数
    pub kept: usize,
    /// 淘汰数
    pub deleted: usize,
}

/// 执行一轮校验。
///
/// 并发数为 `CHECK_CONCURRENCY`(默认 100)。Python 版固定 20 线程是全量 2 分钟
/// 校验跑不完的主因, 这是明确的吞吐改进点, 需要时可用环境变量调回小值。
///
/// # Examples
///
/// ```no_run
/// use std::sync::Arc;
///
/// use proxy_pool::checker;
/// use proxy_pool::handler::bootstrap;
/// use proxy_pool::validator::{Mode, Validator};
///
/// # async fn demo() -> Result<(), proxy_pool::errors::PoolError> {
/// let (conf, handler) = bootstrap().await?;
/// let validator = Arc::new(Validator::new(conf));
/// checker::run(Mode::Use, Vec::new(), handler, validator).await;
/// # Ok(())
/// # }
/// ```
pub async fn run(
    mode: Mode,
    proxies: Vec<Proxy>,
    handler: Arc<ProxyHandler>,
    validator: Arc<Validator>,
) -> CheckStats {
    let total = proxies.len();
    let concurrency = handler.conf().check_concurrency.max(1);
    let kept = Arc::new(AtomicUsize::new(0));
    let deleted = Arc::new(AtomicUsize::new(0));

    stream::iter(proxies)
        .map(|mut proxy| {
            let handler = handler.clone();
            let validator = validator.clone();
            let kept = kept.clone();
            let deleted = deleted.clone();
            async move {
                validator.validate(&mut proxy, mode).await;
                match mode {
                    Mode::Raw => {
                        // Python __ifRaw: 存在则跳过保留原记录, 不存在才入库。
                        // HSETNX 原子实现: 省一次 exists 往返, 且不存在"检查后被人写入"的竞态。
                        if proxy.last_status {
                            match handler.put_if_absent(&proxy).await {
                                Ok(true) => {
                                    kept.fetch_add(1, Ordering::Relaxed);
                                }
                                Ok(false) => {}
                                Err(e) => warn!(proxy = proxy.proxy, error = %e, "raw 入库失败"),
                            }
                        }
                    }
                    Mode::Use => {
                        let max_fail = handler.conf().max_fail_count;
                        // put/delete 返回类型不同, 统一成 (), 删除与否用 is_delete 区分
                        let is_delete = !proxy.last_status && proxy.fail_count > max_fail;
                        let outcome: PoolResult<()> = if is_delete {
                            handler.delete(&proxy).await.map(|_| ())
                        } else {
                            handler.put(&proxy).await
                        };
                        match outcome {
                            Ok(()) => {
                                let counter = if is_delete { &deleted } else { &kept };
                                counter.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => warn!(proxy = proxy.proxy, error = %e, "use 回写失败"),
                        }
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .for_each(|_| async {})
        .await;

    let stats = CheckStats {
        total,
        kept: kept.load(Ordering::Relaxed),
        deleted: deleted.load(Ordering::Relaxed),
    };
    info!(
        ?mode,
        total = stats.total,
        kept = stats.kept,
        deleted = stats.deleted,
        "ProxyCheck complete"
    );
    stats
}
