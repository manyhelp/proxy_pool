//! 20 个免费代理源, 站点与解析逻辑对齐 Python 版 fetcher/proxyFetcher.py。
//! 每个源返回 `ip:port` 原始字符串列表; 格式预校验与去重由 [`super::run`] 统一处理,
//! 源内部只负责抓取与抽取。

use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;
use tracing::warn;

use crate::settings::Settings;
use crate::web;

/// 按配置名分发到具体源; 未知名字记日志并返回空(与 Python 版一致地跳过)。
pub async fn fetch(name: &str, conf: &Settings) -> Vec<String> {
    match name {
        "freeProxy01" => free_proxy01(conf).await,
        "freeProxy02" => free_proxy02(conf).await,
        "freeProxy03" => free_proxy03(conf).await,
        "freeProxy04" => free_proxy04(conf).await,
        "freeProxy05" => free_proxy05(conf).await,
        "freeProxy06" => free_proxy06(conf).await,
        "freeProxy07" => free_proxy07(conf).await,
        "freeProxy08" => free_proxy08(conf).await,
        "freeProxy09" => free_proxy09(conf).await,
        "freeProxy10" => free_proxy10(conf).await,
        "freeProxy11" => free_proxy11(conf).await,
        "freeProxy12" => free_proxy12(conf).await,
        "freeProxy13" => free_proxy13(conf).await,
        "freeProxy14" => free_proxy14(conf).await,
        "freeProxy15" => free_proxy15(conf).await,
        "freeProxy16" => free_proxy16(conf).await,
        "freeProxy17" => free_proxy17(conf).await,
        "freeProxy18" => free_proxy18(conf).await,
        "freeProxy19" => free_proxy19(conf).await,
        "freeProxy20" => free_proxy20(conf).await,
        other => {
            warn!(source = other, "抓取源不存在, 跳过");
            vec![]
        }
    }
}

/// `ip:port` 行抽取, 正则对齐 Python `_proxyTxtFetcher`
fn ip_port_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}:\d{2,5}\b").expect("内置正则, 编译失败属于程序错误")
    })
}

/// 纯文本源公共实现: 全文抽取 ip:port
async fn txt_source(url: &str, conf: &Settings) -> Vec<String> {
    let Some(text) = web::get_text(url, conf, 30).await else {
        return vec![];
    };
    ip_port_re()
        .find_iter(&text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// `<td>ip</td>...<td>port</td>` 相邻单元格抽取(云代理/proxylistplus 同构)
async fn td_pair_source(urls: &[&str], conf: &Settings) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"<td>(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})</td>[\s\S]*?<td>(\d+)</td>")
            .expect("内置正则, 编译失败属于程序错误")
    });
    let mut out = vec![];
    for url in urls {
        let Some(text) = web::get_text(url, conf, 30).await else {
            continue;
        };
        for cap in re.captures_iter(&text) {
            out.push(format!("{}:{}", &cap[1], &cap[2]));
        }
    }
    out
}

/// JSON 字段取值: 兼容字符串与数字两种形态
fn val_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `{"data": [{ip, port, ...}, ...]}` 形态源公共实现
async fn json_data_list(url: &str, conf: &Settings) -> Vec<String> {
    let Some(json) = web::get_json(url, conf, 30).await else {
        return vec![];
    };
    let Some(arr) = json.get("data").and_then(Value::as_array) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|e| Some(format!("{}:{}", val_str(e, "ip")?, val_str(e, "port")?)))
        .collect()
}

/// 快代理 https://www.kuaidaili.com
async fn free_proxy01(conf: &Settings) -> Vec<String> {
    let mut out = vec![];
    // Python 默认 page_count=1: inha/intr 各抓第 1 页
    for kind in ["inha", "intr"] {
        let url = format!("https://www.kuaidaili.com/free/{kind}/1/");
        let Some(text) = web::get_text(&url, conf, 30).await else {
            continue;
        };
        out.extend(parse_kuaidaili_table(&text));
        // Python sleep(1): 同站连续请求必须间隔, 否则第二条拿不到数据
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    out
}

/// 解析快代理表格; Python 用 lxml xpath `./td/text()` 取前两列
fn parse_kuaidaili_table(text: &str) -> Vec<String> {
    use scraper::{Html, Selector};

    let html = Html::parse_document(text);
    let tr_sel = match Selector::parse("table tr") {
        Ok(s) => s,
        Err(e) => {
            warn!(error = ?e, "kuaidaili selector 解析失败");
            return vec![];
        }
    };
    let td_sel = Selector::parse("td").expect("内置 selector, 不可能失败");
    html.select(&tr_sel)
        .skip(1) // Python proxy_list[1:]: 跳过表头
        .filter_map(|tr| {
            let tds: Vec<String> = tr.select(&td_sel).map(|td| td.text().collect()).collect();
            (tds.len() >= 2).then(|| format!("{}:{}", tds[0].trim(), tds[1].trim()))
        })
        .collect()
}

/// 云代理 http://www.ip3366.net
async fn free_proxy02(conf: &Settings) -> Vec<String> {
    let urls = [
        "http://www.ip3366.net/free/?stype=1",
        "http://www.ip3366.net/free/?stype=2",
    ];
    td_pair_source(&urls, conf).await
}

/// 89免费代理 index 页
async fn free_proxy03(conf: &Settings) -> Vec<String> {
    let Some(text) = web::get_text("https://www.89ip.cn/index_1.html", conf, 30).await else {
        return vec![];
    };
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"<td.*?>[\s\S]*?(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})[\s\S]*?</td>[\s\S]*?<td.*?>[\s\S]*?(\d+)[\s\S]*?</td>",
        )
        .expect("内置正则, 编译失败属于程序错误")
    });
    re.captures_iter(&text)
        .map(|c| format!("{}:{}", &c[1], &c[2]))
        .collect()
}

/// proxylistplus(直连常见 403, 依赖 FETCH_PROXY 回退)
async fn free_proxy04(conf: &Settings) -> Vec<String> {
    let urls = [
        "https://list.proxylistplus.com/Fresh-HTTP-Proxy-List-1",
        "https://list.proxylistplus.com/Fresh-HTTP-Proxy-List-2",
    ];
    td_pair_source(&urls, conf).await
}

/// CharlesPikachu/freeproxy proxies.json
async fn free_proxy05(conf: &Settings) -> Vec<String> {
    let Some(json) =
        web::get_json("https://raw.githubusercontent.com/CharlesPikachu/freeproxy/master/proxies.json", conf, 30).await
    else {
        return vec![];
    };
    let Some(arr) = json.get("data").and_then(Value::as_array) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|e| {
            // Python: protocol 含 http 或 https 即收入(https 含 http 子串, 等价 contains("http"))
            let proto = e.get("protocol")?.as_str()?.to_lowercase();
            if !proto.contains("http") {
                return None;
            }
            Some(format!("{}:{}", val_str(e, "ip")?, val_str(e, "port")?))
        })
        .collect()
}

/// proxifly jsdelivr json(http)
async fn free_proxy06(conf: &Settings) -> Vec<String> {
    json_data_list(
        "https://cdn.jsdelivr.net/gh/proxifly/free-proxy-list@main/proxies/protocols/http/data.json",
        conf,
    )
    .await
}

/// proxifly jsdelivr json(https)
async fn free_proxy07(conf: &Settings) -> Vec<String> {
    json_data_list(
        "https://cdn.jsdelivr.net/gh/proxifly/free-proxy-list@main/proxies/protocols/https/data.json",
        conf,
    )
    .await
}

/// geonode api(http)
async fn free_proxy08(conf: &Settings) -> Vec<String> {
    json_data_list(
        "https://proxylist.geonode.com/api/proxy-list?limit=100&protocols=http&page=1&sort_by=lastChecked&sort_type=desc",
        conf,
    )
    .await
}

/// TheSpeedX/SOCKS-List http.txt: 每行一个 ip:port, 端口须为纯数字
async fn free_proxy09(conf: &Settings) -> Vec<String> {
    let Some(text) =
        web::get_text("https://raw.githubusercontent.com/TheSpeedX/SOCKS-List/master/http.txt", conf, 30).await
    else {
        return vec![];
    };
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (ip, port) = line.split_once(':')?;
            (!port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
                .then(|| format!("{ip}:{port}"))
        })
        .collect()
}

/// parserpp/ip_ports: {分类: [{host, port, ...}, ...]}
async fn free_proxy10(conf: &Settings) -> Vec<String> {
    let Some(json) =
        web::get_json("https://raw.githubusercontent.com/parserpp/ip_ports/main/proxyinfo.json", conf, 30).await
    else {
        return vec![];
    };
    let Some(obj) = json.as_object() else {
        return vec![];
    };
    obj.values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|e| Some(format!("{}:{}", val_str(e, "host")?, val_str(e, "port")?)))
        .collect()
}

/// 89IP 全量接口 tqdl.html, 需要 Referer 等请求头过风控
async fn free_proxy11(conf: &Settings) -> Vec<String> {
    let url = reqwest::Url::parse_with_params(
        "https://www.89ip.cn/tqdl.html",
        &[
            ("num", "9999"),
            ("address", ""),
            ("kill_address", ""),
            ("port", ""),
            ("kill_port", ""),
            ("isp", ""),
        ],
    )
    .expect("内置 URL 拼接, 不可能失败");
    let headers = [
        ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
        ("Referer", "https://www.89ip.cn/ti.html"),
        ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36"),
    ];
    let Some(text) = web::get_text_with_headers(url.as_str(), conf, 30, &headers).await else {
        return vec![];
    };
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // 注意此处端口是 \d+, 与 _proxyTxtFetcher 的 \d{2,5} 不同, 对齐 Python 原实现
        Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}:\d+\b").expect("内置正则, 编译失败属于程序错误")
    });
    re.find_iter(&text).map(|m| m.as_str().to_string()).collect()
}

/// monosans/proxy-list
async fn free_proxy12(conf: &Settings) -> Vec<String> {
    txt_source(
        "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/http.txt",
        conf,
    )
    .await
}

/// proxifly raw 文本版(jsdelivr 被墙时的备用入口)
async fn free_proxy13(conf: &Settings) -> Vec<String> {
    txt_source(
        "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/http/data.txt",
        conf,
    )
    .await
}

/// clarketm/proxy-list
async fn free_proxy14(conf: &Settings) -> Vec<String> {
    txt_source(
        "https://raw.githubusercontent.com/clarketm/proxy-list/master/proxy-list-raw.txt",
        conf,
    )
    .await
}

/// mmpx12/proxy-list
async fn free_proxy15(conf: &Settings) -> Vec<String> {
    txt_source(
        "https://raw.githubusercontent.com/mmpx12/proxy-list/master/http.txt",
        conf,
    )
    .await
}

/// roosterkid/openproxylist
async fn free_proxy16(conf: &Settings) -> Vec<String> {
    txt_source(
        "https://raw.githubusercontent.com/roosterkid/openproxylist/main/HTTPS_RAW.txt",
        conf,
    )
    .await
}

/// proxyscrape api
async fn free_proxy17(conf: &Settings) -> Vec<String> {
    txt_source(
        "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=http&timeout=10000&country=all",
        conf,
    )
    .await
}

/// openproxylist.xyz
async fn free_proxy18(conf: &Settings) -> Vec<String> {
    txt_source("https://openproxylist.xyz/http.txt", conf).await
}

/// proxyspace.pro
async fn free_proxy19(conf: &Settings) -> Vec<String> {
    txt_source("https://proxyspace.pro/http.txt", conf).await
}

/// free-proxy-list.net
async fn free_proxy20(conf: &Settings) -> Vec<String> {
    txt_source("https://free-proxy-list.net/", conf).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_regex_extracts_ip_port() {
        let text = "1.2.3.4:8080\r\nhttp://5.6.7.8:3128\nnot an ip\n9.9.9.9:443";
        let got: Vec<String> = ip_port_re()
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect();
        assert_eq!(got, vec!["1.2.3.4:8080", "5.6.7.8:3128", "9.9.9.9:443"]);
    }

    #[test]
    fn td_pair_regex_extracts_adjacent_cells() {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"<td>(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})</td>[\s\S]*?<td>(\d+)</td>")
                .unwrap()
        });
        let text = "<table><tr><td>1.2.3.4</td><td>80</td></tr>\n<tr><td>5.6.7.8</td><td>8080</td></tr></table>";
        let got: Vec<String> = re
            .captures_iter(text)
            .map(|c| format!("{}:{}", &c[1], &c[2]))
            .collect();
        assert_eq!(got, vec!["1.2.3.4:80", "5.6.7.8:8080"]);
    }

    #[test]
    fn kuaidaili_table_parses_rows_and_skips_header() {
        let html = "<table><tr><th>IP</th><th>PORT</th></tr>\
                    <tr><td> 1.2.3.4 </td><td>80</td></tr>\
                    <tr><td>5.6.7.8</td><td>8080</td></tr></table>";
        assert_eq!(
            parse_kuaidaili_table(html),
            vec!["1.2.3.4:80", "5.6.7.8:8080"]
        );
    }

    #[test]
    fn val_str_handles_string_and_number() {
        let v: Value = serde_json::from_str(r#"{"ip": "1.2.3.4", "port": 80}"#).unwrap();
        assert_eq!(val_str(&v, "ip").as_deref(), Some("1.2.3.4"));
        assert_eq!(val_str(&v, "port").as_deref(), Some("80"));
        assert_eq!(val_str(&v, "missing"), None);
    }

    #[test]
    fn thespeedx_line_format() {
        // 对齐 free_proxy09 的逐行判定逻辑
        let text = "1.2.3.4:80\n5.6.7.8:8080:x\n9.9.9.9\n1.1.1.1:080";
        let got: Vec<String> = text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let (ip, port) = line.split_once(':')?;
                (!port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
                    .then(|| format!("{ip}:{port}"))
            })
            .collect();
        assert_eq!(got, vec!["1.2.3.4:80", "1.1.1.1:080"]);
    }
}
