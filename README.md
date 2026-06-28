# 股选优

这是一个面向 A 股的选股智能体项目。当前运行时已经统一为 Tauri + Rust：桌面端和 Android 端加载同一套 React/TypeScript 前端，通过 Tauri command 调用 Rust 后端获取行情、财务、消息、回测和资金证据数据。

项目不再包含 Python/FastAPI 后端、Python 依赖清单或 Python 测试入口。

## 当前能力

- 基础选股：按行业、市盈率、市净率、ROE、市值、ST 状态、轮动热度等条件筛选，并支持分板块选股。
- 关系图选股：用股票知识图谱和关系评分建模同行业、供应链、主题、估值相似、市值相近等关系。
- 趋势选股：把通达信风格的 SWL/SWS 指标、红色持股、短买、离场、支撑阻力和量化评分转成可计算结果。
- 行情观察：查看股票快照、股本/EPS、分钟线、日线技术面和综合资金证据。
- 回测验证：支持等权组合、月度/季度再平衡、交易成本扣减、候选池等权基准，以及自选观察池固定标的回测。
- 消息证据：支持个股消息、上下游 RAG 包、移动端扫码导入和本地规则判定。
- 移动端：安装后联网生成手机本地股票池，支持筛选、观察、收藏、自选回测、消息拉取和响应式面板。
- 数据维护：支持股票池刷新、交易日收盘后自动刷新检查、缓存状态查看和可丢弃行情缓存清理。

## 风险提示

本项目用于 A 股选股研究、策略验证和产品使用，不构成任何投资建议或收益承诺。股票市场有风险，投资需谨慎；用户应结合自身风险承受能力独立判断，并自行承担投资决策结果。

## 本地运行

默认开发入口是 Tauri 桌面端：

```bash
.\start-tauri-dev.bat
```

首次运行会准备 `desktop/mobile-dist` 静态前端资源，并执行 Rust/Tauri 预检。也可以在 `desktop/` 下直接运行：

```bash
cd desktop
cmd /c npm install
cmd /c npm run dev -- --no-watch
```

构建 Windows 桌面安装包：

```bash
cd desktop
cmd /c npm run build:windows
```

## Android

构建 Tauri Android 安装包前，需要安装 Android SDK、Android NDK，并设置 `ANDROID_HOME` 和 `NDK_HOME`。仓库提供初始化和构建脚本：

```bash
powershell -ExecutionPolicy Bypass -File scripts\build-android.ps1 -InitOnly
powershell -ExecutionPolicy Bypass -File scripts\build-android.ps1
```

也可以在 `desktop/` 下执行：

```bash
cmd /c npm run android:init
cmd /c npm run build:android
```

Android 端加载本地静态前端，并通过 Tauri command 调用 Rust 后端。股票池不随安装包预制；首次安装打开后，移动端通过腾讯行情联网生成手机本地股票池，后续“联网更新股票池”会直接刷新手机缓存。

## 代码结构

- `desktop/frontend/`：React + TypeScript 前端源码，Windows 和 Android 共用。
- `desktop/mobile-dist/`：前端构建产物，由脚本生成，不手工编辑。
- `desktop/src-tauri/`：Tauri 桌面/移动壳和 Rust command。
- `native/gp-core/`：可嵌入的 Rust 核心库，覆盖筛选、趋势、关系图、回测和本地智能体路由。
- `scripts/`：Tauri、Android、移动核心和缓存维护脚本。
- `app/prompts/`：智能体约束和移动端技能说明文本。

前端源码唯一真源是 `desktop/frontend/`。`scripts/prepare-tauri-android-assets.ps1` 会在 Android 构建前重建前端产物并写入移动端财务快照。

## Rust 核心

运行核心库测试：

```bash
cargo test --manifest-path native/gp-core/Cargo.toml
```

构建移动核心：

```bash
powershell -ExecutionPolicy Bypass -File scripts\build-mobile-core.ps1
```

`native/gp-core` 同时暴露 Rust 函数和小型 C ABI，移动端可以把 SQLite、内置 JSON 或远端接口拿到的股票、关系、历史行情封装为 `CoreDataSet` 后传入 Rust 核心。

## 数据源

桌面端和移动端统一使用 Rust/Tauri 数据链路：通达信负责 A 股股票池、日线和分钟线，腾讯股票负责筛选行情和盘口优先数据。数据源对外只保留 `tdx`；旧版本保存过的 `astock`、`akshare`、`eastmoney` 配置已不再兼容，发布版遇到这些值会明确拒绝。

常用环境变量：

- `TDX_CACHE`：通达信股票池缓存路径，默认 `data/cache/tdx_stocks.csv`。
- `TDX_REFRESH=true`：强制刷新股票池。
- `TDX_HOSTS=host1:7709,host2:7709`：指定通达信服务器。
- `TDX_TIMEOUT=6`：连接超时秒数。
- `TDX_TENCENT_BATCH_SIZE=80`：腾讯批量行情请求大小。
- `STOCK_PROXY_MODE=system|none`：行情请求是否使用系统代理。

移动端 RAG 的使用方式和工程设计见 `docs/rag.md`。当前 Tauri 桌面端提供轻量 Rust RAG pack 构建/查询和上游 RAG 内联导入 JSON。

## 数据维护

默认通过 Tauri 内置数据维护入口执行状态检查、刷新和清理。脚本层可用 `scripts\maintain-data-tauri.ps1` 查看或清理 Tauri AppData 缓存；联网刷新仍在桌面 App 内执行，因为刷新进度依赖 Tauri AppHandle 事件。

```bash
powershell -ExecutionPolicy Bypass -File scripts\maintain-data-tauri.ps1
```

## 智能体与大模型接口

智能体可调用 OpenAI Chat Completions 或兼容 OpenAI 协议的接口。可配置：

- `OPENAI_API_KEY`
- `OPENAI_MODEL`
- `OPENAI_BASE_URL`
- `OPENAI_TEMPERATURE`
- `OPENAI_TIMEOUT_SECONDS`
- `OPENAI_JSON_MODE=false`

智能体定位为“选股研究助手”：只做条件筛选、行情观察、关系图分析、消息证据梳理和回测解释，不承诺收益，不输出直接交易指令。

## 发布检查

常用验证命令：

```bash
cargo check --manifest-path desktop/src-tauri/Cargo.toml
cmd /c npm --prefix desktop/frontend run build
cmd /c npm --prefix desktop run build
```