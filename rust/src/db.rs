//! Redis 存储层: hash 结构, key 为 `ip:port`, value 为 [`Proxy`](crate::proxy::Proxy)
//! 的 JSON。语义对齐 Python 版 `db/redisClient.py`。
//! 仅实现 Redis(SSDB 支持已移除, 见 README)。

use rand::seq::IteratorRandom;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde_json::Value;

use crate::errors::PoolResult;
use crate::proxy::Proxy;

/// 池子统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolCount {
    /// 总数
    pub total: usize,
    /// 其中支持 https 的数量
    pub https: usize,
}

/// Redis 客户端封装。
///
/// 所有方法 `&self`: 内部克隆 [`ConnectionManager`] (廉价, 共享底层连接),
/// 因此可放在 `Arc` 中被多任务并发使用。
pub struct RedisClient {
    conn: ConnectionManager,
    table: String,
}

impl RedisClient {
    /// 建立连接。连接串兼容 Python 版默认值 `redis://:@host:port/db`(空凭据会被剥离)。
    ///
    /// # Errors
    ///
    /// 连接串非法或连不上 Redis 时返回 [`PoolError::Redis`]。
    ///
    /// ```no_run
    /// use proxy_pool::db::RedisClient;
    ///
    /// # async fn demo() -> Result<(), proxy_pool::errors::PoolError> {
    /// let db = RedisClient::new("redis://:@127.0.0.1:6379/0").await?;
    /// db.ping().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(db_conn: &str) -> PoolResult<Self> {
        let conn_url = normalize_conn(db_conn);
        let client = redis::Client::open(conn_url.as_str())?;
        let conn = client.get_connection_manager().await?;
        Ok(Self {
            conn,
            table: String::new(),
        })
    }

    /// 切换操作的 hash 表名(对齐 Python changeTable)。
    pub fn change_table(&mut self, table: &str) {
        self.table = table.to_string();
    }

    /// 当前表名。
    pub fn table(&self) -> &str {
        &self.table
    }

    /// 连接自检: PING。
    ///
    /// # Errors
    ///
    /// Redis 不可达或认证失败。
    pub async fn ping(&self) -> PoolResult<()> {
        let mut conn = self.conn.clone();
        redis::cmd("PING").query_async::<String>(&mut conn).await?;
        Ok(())
    }

    /// 随机取一个代理的 JSON; `https=true` 时只取支持 https 的(对齐 Python get)。
    ///
    /// # Errors
    ///
    /// Redis 读写失败。
    pub async fn get(&self, https: bool) -> PoolResult<Option<String>> {
        let mut conn = self.conn.clone();
        if https {
            let items: Vec<String> = conn.hvals(&self.table).await?;
            // ThreadRng 不是 Send: 随机选择收进独立作用域, 保证 rng 在后续 await 前释放
            let picked = {
                let mut rng = rand::thread_rng();
                items
                    .iter()
                    .filter(|j| json_https_true(j))
                    .choose(&mut rng)
                    .cloned()
            };
            Ok(picked)
        } else {
            let keys: Vec<String> = conn.hkeys(&self.table).await?;
            let picked = {
                let mut rng = rand::thread_rng();
                keys.iter().choose(&mut rng).cloned()
            };
            match picked {
                Some(key) => Ok(conn.hget(&self.table, key).await?),
                None => Ok(None),
            }
        }
    }

    /// 写入代理(hset 覆盖式)。
    ///
    /// # Errors
    ///
    /// Redis 写入失败。
    pub async fn put(&self, proxy: &Proxy) -> PoolResult<()> {
        let mut conn = self.conn.clone();
        let _: i64 = conn.hset(&self.table, &proxy.proxy, proxy.to_json()).await?;
        Ok(())
    }

    /// 仅当代理不存在时写入(HSETNX 原子操作), 返回是否发生了写入。
    ///
    /// 新采集代理入库用: 存在即保留原记录(来源列表/历史计数不被覆盖),
    /// 与 Python raw 分支 `exists → put` 语义一致, 但单次往返且无竞态窗口。
    ///
    /// # Errors
    ///
    /// Redis 写入失败。
    pub async fn put_if_absent(&self, proxy: &Proxy) -> PoolResult<bool> {
        let mut conn = self.conn.clone();
        Ok(conn.hset_nx(&self.table, &proxy.proxy, proxy.to_json()).await?)
    }

    /// 取出并删除一个代理(对齐 Python pop)。
    ///
    /// # Errors
    ///
    /// Redis 读写失败。
    pub async fn pop(&self, https: bool) -> PoolResult<Option<String>> {
        let Some(json) = self.get(https).await? else {
            return Ok(None);
        };
        let key = serde_json::from_str::<Value>(&json)
            .ok()
            .and_then(|v| v.get("proxy").and_then(Value::as_str).map(String::from));
        if let Some(key) = key {
            self.delete(&key).await?;
        }
        Ok(Some(json))
    }

    /// 删除指定代理, 返回删除数量(0/1)。
    ///
    /// # Errors
    ///
    /// Redis 删除失败。
    pub async fn delete(&self, key: &str) -> PoolResult<usize> {
        let mut conn = self.conn.clone();
        let n: i64 = conn.hdel(&self.table, key).await?;
        Ok(n.max(0) as usize)
    }

    /// 批量删除, 返回实际删除数量。
    ///
    /// # Errors
    ///
    /// Redis 删除失败。
    pub async fn delete_many(&self, keys: &[String]) -> PoolResult<usize> {
        if keys.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn.clone();
        let n: i64 = conn.hdel(&self.table, keys).await?;
        Ok(n.max(0) as usize)
    }

    /// 判断代理是否存在。
    ///
    /// # Errors
    ///
    /// Redis 查询失败。
    pub async fn exists(&self, key: &str) -> PoolResult<bool> {
        let mut conn = self.conn.clone();
        Ok(conn.hexists(&self.table, key).await?)
    }

    /// 全量代理 JSON 列表; `https=true` 时只取支持 https 的。
    ///
    /// # Errors
    ///
    /// Redis 查询失败。
    pub async fn get_all(&self, https: bool) -> PoolResult<Vec<String>> {
        let mut conn = self.conn.clone();
        let items: Vec<String> = conn.hvals(&self.table).await?;
        if https {
            Ok(items.into_iter().filter(|j| json_https_true(j)).collect())
        } else {
            Ok(items)
        }
    }

    /// 池子统计(总量与 https 数, 对齐 Python getCount)。
    ///
    /// # Errors
    ///
    /// Redis 查询失败。
    pub async fn get_count(&self) -> PoolResult<PoolCount> {
        let items = self.get_all(false).await?;
        let https = items.iter().filter(|j| json_https_true(j)).count();
        Ok(PoolCount {
            total: items.len(),
            https,
        })
    }
}

/// 剥离连接串中的空凭据: redis-rs 会把 `redis://:@host` 的空密码当作需要 AUTH,
/// 而无密码的 Redis 会拒绝 AUTH; Python redis-py 对空串不做 AUTH, 这里对齐其行为。
fn normalize_conn(conn: &str) -> String {
    conn.replace(":@", "@").replace("//@", "//")
}

/// 判断代理 JSON 的 https 字段是否为 true(坏数据按 false 处理)。
fn json_https_true(json: &str) -> bool {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| v.get("https").and_then(Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_empty_credentials() {
        assert_eq!(
            normalize_conn("redis://:@127.0.0.1:6379/0"),
            "redis://127.0.0.1:6379/0"
        );
        assert_eq!(
            normalize_conn("redis://@proxy_redis:6379/0"),
            "redis://proxy_redis:6379/0"
        );
        // 带真实密码的连接串不能被误伤
        assert_eq!(
            normalize_conn("redis://:pwd@127.0.0.1:6379/0"),
            "redis://:pwd@127.0.0.1:6379/0"
        );
    }
}
