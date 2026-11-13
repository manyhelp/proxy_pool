//! 代理可用性校验: 目标与判定语义对齐 Python 版 helper/validator.py + helper/check.py。
//!
//! - http 校验: HEAD [`Settings::http_url`], 经 HTTP 代理, 不跟随重定向, 严格 200 通过;
//! - https 校验: HEAD + GET [`Settings::https_url`], 走 HTTP 代理 CONNECT 隧道,
//!   不校验目标证书(对应 Python `verify=False`), 全部 200 才判定支持 https;
//! - 成功减一次 `fail_count`(不低于 0), 失败加一; 淘汰判定(`MAX_FAIL_COUNT`)在
//!   [`crate::checker`] 中, 与 Python `__ifUse` 一致。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::errors::PoolResult;
use crate::proxy::Proxy;
use crate::settings::Settings;
use crate::web;

/// 校验批次类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 新采集待入库的代理(Fetcher 产出)
    Raw,
    /// 池内存量复检
    Use,
}

/// 校验请求 UA: 对齐 Python validator 的固定 Chrome UA
const VALIDATOR_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36";

/// 校验器。持共享配置; 每次校验按代理地址构建独立客户端
/// (代理地址烧在连接配置里, 无法跨代理复用)。
pub struct Validator {
    conf: Arc<Settings>,
}

impl Validator {
    /// 以共享配置构建。
    pub fn new(conf: Arc<Settings>) -> Self {
        Self { conf }
    }

    /// 为指定代理构建校验客户端。
    ///
    /// # Errors
    ///
    /// 代理地址无法构造为代理 URL(理论上已被格式校验拦截)或客户端构建失败。
    fn client(&self, proxy: &str, accept_invalid_certs: bool) -> PoolResult<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.conf.verify_timeout))
            // Python requests.head 默认不跟随重定向, 判定必须一致
            .redirect(reqwest::redirect::Policy::none())
            .proxy(reqwest::Proxy::all(format!("http://{proxy}"))?);
        if accept_invalid_certs {
            // 对应 Python verify=False: 校验的是"隧道可达", 不是证书链
            builder = builder.danger_accept_invalid_certs(true);
        }
        Ok(builder.build()?)
    }

    /// http 校验: HEAD `http_url`, 严格 200。
    pub async fn http_ok(&self, proxy: &str) -> bool {
        let client = match self.client(proxy, false) {
            Ok(c) => c,
            Err(_) => return false,
        };
        match with_headers(client.head(&self.conf.http_url)).send().await {
            Ok(resp) => resp.status() == reqwest::StatusCode::OK,
            // 超时/连接失败是常态路径且量大, 不逐次记日志
            Err(_) => false,
        }
    }

    /// https 校验: HEAD + GET `https_url` 均需 200
    /// (对齐 Python 注册了 HEAD/GET 两个 httpsValidator 且必须全过的语义)。
    pub async fn https_ok(&self, proxy: &str) -> bool {
        let client = match self.client(proxy, true) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let head_ok = match with_headers(client.head(&self.conf.https_url)).send().await {
            Ok(resp) => resp.status() == reqwest::StatusCode::OK,
            Err(_) => false,
        };
        if !head_ok {
            return false;
        }
        match with_headers(client.get(&self.conf.https_url)).send().await {
            Ok(resp) => resp.status() == reqwest::StatusCode::OK,
            Err(_) => false,
        }
    }

    /// 完成一轮校验并就地更新代理属性(对齐 Python DoValidator.validator):
    /// 成功时 `fail_count` 减一(不低于 0)并以本轮 https 结果刷新 `https` 标志;
    /// 失败时 `fail_count` 加一, `https` 标志保持原值。
    pub async fn validate(&self, proxy: &mut Proxy, mode: Mode) {
        let http_ok = self.http_ok(&proxy.proxy).await;
        let https_ok = if http_ok {
            self.https_ok(&proxy.proxy).await
        } else {
            false
        };

        proxy.check_count += 1;
        proxy.last_time = now_string();
        proxy.last_status = http_ok;
        if http_ok {
            if proxy.fail_count > 0 {
                proxy.fail_count -= 1;
            }
            proxy.https = https_ok;
            if mode == Mode::Raw && self.conf.proxy_region {
                proxy.region = self.region_of(&proxy.proxy).await;
            }
        } else {
            proxy.fail_count += 1;
        }
    }

    /// 地域查询(ip-api.com), 失败返回 "error"(对齐 Python regionGetter)。
    /// Python 版对 `user:pass@ip:port` 会误把用户名当 IP 查, 这里取最后一个 `@` 后的地址。
    async fn region_of(&self, proxy: &str) -> String {
        let host = proxy
            .rsplit('@')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");
        let url = format!("http://ip-api.com/json/{host}?lang=zh-CN");
        let value = web::get_text(&url, &self.conf, 2)
            .await
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        match value {
            Some(v) => format!(
                "{}/{}/{}",
                v.get("country").and_then(Value::as_str).unwrap_or(""),
                v.get("regionName").and_then(Value::as_str).unwrap_or(""),
                v.get("city").and_then(Value::as_str).unwrap_or("")
            ),
            None => "error".to_string(),
        }
    }
}

/// 校验请求头: 对齐 Python validator 的 UA/Accept/Accept-Language。
fn with_headers(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    builder
        .header("User-Agent", VALIDATOR_UA)
        .header("Accept", "*/*")
        .header("Accept-Language", "zh-CN,zh;q=0.9")
}

/// 本地时间字符串, 与 Python `datetime.now().strftime("%Y-%m-%d %H:%M:%S")` 同格式。
pub fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_string_format_matches_python() {
        let s = now_string();
        // 形如 2026-11-14 12:00:00
        assert_eq!(s.len(), 19);
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[10], b' ');
        assert_eq!(s.as_bytes()[13], b':');
        assert_eq!(s.as_bytes()[16], b':');
    }
}
