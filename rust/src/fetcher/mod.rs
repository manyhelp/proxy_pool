//! 抓取调度: 并发运行启用的抓取源, 去重并聚合来源, 做格式预校验。
//! 对齐 Python 版 helper/fetch.py。

pub mod sources;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::proxy::Proxy;
use crate::settings::Settings;

/// 运行全部启用的抓取源, 返回去重且通过格式预校验的代理列表。
///
/// 每个源一个并发任务; 同一代理被多个源命中时合并 `source`
/// (对齐 Python 采集字典的 add_source)。
///
/// # Examples
///
/// ```no_run
/// use std::sync::Arc;
///
/// use proxy_pool::fetcher;
/// use proxy_pool::settings::Settings;
///
/// # async fn demo() {
/// let conf = Arc::new(Settings::load().expect("配置加载失败"));
/// let proxies = fetcher::run(&conf).await;
/// # }
/// ```
pub async fn run(conf: &Arc<Settings>) -> Vec<Proxy> {
    info!(count = conf.proxy_fetcher.len(), "ProxyFetch start");

    let mut handles: Vec<(String, JoinHandle<Vec<String>>)> = Vec::new();
    for name in &conf.proxy_fetcher {
        let name = name.clone();
        let conf = conf.clone();
        handles.push((
            name.clone(),
            tokio::spawn(async move { sources::fetch(&name, &conf).await }),
        ));
    }

    let mut dict: HashMap<String, Proxy> = HashMap::new();
    for (name, handle) in handles {
        let list = match handle.await {
            Ok(list) => list,
            Err(e) => {
                error!(source = name, error = %e, "抓取任务 panic");
                continue;
            }
        };
        for raw in list {
            let addr = raw.trim().to_string();
            if addr.is_empty() {
                continue;
            }
            match dict.entry(addr) {
                Entry::Occupied(mut e) => e.get_mut().add_source(&name),
                Entry::Vacant(e) => {
                    let mut p = Proxy::new(e.key().clone());
                    p.add_source(&name);
                    e.insert(p);
                }
            }
        }
    }

    let total = dict.len();
    let valid: Vec<Proxy> = dict
        .into_values()
        .filter(|p| Proxy::is_valid_format(&p.proxy))
        .collect();
    info!(total, valid = valid.len(), "ProxyFetch complete");
    valid
}
