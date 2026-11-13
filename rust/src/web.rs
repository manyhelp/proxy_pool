//! 抓取用 HTTP 访问封装: 直连优先, 失败自动改用 `FETCH_PROXY` 重试。
//! 行为对齐 Python 版 `util/webRequest.py` 的 get(), 差异见 [`get_text`]。

use std::sync::OnceLock;
use std::time::Duration;

use rand::seq::SliceRandom;
use tracing::warn;

use crate::settings::Settings;

/// 轮换 UA 列表, 原样保留 Python 版取值以维持相近的请求指纹
const UA_LIST: [&str; 8] = [
    "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/30.0.1599.101",
    "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/38.0.2125.122",
    "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/39.0.2171.71",
    "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/39.0.2171.95",
    "Mozilla/5.0 (Windows NT 6.1; WOW64) AppleWebKit/537.1 (KHTML, like Gecko) Chrome/21.0.1180.71",
    "Mozilla/4.0 (compatible; MSIE 6.0; Windows NT 5.1; SV1; QQDownload 732; .NET4.0C; .NET4.0E)",
    "Mozilla/5.0 (Windows NT 5.1; U; en; rv:1.8.1) Gecko/20061208 Firefox/2.0.0 Opera 9.50",
    "Mozilla/5.0 (Windows NT 6.1; WOW64; rv:34.0) Gecko/20100101 Firefox/34.0",
];

/// 直连客户端: 进程级共享连接池, 超时按次传入。
fn direct_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .build()
            .expect("无定制配置, 客户端构建不会失败")
    })
}

/// `FETCH_PROXY` 回退客户端。FETCH_PROXY 是进程级常量, 因此只构建一次;
/// 格式非法会在首次使用时 panic, 消息指明是配置问题。
fn fetch_proxy_client(fetch_proxy: &str) -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .proxy(
                reqwest::Proxy::all(fetch_proxy)
                    .expect("FETCH_PROXY 格式非法, 示例: http://ip:port"),
            )
            .build()
            .expect("无定制配置, 客户端构建不会失败")
    })
}

/// 单次 GET: 严格 200 视为成功, 返回响应文本。
///
/// 非 UTF-8 内容走 lossy 解码: 代理源(gb2312 等)中 IP/端口均为 ASCII, 不影响抽取。
async fn request_once(
    client: &reqwest::Client,
    url: &str,
    timeout_secs: u64,
    extra_headers: &[(&str, &str)],
) -> Option<String> {
    let ua = UA_LIST.choose(&mut rand::thread_rng()).expect("UA_LIST 非空");
    let mut req = client
        .get(url)
        .timeout(Duration::from_secs(timeout_secs))
        .header(reqwest::header::USER_AGENT, *ua);
    for (k, v) in extra_headers {
        req = req.header(*k, *v);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            warn!(url, error = %e, "request failed");
            return None;
        }
    };
    if resp.status() != reqwest::StatusCode::OK {
        warn!(url, status = resp.status().as_u16(), "unexpected status");
        return None;
    }
    match resp.bytes().await {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) => {
            warn!(url, error = %e, "read body failed");
            None
        }
    }
}

async fn get_text_inner(
    url: &str,
    conf: &Settings,
    timeout_secs: u64,
    extra_headers: &[(&str, &str)],
) -> Option<String> {
    // 配置了 FETCH_PROXY 时直连只试 1 次快速失败, 再经代理重试; 未配置则直连重试 3 次。
    // 重试间隔 5 秒, 对齐 Python WebRequest。
    let direct_attempts = if conf.fetch_proxy.is_empty() { 3 } else { 1 };
    for attempt in 0..direct_attempts {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        if let Some(text) = request_once(direct_client(), url, timeout_secs, extra_headers).await {
            return Some(text);
        }
    }
    if !conf.fetch_proxy.is_empty() {
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            if let Some(text) =
                request_once(fetch_proxy_client(&conf.fetch_proxy), url, timeout_secs, extra_headers)
                    .await
            {
                return Some(text);
            }
        }
    }
    warn!(url, "direct 与 FETCH_PROXY 均失败");
    None
}

/// 抓取 URL 文本。
///
/// 与 Python 版的差异: 全部失败返回 `None`(Python 会伪造 200 空响应),
/// 调用方据此明确跳过本轮解析。
///
/// # Examples
///
/// ```no_run
/// use proxy_pool::settings::Settings;
/// use proxy_pool::web;
///
/// # async fn demo() {
/// let conf = Settings::load().expect("配置加载失败");
/// let text = web::get_text("https://example.com/list.txt", &conf, 30).await;
/// # }
/// ```
pub async fn get_text(url: &str, conf: &Settings, timeout_secs: u64) -> Option<String> {
    get_text_inner(url, conf, timeout_secs, &[]).await
}

/// 同 [`get_text`], 附带额外请求头(如 89IP 需要 Referer 过风控)。
pub async fn get_text_with_headers(
    url: &str,
    conf: &Settings,
    timeout_secs: u64,
    extra_headers: &[(&str, &str)],
) -> Option<String> {
    get_text_inner(url, conf, timeout_secs, extra_headers).await
}

/// 抓取并解析 JSON; 抓取失败或 JSON 非法均返回 `None`。
pub async fn get_json(url: &str, conf: &Settings, timeout_secs: u64) -> Option<serde_json::Value> {
    let text = get_text(url, conf, timeout_secs).await?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            warn!(url, error = %e, "json parse failed");
            None
        }
    }
}
