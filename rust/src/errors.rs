//! 统一错误类型: 库内所有可失败操作返回 [`PoolResult`]。

use thiserror::Error;

/// 代理池运行期错误聚合。
#[derive(Debug, Error)]
pub enum PoolError {
    /// Redis 读写失败(连接断开、命令错误等)
    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),
    /// HTTP 客户端构建或发送失败
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    /// 本地 IO 失败(端口绑定等)
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// 其余无法归类的错误
    #[error("{0}")]
    Other(String),
}

/// 库内统一 Result 别名。
pub type PoolResult<T> = Result<T, PoolError>;
