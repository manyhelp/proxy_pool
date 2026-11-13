//! 代理模型与格式校验。
//!
//! 字段名与 JSON 结构同 Python 版 `helper/proxy.py` 完全一致, 存入 Redis 的 value
//! 可被两版互相解析(这是灰度并存的前提), 因此不得增删/改名任何字段。

use std::sync::OnceLock;

use regex::Regex;
use serde::de::{Deserializer, Error as DeError, Visitor};
use serde::{Deserialize, Serialize};

/// 代理对象, JSON 序列化后作为 Redis hash value 存储。
///
/// # Examples
///
/// ```
/// use proxy_pool::proxy::Proxy;
///
/// let mut p = Proxy::new("1.2.3.4:80");
/// p.add_source("freeProxy01");
/// p.add_source("freeProxy05");
/// p.add_source("freeProxy01"); // 重复来源不追加
/// assert_eq!(p.source, "freeProxy01/freeProxy05");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Proxy {
    /// 代理地址 `ip:port`(可带 `user:pass@` 前缀), 同时是 Redis hash key
    pub proxy: String,
    /// 连续校验失败次数, 成功一轮减一(不低于 0)
    #[serde(default)]
    pub fail_count: i64,
    /// 地域(国家/省/市), 仅 `PROXY_REGION` 开启时填写
    #[serde(default)]
    pub region: String,
    /// 匿名度(预留字段, Python 版未实现)
    #[serde(default)]
    pub anonymous: String,
    /// 来源抓取源名, 多个以 `/` 连接
    #[serde(default)]
    pub source: String,
    /// 累计校验次数
    #[serde(default)]
    pub check_count: i64,
    /// 最近一轮校验是否通过
    #[serde(default, deserialize_with = "de_bool_lenient")]
    pub last_status: bool,
    /// 最近一轮校验时间, 本地时间 `%Y-%m-%d %H:%M:%S`
    #[serde(default)]
    pub last_time: String,
    /// 是否支持 https(经 HTTP 代理 CONNECT 隧道验证)
    #[serde(default, deserialize_with = "de_bool_lenient")]
    pub https: bool,
}

impl Proxy {
    /// 以默认属性创建代理(除地址外全部归零/置空)。
    ///
    /// # Examples
    ///
    /// ```
    /// use proxy_pool::proxy::Proxy;
    ///
    /// let p = Proxy::new("1.2.3.4:80");
    /// assert_eq!(p.fail_count, 0);
    /// assert!(!p.https);
    /// ```
    pub fn new(proxy: impl Into<String>) -> Self {
        Self {
            proxy: proxy.into(),
            fail_count: 0,
            region: String::new(),
            anonymous: String::new(),
            source: String::new(),
            check_count: 0,
            last_status: false,
            last_time: String::new(),
            https: false,
        }
    }

    /// 从 JSON 反序列化(兼容 Python 版写入的 value)。
    ///
    /// # Errors
    ///
    /// JSON 非法或字段类型不兼容时返回 [`serde_json::Error`]。
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// 序列化为 JSON(即存入 Redis 的形态)。
    ///
    /// # Panics
    ///
    /// 不可能失败: 全部字段为字符串/数值/布尔基本类型; `expect` 仅用于绕过 Result 签名。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Proxy 字段均为基本类型, 序列化不会失败")
    }

    /// 追加来源, 幂等去重。
    ///
    /// 与 Python 版的差异: Python 用 `set` 去重导致顺序不稳定, 这里保持插入序。
    pub fn add_source(&mut self, source: &str) {
        if source.is_empty() {
            return;
        }
        let mut list: Vec<&str> = self.source.split('/').filter(|s| !s.is_empty()).collect();
        if !list.contains(&source) {
            list.push(source);
        }
        self.source = list.join("/");
    }

    /// 代理格式校验: 可选 `user:pass@` 前缀 + IPv4:port, 全串匹配
    /// (对齐 Python preValidator 的 `IP_REGEX.fullmatch`)。
    ///
    /// # Examples
    ///
    /// ```
    /// use proxy_pool::proxy::Proxy;
    ///
    /// assert!(Proxy::is_valid_format("1.2.3.4:80"));
    /// assert!(Proxy::is_valid_format("user:pass@1.2.3.4:8080"));
    /// assert!(!Proxy::is_valid_format("1.2.3.4"));
    /// assert!(!Proxy::is_valid_format("1.2.3.4:123456"));
    /// ```
    pub fn is_valid_format(proxy: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| {
            Regex::new(r"^(.*:.*@)?\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d{1,5}$")
                .expect("内置正则, 编译失败属于程序错误")
        })
        .is_match(proxy)
    }
}

/// 宽松布尔反序列化: 兼容 Python 端可能写入的 `"True"`/`""` 等历史值。
fn de_bool_lenient<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoolVisitor;

    impl<'de> Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bool 或可判定真伪的字符串/数字")
        }

        fn visit_bool<E: DeError>(self, v: bool) -> Result<bool, E> {
            Ok(v)
        }

        fn visit_u64<E: DeError>(self, v: u64) -> Result<bool, E> {
            Ok(v != 0)
        }

        fn visit_str<E: DeError>(self, v: &str) -> Result<bool, E> {
            Ok(matches!(v.trim().to_lowercase().as_str(), "true" | "1"))
        }
    }

    deserializer.deserialize_any(BoolVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python 版 json.dumps(ensure_ascii=False) 产出的真实形态
    const PY_JSON: &str = r#"{"proxy": "1.2.3.4:80", "https": true, "fail_count": 0, "region": "中国/广东/深圳", "anonymous": "", "source": "freeProxy01/freeProxy05", "check_count": 3, "last_status": true, "last_time": "2026-11-14 12:00:00"}"#;

    #[test]
    fn parses_python_generated_json() {
        let p = Proxy::from_json(PY_JSON).unwrap();
        assert_eq!(p.proxy, "1.2.3.4:80");
        assert!(p.https);
        assert!(p.last_status);
        assert_eq!(p.check_count, 3);
        assert_eq!(p.region, "中国/广东/深圳");
    }

    #[test]
    fn json_roundtrip_preserves_all_fields() {
        let p = Proxy::from_json(PY_JSON).unwrap();
        let p2 = Proxy::from_json(&p.to_json()).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn lenient_bool_accepts_python_edge_values() {
        let p = Proxy::from_json(r#"{"proxy": "1.2.3.4:80", "last_status": "", "https": "True"}"#)
            .unwrap();
        assert!(!p.last_status);
        assert!(p.https);
    }

    #[test]
    fn missing_fields_default() {
        let p = Proxy::from_json(r#"{"proxy": "1.2.3.4:80"}"#).unwrap();
        assert_eq!(p.fail_count, 0);
        assert!(!p.https);
        assert_eq!(p.source, "");
    }

    #[test]
    fn add_source_dedups_and_keeps_order() {
        let mut p = Proxy::new("1.2.3.4:80");
        p.add_source("freeProxy05");
        p.add_source("freeProxy01");
        p.add_source("freeProxy05");
        p.add_source("");
        assert_eq!(p.source, "freeProxy05/freeProxy01");
    }

    #[test]
    fn format_validation_matches_python_semantics() {
        for ok in ["1.2.3.4:80", "user:pass@1.2.3.4:8080", "255.255.255.255:65535"] {
            assert!(Proxy::is_valid_format(ok), "{ok} 应通过");
        }
        for bad in ["1.2.3.4", "1.2.3.4:", "1.2.3.4:123456", "abc", "1.2.3.4:80 extra"] {
            assert!(!Proxy::is_valid_format(bad), "{bad} 应拒绝");
        }
    }
}
