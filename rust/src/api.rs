//! WebApi(axum): 路由与响应结构对齐 Python 版 api/proxyApi.py,
//! JSON 字段名与 Python 完全一致, 消费方可无缝切换。
//!
//! 与 Python 的差异: `/get` 与 `/get/` 等斜杠变体都直接 200(Flask 会 308 重定向);
//! DB 异常返回 JSON 错误结构而非 500 HTML。

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tracing::{error, info};

use crate::errors::PoolResult;
use crate::handler::{bootstrap, ProxyHandler};
use crate::proxy::Proxy;
use crate::settings::{Settings, VERSION};

/// API 服务共享状态
struct AppState {
    handler: Arc<ProxyHandler>,
}

/// 启动 API 服务: 自行装配配置与 DB, 然后进入 [`run_with`]。
///
/// # Errors
///
/// 启动阶段配置文件非法或 DB 连不上。
///
/// # Examples
///
/// ```no_run
/// // 由 CLI 的 `proxy-pool server` 子命令调用, 需在 tokio 运行时内执行:
/// tokio::runtime::Runtime::new()
///     .unwrap()
///     .block_on(proxy_pool::api::run())
///     .unwrap();
/// ```
pub async fn run() -> PoolResult<()> {
    let (conf, handler) = bootstrap().await?;
    run_with(conf, handler).await
}

/// 启动 API 服务(使用已装配的组件, 供 [`crate::runner::run_all`] 复用), 阻塞至进程退出。
///
/// # Errors
///
/// 端口绑定失败。
pub async fn run_with(conf: Arc<Settings>, handler: Arc<ProxyHandler>) -> PoolResult<()> {
    info!(
        version = VERSION,
        host = conf.host,
        port = conf.port,
        config = conf.config_file.as_ref().map(|p| p.display().to_string())
            .unwrap_or_else(|| "内置默认(未找到 setting.toml)".into()),
        "ProxyPool api 启动"
    );

    let state = Arc::new(AppState { handler });

    // 斜杠与非斜杠路径都注册, 避免依赖客户端跟随 308 重定向
    let app = Router::new()
        .route("/", get(index))
        .route("/get", get(get_proxy))
        .route("/get/", get(get_proxy))
        .route("/pop", get(pop_proxy))
        .route("/pop/", get(pop_proxy))
        .route("/all", get(get_all))
        .route("/all/", get(get_all))
        .route("/count", get(get_count))
        .route("/count/", get(get_count))
        .route("/refresh", get(refresh))
        .route("/refresh/", get(refresh))
        .route("/delete", get(delete_one).post(delete_many_handler))
        .route("/delete/", get(delete_one).post(delete_many_handler))
        .with_state(state);

    let addr = format!("{}:{}", conf.host, conf.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr, "api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Json<Value> {
    Json(json!({"url": [
        {"url": "/get", "params": "type: ''https'|''", "desc": "get a proxy"},
        {"url": "/pop", "params": "", "desc": "get and delete a proxy"},
        {"url": "/delete", "params": "proxy: 'e.g. 127.0.0.1:8080'", "desc": "delete an unable proxy"},
        {"url": "/all", "params": "type: ''https'|''", "desc": "get all proxy from proxy pool"},
        {"url": "/count", "params": "", "desc": "return proxy count"}
    ]}))
}

/// `?type=https` 参数解析(对齐 Python: 其他取值一律视为不过滤)
fn https_flag(q: &HashMap<String, String>) -> bool {
    q.get("type").map(|s| s.to_lowercase() == "https").unwrap_or(false)
}

async fn get_proxy(
    State(st): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    match st.handler.get(https_flag(&q)).await {
        Ok(Some(p)) => Json(proxy_value(&p)),
        Ok(None) => Json(json!({"code": 0, "src": "no proxy"})),
        Err(e) => {
            error!(error = %e, "/get 查询失败");
            Json(json!({"code": 1, "src": "db error"}))
        }
    }
}

async fn pop_proxy(
    State(st): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    match st.handler.pop(https_flag(&q)).await {
        Ok(Some(p)) => Json(proxy_value(&p)),
        Ok(None) => Json(json!({"code": 0, "src": "no proxy"})),
        Err(e) => {
            error!(error = %e, "/pop 查询失败");
            Json(json!({"code": 1, "src": "db error"}))
        }
    }
}

async fn get_all(
    State(st): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Vec<Value>> {
    let proxies = match st.handler.get_all(https_flag(&q)).await {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "/all 查询失败");
            Vec::new()
        }
    };
    Json(proxies.iter().map(proxy_value).collect())
}

async fn get_count(State(st): State<Arc<AppState>>) -> Json<Value> {
    let proxies = match st.handler.get_all(false).await {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "/count 查询失败");
            Vec::new()
        }
    };
    let mut http_type: HashMap<&str, usize> = HashMap::new();
    let mut source: HashMap<&str, usize> = HashMap::new();
    for p in &proxies {
        let t = if p.https { "https" } else { "http" };
        *http_type.entry(t).or_insert(0) += 1;
        for s in p.source.split('/').filter(|s| !s.is_empty()) {
            *source.entry(s).or_insert(0) += 1;
        }
    }
    Json(json!({"http_type": http_type, "source": source, "count": proxies.len()}))
}

async fn refresh() -> &'static str {
    "success"
}

async fn delete_one(
    State(st): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let Some(proxy) = q.get("proxy") else {
        return Json(json!({"code": 1, "msg": "proxy or proxies parameter required"}));
    };
    let deleted = match st.handler.delete(&Proxy::new(proxy.clone())).await {
        Ok(n) => usize::from(n),
        Err(e) => {
            error!(error = %e, "删除失败");
            0
        }
    };
    Json(json!({"code": 0, "src": deleted}))
}

/// 批量删除: POST body 支持 JSON `{"proxies": [...]}` 或表单
/// `proxies=a&proxies=b`(对齐 Flask request.form.getlist / request.json)。
async fn delete_many_handler(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Json<Value> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let proxies: Vec<String> = if content_type.starts_with("application/json") {
        serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v.get("proxies").and_then(Value::as_array).cloned())
            .map(|arr| {
                arr.into_iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        parse_form_pairs(&body)
            .into_iter()
            .filter(|(k, _)| k == "proxies")
            .map(|(_, v)| v)
            .collect()
    } else {
        Vec::new()
    };
    if proxies.is_empty() {
        return Json(json!({"code": 1, "msg": "proxy or proxies parameter required"}));
    }
    let deleted = match st.handler.delete_many(&proxies).await {
        Ok(n) => n,
        Err(e) => {
            error!(error = %e, "批量删除失败");
            0
        }
    };
    Json(json!({"code": 0, "src": deleted}))
}

/// Proxy 转 JSON value(字段与 Python to_dict 一致)。
fn proxy_value(p: &Proxy) -> Value {
    serde_json::to_value(p).expect("Proxy 字段均为基本类型, 序列化不会失败")
}

/// 极简表单解析: `k=v&k=v`, 值做百分号解码(等价 Flask form.getlist)。
fn parse_form_pairs(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((url_decode(k), url_decode(v)))
        })
        .collect()
}

/// 百分号解码, `+` 视为空格(表单编码规则); 解不开的序列原样保留。
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let byte = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(b) = byte {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_pairs_decode_repeated_keys() {
        let pairs = parse_form_pairs("proxies=1.2.3.4%3A80&proxies=5.6.7.8%3A8080&x=1");
        let proxies: Vec<String> = pairs
            .into_iter()
            .filter(|(k, _)| k == "proxies")
            .map(|(_, v)| v)
            .collect();
        assert_eq!(proxies, vec!["1.2.3.4:80", "5.6.7.8:8080"]);
    }

    #[test]
    fn url_decode_plus_and_invalid_escape() {
        assert_eq!(url_decode("a%20b+c"), "a b c");
        assert_eq!(url_decode("%ZZ"), "%ZZ");
    }

    #[test]
    fn https_flag_semantics() {
        let mut q = HashMap::new();
        assert!(!https_flag(&q));
        q.insert("type".into(), "HTTPS".into());
        assert!(https_flag(&q));
        q.insert("type".into(), "http".into());
        assert!(!https_flag(&q));
    }
}
