# A股选股智能体

这是一个面向 A 股的选股智能体项目，后端使用 FastAPI，前端提供行情观察、基础选股、关系图选股、趋势指标选股和回测界面。项目同时包含 Tauri 桌面壳、移动端可嵌入的 Rust 核心模块，以及可切换的数据源适配层。

当前能力包括：

- 基础选股：按行业、市盈率、市净率、净资产收益率、市值、是否包含 ST 等条件筛选。
- 关系图选股：用股票知识图谱和类 GNN 关系评分建模同行业、供应链、主题、估值相似、市值相近等关系。
- 趋势选股：把通达信风格的 SWL/SWS 指标、红色持股、短买、离场、支撑阻力、量化评分等信号转成可计算结果。
- 行情观察：查看股票快照、五档盘口、分钟线和日线技术面。
- 智能体编排：优先使用 LangGraph 编排状态和工具流；未安装时自动回退到本地状态机。
- 数据维护：支持股票池刷新、缓存状态查看和可丢弃行情缓存清理。

## 风险提示

本项目仅用于 A 股选股研究、策略验证和技术演示，不构成任何投资建议或收益承诺。股票市场有风险，投资需谨慎；用户应结合自身风险承受能力独立判断，并自行承担投资决策结果。

## 本地运行

```bash
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -r requirements.txt
set STOCK_PROVIDER=akshare
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

开发模式会优先使用 `GP_ASSISTANT_PYTHON` 指定的解释器，然后依次回退到 `.venv-cpython\Scripts\python.exe`、`.venv\Scripts\python.exe` 和系统 `PATH` 中的 `python`。默认后端地址为 `http://127.0.0.1:8010`，默认数据源为 `STOCK_PROVIDER=mock`。

构建带 FastAPI sidecar 的 Windows 桌面安装包：

```bash
cd desktop
cmd /c npm run build:windows
```

sidecar 构建使用 PyInstaller，输出到 `desktop/src-tauri/binaries/`。

## 移动端 / 原生核心

移动端不建议直接嵌入 Python/FastAPI sidecar。项目已把核心选股逻辑迁移到 `native/gp-core` Rust 库，可供 Tauri mobile command 调用，也可以通过小型 C ABI 包装给 Swift/Kotlin 使用。

```bash
cargo test --manifest-path native/gp-core/Cargo.toml
powershell -ExecutionPolicy Bypass -File scripts/build-mobile-core.ps1
```

如需构建指定移动端目标，可设置 `GP_CORE_TARGET`，例如安装 Rust target 和 Android NDK 后使用 `aarch64-linux-android`。

Rust 核心目前包含本地演示数据选股、关系图选股、SWL/SWS 趋势选股、确定性回测和本地启发式智能体路由。移动端可以把 SQLite、内置 JSON 或远端接口拿到的股票、关系、历史行情统一封装为 `CoreDataSet` 后传入 Rust 核心。

## 数据源

Web 界面可以在每次请求中切换数据源：

- `mock`：本地演示数据，便于离线测试和 UI 验证。
- `akshare`：通过 AkShare 获取 A 股公开行情。
- `eastmoney`：直接从东方财富获取 A 股股票池，并使用本地 CSV 缓存。

API 客户端也可以通过请求头切换：

- `X-Stock-Provider: mock|akshare|eastmoney`
- `X-Stock-Refresh: true` 强制刷新所选数据源的股票池缓存

AkShare 股票池缓存路径为 `data/cache/stocks.csv`，可设置 `AKSHARE_REFRESH=true` 强制刷新。东方财富缓存路径为 `data/cache/eastmoney_stocks.csv`。

## 数据维护

数据维护接口：

- `GET /api/data-sources/status` 查看股票池数量、缓存大小和新鲜度。
- `POST /api/data-sources/refresh-universe` 刷新股票池。
- `POST /api/data-sources/prune-cache` 清理可丢弃缓存。

命令行定时维护示例：

```bash
python scripts\maintain_data.py --source eastmoney --refresh --prune
```

移动端存储较小，建议使用轻量缓存策略：保留股票池和最近需要观察的个股行情，定期清理历史行情、分钟线和盘口缓存。

## 智能体与大模型接口

智能体可以调用 OpenAI Chat Completions 或兼容 OpenAI 协议的接口。可配置：

- `OPENAI_API_KEY`
- `OPENAI_MODEL`
- `OPENAI_BASE_URL`
- `OPENAI_TEMPERATURE`
- `OPENAI_TIMEOUT_SECONDS`
- `OPENAI_JSON_MODE=false`：用于不支持 JSON mode 的兼容服务

网页的“智能指令台”也支持按请求传入这些参数。

LangGraph 只负责智能体状态和工具编排，例如：

```text
parse_intent -> screen/graph_screen/trend_screen/backtest/clarify
```

股票之间的关系不是由 LangGraph 本身建模，而是由项目里的知识图谱和类 GNN 关系评分层建模。

## 行情与技术面接口

- `GET /api/observe/{code}` 返回股票快照、日线 SWL/SWS 技术面、分钟线和五档盘口。
- `GET /api/minutes/{code}` 返回规范化 A 股分钟线。
- `GET /api/order-book/{code}` 返回规范化买卖盘口。

AkShare 分钟线使用 `stock_zh_a_hist_min_em`，盘口使用 `stock_bid_ask_em`。本地演示数据为确定性生成，适合本地测试。

回测日期使用 `YYYYMMDD` 格式。

## 构建本地训练数据（RQData HTTP API）

```bash
set RQDATA_USERNAME=your_username
set RQDATA_PASSWORD=your_password
python scripts\build_dataset.py --start-date 20190101 --end-date 20240101 --intent-samples 500 --report-samples 100
```

输出：

- `data/datasets/intent.jsonl`：指令到 JSON 参数
- `data/datasets/report.jsonl`：指标到中文报告文本

## API 示例

基础选股：

```bash
curl -X POST http://127.0.0.1:8000/api/screen ^
  -H "Content-Type: application/json" ^
  -d "{\"max_pe\":10,\"min_roe\":0.1,\"limit\":10}"
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
  -d "{\"criteria\":{\"max_pe\":30},\"start_date\":\"20200101\",\"end_date\":\"20240101\",\"limit\":10}"
```

持有回测：

```bash
curl -X POST http://127.0.0.1:8000/api/backtest ^
  -H "Content-Type: application/json" ^
  -d "{\"criteria\":{\"max_pe\":10},\"start_date\":\"20200101\",\"end_date\":\"20240101\",\"top_n\":10}"
```
