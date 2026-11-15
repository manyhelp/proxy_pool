//! 代理池 CRUD 门面与运行时装配。
//!
//! [`ProxyHandler`] 包装 [`RedisClient`](crate::db::RedisClient), 上层
//! (scheduler/checker/api)只面向 [`Proxy`] 模型; [`bootstrap`] 完成配置加载 +
//! DB 自检, schedule/server 两个入口共用。

use std::sync::Arc;

use tracing::warn;

use crate::db::{PoolCount, RedisClient};
use crate::errors::PoolResult;
use crate::proxy::Proxy;
use crate::settings::Settings;

/// 代理池操作门面。可放入 `Arc` 并发共享(内部 DB 连接自带池化)。
pub struct ProxyHandler {
    conf: Arc<Settings>,
    db: RedisClient,
}

impl ProxyHandler {
    /// 用已建立的 DB 连接构造, 并切换到配置的表名。
    ///
    /// # Errors
    ///
    /// 预留给多后端扩展(当前实现不会失败)。
    pub fn with_db(conf: Arc<Settings>, db: RedisClient) -> PoolResult<Self> {
        let mut db = db;
        db.change_table(&conf.table_name);
        Ok(Self { conf, db })
    }

    /// 共享配置的引用。
    pub fn conf(&self) -> &Arc<Settings> {
        &self.conf
    }

    /// 随机取一个代理; `https=true` 只取支持 https 的。
    ///
    /// # Errors
    ///
    /// Redis 读写失败; 单条坏数据记 warn 跳过, 不视为错误。
    pub async fn get(&self, https: bool) -> PoolResult<Option<Proxy>> {
        match self.db.get(https).await? {
            Some(json) => Ok(parse_or_none(&json)),
            None => Ok(None),
        }
    }

    /// 取出并删除一个代理(消耗式取用)。
    ///
    /// # Errors
    ///
    /// Redis 读写失败。
    pub async fn pop(&self, https: bool) -> PoolResult<Option<Proxy>> {
        match self.db.pop(https).await? {
            Some(json) => Ok(parse_or_none(&json)),
            None => Ok(None),
        }
    }

    /// 写入/更新代理。
    ///
    /// # Errors
    ///
    /// Redis 写入失败。
    pub async fn put(&self, proxy: &Proxy) -> PoolResult<()> {
        self.db.put(proxy).await
    }

    /// 仅当不存在时写入(新代理入库), 返回是否写入。
    ///
    /// # Errors
    ///
    /// Redis 写入失败。
    pub async fn put_if_absent(&self, proxy: &Proxy) -> PoolResult<bool> {
        self.db.put_if_absent(proxy).await
    }

    /// 删除代理, 返回是否删除了记录。
    ///
    /// # Errors
    ///
    /// Redis 删除失败。
    pub async fn delete(&self, proxy: &Proxy) -> PoolResult<bool> {
        Ok(self.db.delete(&proxy.proxy).await? > 0)
    }

    /// 批量删除, 返回实际删除数量。
    ///
    /// # Errors
    ///
    /// Redis 删除失败。
    pub async fn delete_many(&self, proxies: &[String]) -> PoolResult<usize> {
        self.db.delete_many(proxies).await
    }

    /// 是否存在指定代理。
    ///
    /// # Errors
    ///
    /// Redis 查询失败。
    pub async fn exists(&self, proxy: &Proxy) -> PoolResult<bool> {
        self.db.exists(&proxy.proxy).await
    }

    /// 全量代理列表。
    ///
    /// # Errors
    ///
    /// Redis 查询失败; 单条坏数据记 warn 跳过。
    pub async fn get_all(&self, https: bool) -> PoolResult<Vec<Proxy>> {
        Ok(self
            .db
            .get_all(https)
            .await?
            .iter()
            .filter_map(|j| parse_or_none(j))
            .collect())
    }

    /// 全量代理 key 列表(不解析 value, 供采集预过滤)。
    ///
    /// # Errors
    ///
    /// Redis 查询失败。
    pub async fn get_all_keys(&self) -> PoolResult<Vec<String>> {
        self.db.get_all_keys().await
    }

    /// 池子统计。
    ///
    /// # Errors
    ///
    /// Redis 查询失败。
    pub async fn get_count(&self) -> PoolResult<PoolCount> {
        self.db.get_count().await
    }
}

/// 解析单条代理 JSON; 坏数据记 warn 返回 None(不因个别脏数据中断整个批次)。
fn parse_or_none(json: &str) -> Option<Proxy> {
    match Proxy::from_json(json) {
        Ok(p) => Some(p),
        Err(e) => {
            warn!(error = %e, json = %truncate_for_log(json), "代理数据解析失败, 跳过");
            None
        }
    }
}

/// 日志中截断超长 JSON, 防止坏数据刷屏。
fn truncate_for_log(json: &str) -> &str {
    // 按字符边界截断, 避免切在多字节字符中间
    match json.char_indices().nth(120) {
        Some((idx, _)) => &json[..idx],
        None => json,
    }
}

/// 运行时装配: 加载配置(环境变量 > setting.toml > 默认值) → 建立 Redis 连接 →
/// 自检 → 构造处理器。schedule 与 server 两个入口共用; 配置或 DB 不可达时
/// 直接返回错误(对齐 Python launcher 的快速失败)。
///
/// # Errors
///
/// 配置文件非法、DB 连不上或 PING 失败。
pub async fn bootstrap() -> PoolResult<(Arc<Settings>, Arc<ProxyHandler>)> {
    let conf = Arc::new(Settings::load()?);
    let db = RedisClient::new(&conf.db_conn).await?;
    db.ping().await?;
    let handler = ProxyHandler::with_db(conf.clone(), db)?;
    Ok((conf, Arc::new(handler)))
}
