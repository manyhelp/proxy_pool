# ProxyPool Rust 版

Python 版 proxy_pool 的 Rust 重写。**Redis 数据结构与 HTTP API 与 Python 版兼容**,
两版可以同时运行、灰度切换; 随时回滚到 Python 版不丢数据。

## 与 Python 版的兼容性

| 维度 | 状态 |
|---|---|
| Redis hash(`use_proxy`, key=`ip:port`, value=Proxy JSON) | ✅ 字段级一致, 互相可读 |
| `/get` `/pop` `/all` `/delete` `/count` `/refresh` | ✅ 路径与响应字段名一致 |
| `/delete` POST 批量(`proxies` 表单或 JSON) | ✅ |
| 校验语义(东方财富目标站、http→https 隧道、`MAX_FAIL_COUNT` 淘汰) | ✅ 对齐 |
| `FETCH_PROXY` 直连失败回退 | ✅ 对齐 |
| SSDB 后端 | ❌ 已移除(仅 Redis) |

## 构建

```bash
rustc >= 1.75
cargo build --release
```

TLS 走默认 native-tls(Windows 用 schannel, 零额外依赖; Linux 部署需 `libssl-dev`)。

## 运行

```bash
# 双击 proxy-pool.exe(无参数) = 合并模式: 调度+API 同进程一起跑, Ctrl-C 停止
# 等价命令:
proxy-pool run

# 或分开跑:
proxy-pool schedule   # 只跑调度(采集+校验, 常驻)
proxy-pool server     # 只跑 WebApi(默认 0.0.0.0:5010)

# 或两个独立进程一起跑
start.cmd   # Windows
./start.sh  # Linux
```

部署方式: 把 `proxy-pool.exe` 和 `setting.toml` 放在同一目录即可, 双击 exe 启动,
配置自动从 exe 所在目录加载(双击时工作目录就是 exe 目录; 即使工作目录不同,
exe 同目录的配置文件也会兜底生效)。

## 配置(三层优先级: 环境变量 > setting.toml > 内置默认值)

对应 Python 版的 `setting.py`, Rust 版使用 **`setting.toml`**(仓库里已带模板, 直接改)。

**文件查找顺序**(找到第一个即停):
1. `SETTING` 环境变量指定的路径(指定了但文件不存在 → 启动报错, 不静默跳过);
2. 当前工作目录下的 `setting.toml`;
3. exe 同目录下的 `setting.toml`。

环境变量可覆盖文件中的同名项(便于容器/临时覆盖, 不用改文件); 两者都没配的项
使用与 Python `setting.py` 相同的默认值。启动日志会打印实际加载的配置文件路径。

| 配置项(setting.toml 键) | 默认值 | 说明 |
|---|---|---|
| `host` / `port` | `0.0.0.0` / `5010` | API 监听地址(环境变量 `HOST`/`PORT`) |
| `db_conn` | `redis://:@127.0.0.1:6379/0` | Redis 连接串(空凭据自动剥离, `DB_CONN`) |
| `table_name` | `use_proxy` | Redis hash 表名(`TABLE_NAME`) |
| `proxy_fetcher` | 全部 20 个源 | 抓取源名单数组(环境变量为逗号分隔) |
| `fetch_proxy` | `http://100.72.93.13:11808` | ⚠️ 沿用了 setting.py 的现网取值, 换机器部署请显式设置或置空关闭(`FETCH_PROXY`) |
| `http_url` / `https_url` | 东方财富页面/接口 | 校验目标(`HTTP_URL`/`HTTPS_URL`) |
| `verify_timeout` | `5` | 单次校验超时秒(`VERIFY_TIMEOUT`) |
| `max_fail_count` | `0` | 允许的失败次数, 0 = 一次失败即淘汰(`MAX_FAIL_COUNT`) |
| `pool_size_min` | `200` | check 时池子低于此值先补采(`POOL_SIZE_MIN`) |
| `proxy_region` | `false` | 是否查询代理地域(ip-api.com, `PROXY_REGION`) |
| `check_concurrency` | `100` | 单轮校验并发数(`CHECK_CONCURRENCY`, Rust 新增) |

配置文件非法(路径错误/TOML 语法错误/字段类型错)会**启动即失败**并给出明确错误,
不会静默回退默认值——防止改了配置却没生效的假运行。

日志: 输出到 stdout, 级别由 `RUST_LOG` 控制(默认 `info`)。不再写 `log/` 目录,
长期运行建议交给 systemd/journald 或重定向采集。

## 与 Python 版的行为差异(有意为之)

1. **校验并发可调**: Python 固定 20 线程, 全量校验一轮常跑不完 2 分钟;
   Rust 版默认 100 并发(`buffer_unordered`), 可用 `CHECK_CONCURRENCY` 调整。
2. **同名任务不重叠**: Python `max_instances=10` 允许多轮 check 并存, 互相挤占;
   Rust 版用原子标志跳过上轮未结束的 tick(2026-11-16 起 Python 版也加了同样的
   非阻塞锁, 两版行为一致)。
3. **错误容忍**: Python 校验线程里任何 DB 异常会悄悄杀掉线程(daemon),
   Rust 版单条失败记日志继续, 不中断整个批次。
4. **Raw 入库原子化**: Python 是 `exists` → `put` 两步(两次往返 + 竞态窗口),
   Rust 版用 HSETNX 一次原子完成, 语义等价(存在保留原记录, 不存在才写入)。
5. **采集预过滤(去重复校验)**: 每轮抓取结果先与池内 key 比对, **已在池中的代理
   不再重复 raw 校验**。Python 版(修复前)对全部抓取结果从头验证, 池内常驻代理
   每轮抓取都被重复验证一次(实测 365 个抓取结果仅 44 个在池外, 大部分校验纯浪费)。
6. **采集互斥(防并发双跑)**: 定时抓取与 check 内嵌补采共用互斥标志, 同一时刻
   只有一轮采集在跑(修复前的 Python 版两者可能并发双跑)。获取频率不做限制:
   池子低于 `pool_size_min` 时每轮 check 都会补采, 反正预过滤会跳过池内已有代理。
7. **刚入库免复检**: 补采中刚 raw 校验通过的代理, 随后的 use 全量复检跳过,
   不再"入库 5 秒后又验一遍"。
8. **抓取失败显式化**: Python 全部失败时伪造 200 空响应继续解析;
   Rust 版返回 `None` 明确跳过, 日志可查。
9. **`PROXY_REGION` 布尔语义修正**: Python `bool(os.getenv(...))` 下 `"False"` 也是真;
   Rust 版按 `true/1/yes/on` 字面判断。
10. **地域查询修正**: Python 对 `user:pass@ip:port` 会误把用户名当 IP 查;
   Rust 版取最后一个 `@` 后的地址。
11. 斜杠变体(`/get` 与 `/get/`)均直接 200, Flask 版会 308 重定向。
12. API 层 DB 异常返回 JSON 错误结构(`{"code":1,...}`)而非 500 HTML。

## 目录结构

```
src/
├── main.rs            # CLI 入口(schedule/server 两个子命令)
├── lib.rs             # 模块树
├── settings.rs        # 配置(默认值 + 环境变量覆盖)
├── proxy.rs           # 代理模型, 与 Python JSON 严格互操作
├── db.rs              # Redis hash 存储层
├── handler.rs         # CRUD 门面 + bootstrap 装配
├── web.rs             # 抓取 HTTP 封装(直连优先, FETCH_PROXY 回退)
├── fetcher/
│   ├── mod.rs         # 采集调度: 并发源 + 去重 + 格式预校验
│   └── sources.rs     # 20 个代理源
├── validator.rs       # http/https 校验(东方财富目标)
├── checker.rs         # 有界并发校验执行器
├── scheduler.rs       # fetch 10min / check 2min 周期调度
├── api.rs             # axum WebApi
├── logger.rs          # tracing 初始化
└── errors.rs          # 统一错误类型
```

## 测试

```bash
cargo test          # 20 个单元测试 + 10 个文档测试
cargo clippy        # 0 警告
```

关键互操作测试: `proxy::tests::parses_python_generated_json` 用 Python
`json.dumps` 的真实输出做 fixture, 保证两版数据互通。

## 待办

- Dockerfile / docker-compose(Rust 多阶段构建)
- 若需要 SSDB 后端, 按 `db.rs` 的接口补一个实现即可
