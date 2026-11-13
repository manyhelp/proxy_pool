//! 运行配置: 三层优先级 — **环境变量 > setting.toml > 内置默认值**。
//! 内置默认值与 Python 版 `setting.py` 一一对应。
//!
//! setting.toml 查找顺序(找到第一个即停):
//! 1. `SETTING` 环境变量指定的路径(指定了但不存在 → 启动报错, 不静默跳过);
//! 2. 当前工作目录下的 `setting.toml`;
//! 3. exe 同目录下的 `setting.toml`。
//!
//! 环境变量清单(覆盖文件中的同名项):
//!
//! | 环境变量 | 默认值 | 说明 |
//! |---|---|---|
//! | `HOST` / `PORT` | `0.0.0.0` / `5010` | API 监听地址 |
//! | `DB_CONN` | `redis://:@127.0.0.1:6379/0` | Redis 连接串 |
//! | `TABLE_NAME` | `use_proxy` | Redis hash 表名 |
//! | `PROXY_FETCHER` | 全部 20 个源 | 逗号分隔的抓取源名单 |
//! | `FETCH_PROXY` | 与 Python setting.py 同值 | 抓取直连失败回退代理, 置空关闭 |
//! | `HTTP_URL` / `HTTPS_URL` | 东方财富页面/接口 | 校验目标 |
//! | `VERIFY_TIMEOUT` | `5` | 校验超时(秒) |
//! | `MAX_FAIL_COUNT` | `0` | 失败次数上限(0 = 一次失败即淘汰) |
//! | `POOL_SIZE_MIN` | `200` | 低于此值时 check 触发补采 |
//! | `PROXY_REGION` | `false` | 是否查询代理地域 |
//! | `CHECK_CONCURRENCY` | `100` | 单轮校验并发数(Rust 版新增) |
//! | `SETTING` | 无 | 显式指定 setting.toml 路径 |
//! | `RUST_LOG` | `info` | 日志级别 |

use std::env;
use std::path::PathBuf;

use serde::Deserialize;

use crate::errors::{PoolError, PoolResult};

/// 当前版本号(与 Python 版版本号无关)
pub const VERSION: &str = "3.0.0";

/// 抓取源全集, 与 setting.py 的 PROXY_FETCHER 列表一致
pub const DEFAULT_FETCHERS: [&str; 20] = [
    "freeProxy01", "freeProxy02", "freeProxy03", "freeProxy04", "freeProxy05",
    "freeProxy06", "freeProxy07", "freeProxy08", "freeProxy09", "freeProxy10",
    "freeProxy11", "freeProxy12", "freeProxy13", "freeProxy14", "freeProxy15",
    "freeProxy16", "freeProxy17", "freeProxy18", "freeProxy19", "freeProxy20",
];

/// setting.toml 的反序列化载体: 全字段可选(缺项走默认值), 未知字段忽略。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    host: Option<String>,
    port: Option<u16>,
    db_conn: Option<String>,
    table_name: Option<String>,
    proxy_fetcher: Option<Vec<String>>,
    fetch_proxy: Option<String>,
    http_url: Option<String>,
    https_url: Option<String>,
    verify_timeout: Option<u64>,
    max_fail_count: Option<i64>,
    pool_size_min: Option<usize>,
    proxy_region: Option<bool>,
    check_concurrency: Option<usize>,
}

/// 进程级运行配置。
#[derive(Debug, Clone)]
pub struct Settings {
    /// API 监听地址
    pub host: String,
    /// API 监听端口
    pub port: u16,
    /// Redis 连接串
    pub db_conn: String,
    /// Redis hash 表名
    pub table_name: String,
    /// 启用的抓取源名单
    pub proxy_fetcher: Vec<String>,
    /// 抓取源直连失败时的回退代理, 空串表示关闭
    pub fetch_proxy: String,
    /// http 校验目标 URL
    pub http_url: String,
    /// https 校验目标 URL
    pub https_url: String,
    /// 单次校验超时(秒)
    pub verify_timeout: u64,
    /// 最大失败次数(0 = 一次校验失败即淘汰)
    pub max_fail_count: i64,
    /// 池子低于该值时 check 任务先触发一轮补采
    pub pool_size_min: usize,
    /// 是否查询并记录代理地域
    pub proxy_region: bool,
    /// 单轮校验的并发上限
    pub check_concurrency: usize,
    /// 实际加载的配置文件路径(None = 未找到文件, 全部走默认/环境变量)
    pub config_file: Option<PathBuf>,
}

impl Settings {
    /// 加载配置: 环境变量 > setting.toml > 内置默认值。
    ///
    /// # Errors
    ///
    /// `SETTING` 指定的文件不存在、文件读不到或 TOML 内容非法时返回错误
    /// (配置错误必须 fail fast, 不能静默回退默认值)。
    ///
    /// # Examples
    ///
    /// ```
    /// use proxy_pool::settings::Settings;
    ///
    /// let conf = Settings::load().unwrap();
    /// assert!(!conf.table_name.is_empty());
    /// assert!(conf.pool_size_min > 0);
    /// ```
    pub fn load() -> PoolResult<Self> {
        let file = read_config_file()?;
        Ok(Self::from_layers(|k| env::var(k).ok(), file))
    }

    /// 三层合并的纯逻辑, `env_get` 抽出来是为了可测(真实环境变量全局可变, 测试不可靠)。
    fn from_layers(
        get_env: impl Fn(&str) -> Option<String>,
        file: Option<(PathBuf, FileConfig)>,
    ) -> Self {
        let (config_file, f) = match file {
            Some((path, cfg)) => (Some(path), cfg),
            None => (None, FileConfig::default()),
        };
        let s = |k: &str| get_env(k);
        Self {
            host: s("HOST").or(f.host).unwrap_or_else(|| "0.0.0.0".into()),
            port: s("PORT")
                .and_then(|v| v.trim().parse().ok())
                .or(f.port)
                .unwrap_or(5010),
            db_conn: s("DB_CONN")
                .or(f.db_conn)
                .unwrap_or_else(|| "redis://:@127.0.0.1:6379/0".into()),
            table_name: s("TABLE_NAME")
                .or(f.table_name)
                .unwrap_or_else(|| "use_proxy".into()),
            proxy_fetcher: match s("PROXY_FETCHER") {
                Some(raw) => parse_fetchers(Some(&raw)),
                None => f.proxy_fetcher.unwrap_or_else(|| {
                    DEFAULT_FETCHERS.iter().map(|s| s.to_string()).collect()
                }),
            },
            // 对齐 setting.py 的现网取值; 其他机器部署请显式设置或置空关闭
            fetch_proxy: s("FETCH_PROXY")
                .or(f.fetch_proxy)
                .unwrap_or_else(|| "http://100.72.93.13:11808".into()),
            http_url: s("HTTP_URL").or(f.http_url).unwrap_or_else(|| {
                "http://quote.eastmoney.com/concept/sh603777.html?from=classic".into()
            }),
            https_url: s("HTTPS_URL").or(f.https_url).unwrap_or_else(|| {
                "https://push2his.eastmoney.com/api/qt/stock/trends2/get?secid=1.510050&ndays=1&fields1=f1&fields2=f51".into()
            }),
            verify_timeout: s("VERIFY_TIMEOUT")
                .and_then(|v| v.trim().parse().ok())
                .or(f.verify_timeout)
                .unwrap_or(5),
            max_fail_count: s("MAX_FAIL_COUNT")
                .and_then(|v| v.trim().parse().ok())
                .or(f.max_fail_count)
                .unwrap_or(0),
            pool_size_min: s("POOL_SIZE_MIN")
                .and_then(|v| v.trim().parse().ok())
                .or(f.pool_size_min)
                .unwrap_or(200),
            // 与 Python bool(os.getenv(..)) 语义不同(那里 "False" 也是 True), 有意修正
            proxy_region: s("PROXY_REGION")
                .map(|v| matches!(v.trim().to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
                .or(f.proxy_region)
                .unwrap_or(false),
            check_concurrency: s("CHECK_CONCURRENCY")
                .and_then(|v| v.trim().parse().ok())
                .or(f.check_concurrency)
                .unwrap_or(100),
            config_file,
        }
    }
}

/// 解析 PROXY_FETCHER: 未设置用全集; 设置了按逗号拆分, 容忍空项。
fn parse_fetchers(raw: Option<&str>) -> Vec<String> {
    match raw {
        None => DEFAULT_FETCHERS.iter().map(|s| s.to_string()).collect(),
        Some(s) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
    }
}

/// 定位配置文件: `SETTING` > ./setting.toml > exe 同目录 setting.toml。
fn locate_config_file() -> PoolResult<Option<PathBuf>> {
    if let Ok(p) = env::var("SETTING") {
        let path = PathBuf::from(&p);
        if !path.is_file() {
            // 用户显式指定的路径缺文件属于配置错误, 不能静默当没配置
            return Err(PoolError::Other(format!(
                "SETTING 指定的配置文件不存在: {p}"
            )));
        }
        return Ok(Some(path));
    }
    let cwd = PathBuf::from("setting.toml");
    if cwd.is_file() {
        return Ok(Some(cwd));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("setting.toml");
            if p.is_file() {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}

/// 读取并解析配置文件; 返回(路径, 内容)。
fn read_config_file() -> PoolResult<Option<(PathBuf, FileConfig)>> {
    let Some(path) = locate_config_file()? else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| PoolError::Other(format!("读取配置文件 {} 失败: {e}", path.display())))?;
    let cfg: FileConfig = toml::from_str(&text)
        .map_err(|e| PoolError::Other(format!("解析配置文件 {} 失败: {e}", path.display())))?;
    Ok(Some((path, cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无环境变量、无文件的空环境
    fn no_env(_key: &str) -> Option<String> {
        None
    }

    #[test]
    fn layering_env_over_file_over_defaults() {
        let f: FileConfig = toml::from_str("port = 6000\npool_size_min = 50").unwrap();
        let s = Settings::from_layers(
            |k| (k == "HOST").then(|| "127.0.0.1".to_string()),
            Some((PathBuf::from("t.toml"), f)),
        );
        assert_eq!(s.host, "127.0.0.1"); // env 覆盖文件
        assert_eq!(s.port, 6000); // 文件覆盖默认
        assert_eq!(s.pool_size_min, 50);
        assert_eq!(s.table_name, "use_proxy"); // 三层都缺 → 默认
        assert_eq!(s.config_file, Some(PathBuf::from("t.toml")));
    }

    #[test]
    fn no_file_no_env_gives_pure_defaults() {
        let s = Settings::from_layers(no_env, None);
        assert_eq!(s.port, 5010);
        assert_eq!(s.fetch_proxy, "http://100.72.93.13:11808");
        assert_eq!(s.proxy_fetcher.len(), 20);
        assert!(s.config_file.is_none());
    }

    #[test]
    fn file_can_disable_fetch_proxy_with_empty_string() {
        let f: FileConfig = toml::from_str("fetch_proxy = \"\"").unwrap();
        let s = Settings::from_layers(no_env, Some((PathBuf::from("t.toml"), f)));
        assert_eq!(s.fetch_proxy, ""); // 显式置空生效(关闭回退)
    }

    #[test]
    fn fetchers_coming_from_file_list() {
        let f: FileConfig = toml::from_str("proxy_fetcher = [\"freeProxy01\", \"freeProxy12\"]")
            .unwrap();
        let s = Settings::from_layers(no_env, Some((PathBuf::from("t.toml"), f)));
        assert_eq!(s.proxy_fetcher, vec!["freeProxy01", "freeProxy12"]);
    }

    #[test]
    fn env_fetcher_list_wins_over_file() {
        let f: FileConfig = toml::from_str("proxy_fetcher = [\"freeProxy01\"]").unwrap();
        let s = Settings::from_layers(
            |k| (k == "PROXY_FETCHER").then(|| "freeProxy20,freeProxy01".to_string()),
            Some((PathBuf::from("t.toml"), f)),
        );
        assert_eq!(s.proxy_fetcher, vec!["freeProxy20", "freeProxy01"]);
    }

    #[test]
    fn unknown_toml_keys_are_ignored() {
        let f: FileConfig = toml::from_str("whatever = 1\nport = 6000").unwrap();
        assert_eq!(f.port, Some(6000));
    }

    #[test]
    fn fetchers_env_list_trims_and_skips_empty() {
        assert_eq!(
            parse_fetchers(Some(" freeProxy01 ,, freeProxy20 ")),
            vec!["freeProxy01", "freeProxy20"]
        );
        assert!(parse_fetchers(Some("")).is_empty());
        assert_eq!(parse_fetchers(None).len(), 20);
    }
}
