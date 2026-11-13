//! ProxyPool Rust 版: 免费代理采集 + 校验 + Redis 存储 + HTTP API。
//!
//! 与 Python 版保持两类兼容:
//! 1. Redis 数据兼容: hash 结构 `use_proxy`, value 为 Python 版 `Proxy.to_json` 的同构
//!    JSON, 两版可同时运行、灰度切换;
//! 2. HTTP API 兼容: `/get` `/pop` `/all` `/delete` `/count` 的路径与响应字段名一致。
//!
//! 模块职责:
//! - [`settings`] — 配置(默认值对齐 Python setting.py, 环境变量覆盖)
//! - [`proxy`] — 代理模型与格式校验
//! - [`db`] / [`handler`] — Redis 存储与 CRUD 门面
//! - [`web`] — 抓取用 HTTP 封装(直连失败自动走 FETCH_PROXY)
//! - [`fetcher`] — 20 个抓取源与采集调度
//! - [`validator`] / [`checker`] — 可用性校验与并发执行器
//! - [`scheduler`] — 周期调度(fetch 10min / check 2min)
//! - [`api`] — axum WebApi
//! - [`runner`] — 合并运行模式(调度 + API 同进程, 双击 exe 即此模式)
//! - [`logger`] — tracing 初始化

pub mod api;
pub mod checker;
pub mod db;
pub mod errors;
pub mod fetcher;
pub mod handler;
pub mod logger;
pub mod proxy;
pub mod runner;
pub mod scheduler;
pub mod settings;
pub mod validator;
pub mod web;
