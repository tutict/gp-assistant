# 股选优

这是一个面向 A 股的选股智能体项目，后端使用 FastAPI，前端提供行情观察、基础选股、关系图选股、趋势指标选股和回测界面。项目同时包含 Tauri 桌面壳、移动端可嵌入的 Rust 核心模块，以及可切换的数据源适配层。

当前能力包括：

- 基础选股：按行业、市盈率、市净率、净资产收益率、市值、是否包含 ST 等条件筛选，并支持按板块分组后每个板块各取若干只股票。
- 关系图选股：用股票知识图谱和类 GNN 关系评分建模同行业、供应链、主题、估值相似、市值相近等关系。
- 趋势选股：把通达信风格的 SWL/SWS 指标、红色持股、短买、离场、支撑阻力、量化评分等信号转成可计算结果。
- 行情观察：查看股票快照、股本/EPS、分钟线和日线技术面。
- 回测验证：支持等权组合、月度/季度再平衡、交易成本扣减、候选池等权基准对比，以及自选观察池固定标的回测。
- 上下游消息 RAG：基于已有产业链关系图分析利好/利空消息，返回证据、置信度和待核查项。
- 智能体编排：优先使用 LangGraph 编排状态和工具流；未安装时自动回退到本地状态机。
- 移动端：安装后首次联网生成手机本地股票池，支持筛选、分板块选股、观察、收藏、自选观察池回测、扫码导入 RAG 包和移动端响应式面板。
- 数据维护：支持股票池刷新、交易日收盘后自动刷新检查、缓存状态查看和可丢弃行情缓存清理。

## 风险提示

本项目用于 A 股选股研究、策略验证和产品使用，不构成任何投资建议或收益承诺。股票市场有风险，投资需谨慎；用户应结合自身风险承受能力独立判断，并自行承担投资决策结果。

## 当前稳定版

当前稳定版为 `v0.3.0`，发布页见 <https://github.com/tutict/stock-optimizer/releases/tag/v0.3.0>。

发布页提供：

- Windows 安装包：`StockOptimizer_0.3.0_windows-x64-setup.exe`
- Android 可安装测试包：`StockOptimizer_0.3.0_android-aarch64-debug.apk`
- Android 未签名 release 包：`StockOptimizer_0.3.0_android-aarch64-release-unsigned.apk`
- Linux 安装包：`StockOptimizer_0.3.0_linux-amd64.deb`
- Linux 便携包：`StockOptimizer_0.3.0_linux-amd64.AppImage`
- `SHA256SUMS.txt`：发布产物 SHA-256 校验值

Android 的 `release-unsigned.apk` 需要接入正式 keystore 后再签名分发；普通手机安装验证优先使用 debug APK。

## 本地运行

```bash
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -r requirements.txt
set STOCK_PROVIDER=tdx
set OPENAI_API_KEY=your_key_here
set OPENAI_MODEL=gpt-4o-mini
uvicorn app.main:app --reload
```

打开 `http://127.0.0.1:8000` 即可访问响应式网页界面。

## 桌面端（Tauri）

Tauri 桌面壳位于 `desktop/`，启动时会先拉起本地 FastAPI 服务，再打开桌面窗口。

```bash
cd desktop
cmd /c npm install
cmd /c npm run dev
```

开发模式会优先使用 `GP_ASSISTANT_PYTHON` 指定的解释器，然后依次回退到 `.venv-cpython\Scripts\python.exe`、`.venv\Scripts\python.exe` 和系统 `PATH` 中的 `python`。默认后端地址为 `http://127.0.0.1:8010`，默认数据源为 `STOCK_PROVIDER=tdx`。

构建带 FastAPI sidecar 的 Windows 桌面安装包：

```bash
cd desktop
cmd /c npm run build:windows
```

sidecar 构建使用 PyInstaller，输出到 `desktop/src-tauri/binaries/`。

Linux 桌面包需要在 Linux 环境中构建，推荐使用 WSL Ubuntu。先准备 Rust stable、Node.js、Python/PyInstaller，以及 Tauri Linux 依赖（WebKitGTK、GTK、AppIndicator、librsvg、patchelf 等），并生成 Linux 平台 sidecar：

```bash
python -m PyInstaller --clean --onefile --name stock-optimizer-backend \
  --collect-all onnxruntime --collect-all tokenizers --collect-all pytdx \
  --add-data app/static:app/static \
  --add-data app/prompts:app/prompts \
  app/desktop_server.py
cp dist/stock-optimizer-backend desktop/src-tauri/binaries/stock-optimizer-backend-x86_64-unknown-linux-gnu
```

然后在 `desktop/` 下打包 Linux 安装包：

```bash
npm install
npm exec tauri -- build \
  --config src-tauri/tauri.sidecar.conf.json \
  --config '{"bundle":{"icon":["icons/icon.png","icons/icon.ico"]}}' \
  --bundles deb appimage
```

## 移动端 / 原生核心

移动端不建议直接嵌入 Python/FastAPI sidecar。项目已把核心选股逻辑迁移到 `native/gp-core` Rust 库，可供 Tauri mobile command 调用，也可以通过小型 C ABI 包装给 Swift/Kotlin 使用。

```bash
cargo test --manifest-path native/gp-core/Cargo.toml
powershell -ExecutionPolicy Bypass -File scripts/build-mobile-core.ps1
```

构建 Tauri Android 安装包前，需要先安装 Android SDK、Android NDK，并设置 `ANDROID_HOME` 和 `NDK_HOME`。仓库提供了初始化和构建脚本：

```bash
powershell -ExecutionPolicy Bypass -File scripts\build-android.ps1 -InitOnly
powershell -ExecutionPolicy Bypass -File scripts\build-android.ps1
```

也可以在 `desktop/` 下直接执行：

```bash
cmd /c npm run android:init
cmd /c npm run build:android
```

前端只有 `app/static/` 一份源码，桌面端与移动端共用：运行时通过 `isMobileTauriRuntime()` 判断环境，桌面/Web 走 HTTP `/api/*`，移动端改走 Tauri command（见 `app/static/app.js` 的 `MOBILE_API_HANDLERS` 路由表）。`desktop/mobile-dist/` 不是手工维护的副本，而是由 `scripts/prepare-tauri-android-assets.ps1` 从 `app/static/` 全量生成的产物（已 gitignore）。该脚本被接入 `tauri.android.conf.json` 的 `beforeBuildCommand`/`beforeDevCommand`，因此无论通过 `build-android.ps1` 还是直接 `npm run build:android`/`tauri android dev` 构建，移动端前端都会在打包前自动从 `app/static/` 重新生成，不会出现陈旧漂移。**所有前端改动只改 `app/static/`，切勿手改 `mobile-dist`。**

Android 端不会启动 Python/FastAPI sidecar，会加载本地静态前端，并通过 Tauri command 调用 `native/gp-core`。股票池不再随安装包预制：用户首次安装打开后，移动端会通过腾讯行情联网生成手机本地股票池；后续“联网更新股票池”会直接刷新手机缓存，不需要重新构建或重新安装移动包。当前移动端已覆盖基础筛选、分板块筛选、关系图筛选、趋势、观察、收藏、自选观察池回测、本地智能体路由和 RAG 包扫码导入。

如需构建指定移动端目标，可设置 `GP_CORE_TARGET`，例如安装 Rust target 和 Android NDK 后使用 `aarch64-linux-android`。

Rust 核心目前包含关系图选股、SWL/SWS 趋势选股、确定性回测和本地启发式智能体路由。移动端可以把 SQLite、内置 JSON 或远端接口拿到的股票、关系、历史行情统一封装为 `CoreDataSet` 后传入 Rust 核心。

### Python ↔ Rust 双实现与一致性护栏

选股 / 趋势 / 回测 / 关系图这几块逻辑同时存在两份实现：

- **Python（`app/services/*`）是权威源**：功能最全（因子加权打分、PageRank+图嵌入关系评分、带再平衡和基准的组合回测、筹码分布等），桌面端与 Web 后端使用它。
- **Rust（`native/gp-core`）是移动端实现**：手机端无法运行 Python/pandas/akshare，所以核心算法在 Rust 里另写一份。它是刚需，不能删，但目前是 Python 的**简化子集**：
  - `trend`（趋势）：与 Python **数值等价**，是认真对齐的移植（仅缺 Python 的筹码分布旁路）。
  - `screen`（选股）：**过滤条件一致**，但打分口径不同（Rust 加法启发式 vs Python 因子加权制），分数与排序会不同。
  - `graph`（关系图）：候选池一致，但 Rust 只实现了局部邻域评分，**缺 PageRank 与图嵌入**。
  - `backtest`（回测）：选标的与等权 buy&hold 一致，但 Rust **没有再平衡 / 交易成本 / 基准曲线**。

`tests/test_core_parity.py` 通过 `ctypes` 直接加载 `gp-core` 的 cdylib（无需 PyO3），用同一份 mock 数据喂给两套实现做对拍：`trend` 断言数值容差内相等，`screen/graph/backtest` 断言共享部分（过滤、选标的、候选池）不漂移，并把已知算法差异钉成显式契约。**改动任一侧的共享逻辑时，请保持对拍测试通过**；若有意改变某条差异（例如把 Rust 打分对齐到 Python），需同步更新该测试。运行前先构建 cdylib：

```bash
cargo build --release --manifest-path native/gp-core/Cargo.toml
```

未构建时对拍测试会自动跳过（带构建提示），不会让无 Rust 工具链的环境失败。

## 数据源

桌面端和 Web 后端统一使用 `tdx` 数据源组合：通达信负责 A 股股票池、日线和分钟线，筛选行情与盘口优先走腾讯股票。旧版本保存过的 `astock`、`akshare`、`eastmoney` 配置会自动迁移到 `tdx`，不会再作为可选数据源显示。

API 客户端可以通过请求头显式指定通达信数据源：

- `X-Stock-Provider: tdx`
- `X-Stock-Refresh: true` 强制刷新所选数据源的股票池缓存
- `X-Stock-Proxy: system|none` 切换行情数据源请求是否使用系统代理；也可用环境变量 `STOCK_PROXY_MODE=system|none` 设置默认值。

通达信股票池缓存路径默认为 `data/cache/tdx_stocks.csv`，可用 `TDX_CACHE` 调整；可设置 `TDX_REFRESH=true` 强制刷新。通达信服务器可用 `TDX_HOSTS=host1:7709,host2:7709` 指定，连接超时可用 `TDX_TIMEOUT=6` 调整。腾讯批量行情大小可用 `TDX_TENCENT_BATCH_SIZE=80` 调整。

筛选价格口径为“北京时间 15:00 前使用前一交易日收盘价，15:00 后使用当天收盘价”，腾讯行情优先、通达信补充、本地缓存兜底。移动端不再打包预制股票池；用户安装后首次打开会通过腾讯行情联网生成手机本地股票池，后续筛选、观察和回测都读取该本地缓存。

上下游消息分析缓存路径为 `data/cache/news.sqlite`。证据会区分 `news` 与 `community` 两层：新闻/事实层用于事实证据，社区层用于市场讨论、风险传闻、情绪信号和待核查线索。桌面端默认尝试抓取东方财富股吧社区与 AkShare 东方财富个股新闻；如需关闭，可设置 `GP_NEWS_ENABLE_GUBA=false` 或 `GP_NEWS_ENABLE_AKSHARE=false`。雪球社区适配器已预留，设置 `GP_NEWS_ENABLE_XUEQIU=true` 后会检查 `GP_XUEQIU_COOKIE`，但在稳定授权抓取完成前不会混入不可靠数据。`POST /api/news-rag` 会把检索到的证据、已有关系边和本地规则判断交给已配置的 OpenAI 兼容模型复核；未配置模型密钥时会回退到本地规则。RAG 分析只在已有股票关系图范围内检索消息，不从新闻自动生成供应链关系。

移动端 RAG 的使用方式和工程设计见 `docs/rag.md`。目标方案使用版本化只读 `rag_pack.sqlite`、`sqlite-vec` 和 `bge-small-zh` INT8；桌面端预计算文档向量，手机端只做查询向量和本地检索。

## 数据维护

数据维护接口：

- `GET /api/data-sources/status` 查看股票池数量、缓存大小和新鲜度。
- `POST /api/data-sources/refresh-universe` 刷新股票池。
- `POST /api/data-sources/auto-refresh-universe` 按交易日规则检查并自动刷新基础股票池。
- `POST /api/data-sources/prune-cache` 清理可丢弃缓存。
- `POST /api/news-rag` 按已有上下游关系图检索本地消息缓存，并返回影响判断、证据、置信度和待验证点。

自动刷新规则：

- 使用 A 股交易日历判断是否为交易日。
- 只有北京时间 15:30 之后才会刷新，盘中不自动刷新基础股票池。
- 如果当天收盘后已经刷新过 `data/cache/tdx_stocks.csv`，本日不会重复刷新。
- Web 页面会在启动后按间隔触发检查；Tauri 移动端首次安装无缓存时会立即联网生成股票池，之后按交易日规则刷新手机本地缓存。

命令行定时维护示例：

```bash
python scripts\maintain_data.py --source tdx --refresh --prune
```

移动端存储较小，建议使用轻量缓存策略：保留股票池和最近需要观察的个股行情，定期清理历史行情和分钟线缓存。

## 智能体与大模型接口

智能体可以调用 OpenAI Chat Completions 或兼容 OpenAI 协议的接口。可配置：

- `OPENAI_API_KEY`
- `OPENAI_MODEL`
- `OPENAI_BASE_URL`
- `OPENAI_TEMPERATURE`
- `OPENAI_TIMEOUT_SECONDS`
- `OPENAI_JSON_MODE=false`：用于不支持 JSON mode 的兼容服务

网页的“智能指令台”也支持按请求传入这些参数。

智能体定位为“选股研究助手”，边界约束写在 `app/prompts/stock_soul.md`：只做条件筛选、行情观察、关系图分析、消息证据梳理和回测解释，不承诺收益，不输出直接交易指令。后端会在最终回复中追加“仅供选股研究，不构成投资建议”，并拦截“必涨、稳赚、立即买入、满仓、清仓”等跑偏表述。

LangGraph 只负责智能体状态和工具编排，例如：

```text
parse_intent -> observe_stock/screen/graph_screen/trend_screen/backtest/news_rag/clarify
```

股票之间的关系不是由 LangGraph 本身建模，而是由项目里的知识图谱和类 GNN 关系评分层建模。
智能体也可以直接处理个股观察指令，例如“看看 000001 的估值和股本”，会返回股票快照、股本/EPS、分钟线和日线技术面。

## 行情与技术面接口

- `GET /api/observe/{code}` 返回股票快照、股本/EPS、日线 SWL/SWS 技术面和分钟线。
- `GET /api/minutes/{code}` 返回规范化 A 股分钟线。
- `GET /api/order-book/{code}` 返回规范化买卖盘口。

通达信分钟线和盘口通过 `pytdx` 获取。

回测日期使用 `YYYYMMDD` 格式。

## 构建本地训练数据（RQData HTTP 接口）

```bash
set RQDATA_USERNAME=your_username
set RQDATA_PASSWORD=your_password
python scripts\build_dataset.py --start-date 20190101 --intent-samples 500 --report-samples 100
```

未指定 `--end-date` 时，脚本会使用当前系统日期。

输出：

- `data/datasets/intent.jsonl`：指令到 JSON 参数
- `data/datasets/report.jsonl`：指标到中文报告文本

## 接口示例

基础选股：

```bash
curl -X POST http://127.0.0.1:8000/api/screen ^
  -H "Content-Type: application/json" ^
  -d "{\"max_pe\":10,\"min_roe\":0.1,\"limit\":10}"
```

分板块选股：

```bash
curl -X POST http://127.0.0.1:8000/api/sector-screen ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":25,\"min_roe\":0.08},\"max_sectors\":8,\"per_sector_limit\":3}"
```

关系图选股：

```bash
curl -X POST http://127.0.0.1:8000/api/graph-screen ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":25,\"min_roe\":0.1},\"seed_codes\":[\"300750.SZ\"],\"relation_depth\":1,\"relation_weight\":0.4,\"limit\":10}"
```

趋势选股：

```bash
curl -X POST http://127.0.0.1:8000/api/trend-screen ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":30},\"start_date\":\"20200101\",\"limit\":10}"
```

增强回测：

```bash
curl -X POST http://127.0.0.1:8000/api/backtest ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":10},\"start_date\":\"20200101\",\"top_n\":10,\"rebalance_frequency\":\"monthly\",\"transaction_cost_bps\":10,\"benchmark\":\"candidate_equal_weight\"}"
```

趋势和回测接口未指定 `end_date` 时，默认使用服务运行环境的当前系统日期。
