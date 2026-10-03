# RAG 与 Agent 治理基线

本文档是 RAG 与研究 Agent 的持续治理基线。功能迭代必须同时更新实现、契约测试和本文件中对应的版本或门禁；只改页面文案不算完成治理。

## 当前边界

### RAG

- 研究数据库 schema 为 `3`，由 `desktop/src-tauri/src/research.rs` 统一维护。
- 文档先规范化再分块；内容哈希不变时保留已读状态和向量，已被回答引用的内容变更会生成 revision。
- 检索默认使用 BM25；Windows 在当前模型可用时使用 512 维归一化向量，并以 RRF（`k=60`）融合。
- 来源层级固定为 `filing`、`financial_snapshot`、`news`、`research_report`、`community`。社区证据不能单独支撑事实结论。
- 单次回答最多 8 条引用，每篇文档最多 2 个分块；模型回答必须引用检索结果中的有效编号。
- URL/PDF 导入、资料包导入和保留策略的大小、页数、文档数与正文限制必须在后端再次校验，前端限制不能替代后端限制。
- 固定检索评测版本为 `research-retrieval-eval-v4`，夹具位于 `app/prompts/research_retrieval_eval_cases.json`，覆盖来源优先级、股票过滤先于候选截断、社区证据门禁、每文档引用上限、Top K 上限和混合 RRF。

索引状态至少要能区分以下情况：

- FTS 缺失行和孤立行；
- 缺失或过期的 embedding；
- 孤立、维度错误或 BLOB 长度错误的 embedding；
- FTS 结构完整与是否已经具备混合检索条件。

`/api/research/index-status` 返回的治理字段包括 `fts_healthy`、`integrity_healthy`、`hybrid_ready`、`fts_missing_count`、`fts_orphan_count`、`embedding_pending_count`、`embedding_stale_count`、`embedding_orphan_count` 和 `embedding_invalid_count`。新增字段应保持向后兼容；改变已有字段语义必须提升 schema 或接口版本。

### Agent

- 执行顺序固定为“本地工具取证 -> 模型综合 -> 证据与风险校验 -> 安全合并”。模型不能覆盖工具返回的 `action`、`data` 或证据事实。
- Rig runtime 位于 `desktop/src-tauri/src/rig_runtime.rs`，由 `rig-core` 和 `rig-agent` 0.42 提供 provider、消息、流、工具 schema、工具循环和错误类型；Tauri 只保留兼容 payload/event/ledger 边界。
- `rig-core` 与 `rig-agent` 是桌面必选依赖，默认桌面和 Android 构建都使用同一 Rig runtime；`quick / deterministic_v1` 仍跳过 provider。
- Rig 模型请求必须复用应用的 endpoint 校验、超时、请求/响应大小限制、代理、取消和脱敏策略；Rig 不能自行持有凭据或绕过审计。OpenAI-compatible `full_url` 使用 Rig provider extension 覆盖 completion path；Responses/Anthropic 的自定义 full URL fail closed 并回退本地结果。
- Rig 工具只接收现有 dispatcher 的回调。授权、参数校验、超时、取消、脱敏和事件账本仍由原执行链负责，第一阶段不注册写操作或无限工具循环。
- Rig 证据上下文只包装 `ResearchStore` 的结果，保留 `document_id`、`chunk_id`、来源名称/层级、公开 URL、发布时间、引用编号和检索分数；SQLite FTS5、向量 embedding、RRF 融合、来源排序与 `[C#]` 门禁不迁移。
- 跨请求对话历史只接受 payload 中最近 12 条、每条最多 2000 字符的显式历史，并在进入 Rig 前脱敏；runtime 不保留进程全局 conversation memory。
- 快速模式为 `deterministic_v1`，不调用模型；专家模式为 `hot_money_early_v1`；研报模式为 `value_compounder_v1`。
- 仅接受显式传入的模型连接；错误、超时、无效 JSON、未知引用和越界内容必须回退本地工具结果。
- 模型输出的 `reply` 和事实 bullet 必须邻近引用有效 `[E#]`；直接交易建议、收益承诺和操纵市场内容必须拒绝。
- 错误、成功结果和运行账本都必须脱敏 API key、完整 URL 凭据、查询参数秘密和私有模型地址。
- 每次结果的 `harness` 元数据必须包含 `prompt_version`、`policy_version`、`profile_id`、`model_used`、`model_outcome` 和 `api_format`；提示预览还要包含当次限制快照。
- 当前运行结果的 `model_outcome` 固定区分 `model_success`、`policy_rejected`、`not_configured`、`not_requested` 和 `request_failed`；聚合旧账本时还允许 `in_progress`、`run_failed`、`interrupted` 和 `legacy_unknown`。新增状态必须保持旧指标可解释，不能把策略拒绝混入网络失败。
- `/api/agent/metrics` 只聚合最近至多 2000 条运行的状态、profile、模型结果、协议和耗时分位数。接口不得返回问题、事件、结果正文、错误正文或连接凭据；前端指标失败不能阻断运行记录复盘。

指标口径固定如下：

- 样本总体是按开始时间倒序选取的最近 `limit` 条运行，`limit` 取值为 1..2000；传入 `conversation_id` 时先按会话过滤再截取样本，但响应不回显会话 ID。
- `status_counts`、`profile_counts`、`model_outcome_counts` 和 `api_format_counts` 分别按账本状态、解析后的 profile、模型结果和 API 协议计数；profile 的 `fallback` 包含 `not_configured`、`request_failed`、`policy_rejected` 和 `legacy_unknown`。
- 耗时样本只包含非空且非负的 `duration_ms`。`average_ms` 使用整数除法；P50/P95 使用 nearest-rank，索引为 `ceil(n * p) - 1`；`max_ms` 取最大值。
- 没有有效耗时样本时，平均值、分位数和最大值返回 `null`，前端显示 `--`。指标响应不包含会话 ID、问题、事件、结果正文、错误正文或模型连接配置。

当前版本：

- `prompt_version`: `rig-agent-runtime-v1`
- `policy_version`: `agent-policy-v1`
- `agent_harness_eval_version`: `agent-harness-eval-v2`
- `agent_metrics_schema_version`: `1`

提示词、profile id、证据规则或安全过滤器发生语义变化时，必须同步更新离线评测夹具，并保留旧版本结果用于对照。单纯修复脱敏或边界判断时，可以只升级 `policy_version`。

## 变更流程

1. 先写或更新 Rust/TypeScript 契约测试，明确正常、缺失、过期、越界和回退路径。
2. 修改后端不变量，再接入前端状态展示；前端校验只负责体验，不能承担安全边界。
3. 对提示词或模型协议变化，更新 `app/prompts/agent_harness_eval_cases.json` 和对应离线评测；对检索排序、过滤、分块或证据门禁变化，更新 `app/prompts/research_retrieval_eval_cases.json`。
4. 对数据库或资料包格式变化，更新迁移、回滚和跨平台导入测试，并提升 schema/format 版本。
5. 在发布前检查运行账本是否仍然只保存脱敏请求、事件和结果，不保存 API key 或原始模型请求凭据。

## 发布门禁

最小门禁：

```powershell
npm --prefix desktop\frontend run test:unit
```

RAG/Agent Rust 变更：

```powershell
npm --prefix desktop\frontend run build:app
cargo test --manifest-path desktop\src-tauri\Cargo.toml --lib
```

只运行固定 RAG 检索质量评测：

```powershell
cargo test --manifest-path desktop\src-tauri\Cargo.toml --lib research::tests::fixed_retrieval_eval_suite_enforces_governance_invariants -- --exact
```

完整发布仍以仓库根目录的 `scripts\release-check.ps1` 为准。桌面 Rust 测试依赖 `desktop/mobile-dist`，因此必须先生成前端构建产物；不要把该目录提交到版本库。

## 后续优先级

- 将索引治理字段接入后台维护事件，形成可追踪的自动修复闭环。
- 为固定检索评测增加历史基线报告，在排序或 embedding 模型升级时比较质量变化。
- 为 Agent 指标增加按版本的趋势基线和受控导出，继续保持本地优先和脱敏。
- 在不扩大权限的前提下补充取消、重试、幂等和长期运行清理策略。
## GEPA 实验治理

- GEPA 通过 Windows 与 Android 正式版中的主动实验面板运行；Python 不进入仓库或运行时。
- `dsrust-gepa` 固定为 `=0.1.0-alpha.3`，由 `gepa-lab` Cargo feature 隔离；Windows 与 Android 正式打包命令显式启用该 feature；其他平台保持关闭。
- 优化对象只有 `expert` / `research` profile 的方法卡；系统级安全规则、只读工具、证据目录、模型输出 schema 和安全合并器不属于 GEPA candidate。
- 训练、验证和 holdout 只来自版本化的 `app/prompts/agent_gepa_eval_cases.json`；不读取历史用户问题，不读取实时行情，不把收益作为 metric。
- 安全、证据引用和工具事实保留是硬门禁；通过后才计算 rubric 覆盖率。候选必须经过 holdout 对照，并由用户显式应用，不能自动激活。
- GEPA 报告写入本机 AppData，保存数据集哈希、引擎版本、seed、预算、逐案例分数和脱敏样例；API key、完整 URL 凭据和原始连接配置不得进入报告。
- 现有策略拒绝仍进入 Agent ledger 和指标，但不再触发“五次拒绝后自动生成并激活”提示词升级。Overlay 应用使用 base prompt version compare-and-swap，版本变化时拒绝陈旧候选。

## RAG and Agent evaluation upgrade (2026-10-01)

### Frozen evaluation assets

- `app/prompts/research_retrieval_eval_cases.json` is the deterministic retrieval fixture (`research-retrieval-eval-v4`). It records corpus, chunking, embedding, index, split and generator lineage, plus required/forbidden evidence IDs and answer-fact-to-evidence mappings.
- The v4 fixture contains 40 synthetic component cases, balanced 5-per-category across fact queries, multi-evidence synthesis, temporal validity, source-tier conflict, community-only evidence, no-evidence refusal, private-evidence handling, and Agent trace contracts. It is not a human-reviewed release set; human calibration remains a separate pending asset.
- `evals/deepeval/frozen_public_results.jsonl` is the only payload allowed to leave the repository for the DeepEval judge. `frozen_manifest.json` pins its SHA-256. The runner rejects every other path or modified payload.
- Deterministic replay produces 40 local rows; the frozen public payload contains 35 rows and excludes five private-evidence cases.
- The application database is schema `3`; portable sync packs remain format `gp-research-pack-v2` with SQLite `user_version=2`.

### Evidence privacy

Documents have `remote_export_allowed`. Public sources default to exportable; URL/PDF user imports default to local-only. Legacy schema-2 databases are migrated so existing user imports become local-only and public records remain exportable. Retrieval applies this filter before lexical/vector candidate truncation and RRF fusion when `remote_safe_only` is requested.

A remote research answer and a remote Rig Agent run always use `remote_safe_only=true`. If no export-safe citation remains, the system returns local evidence mode or a local fallback; there is no request-level bypass. The evidence inspector shows whether a citation is local-only or allowed for remote answering.

### GEPA and replay integration

正式版 GEPA 保留为产品内的提示词方法卡优化能力。GEPA 的 validation、holdout 和候选应用仍通过现有 `gepa-lab` feature 执行，但每个候选案例现在复用 `rig_runtime::execute_with_event_sink` 的共享 Agent 执行路径。

GEPA 不再维护独立的模型回答执行链。候选方法卡通过 `method_card` 注入共享系统提示；工具、证据和模型输出由同一套 Agent runtime 产生。GEPA 报告保存脱敏后的只读工具轨迹，包括工具名、规范化参数、参数哈希、顺序、状态、side-effect 分类和证据文档 ID。

候选应用前必须通过：

- 固定证据引用和事实硬门；
- 工具轨迹只读、成功、无重复调用硬门；
- validation 和 holdout 对照；
- prompt base-version compare-and-swap。

DeepEval、model-backed replay 和人工 calibration 仍只在 `evals/` 与受信 CI 中运行，不进入正式 GEPA UI 或产品运行时。
## DeepEval policy

DeepEval is evaluation-only and pinned to `4.2.6`; it is not part of the Tauri or Android runtime. The judge is fixed to DeepSeek `deepseek-flash` at temperature `0`; the runner uses DeepEval's built-in `DeepSeekModel`, which requests Chat Completions JSON mode (`json_object`). The endpoint is configurable through the ignored local `.env` for an official DeepSeek API or compatible relay. The runner exposes separate deterministic, retrieval, generation, and full tracks. `ContextualPrecision`, `ContextualRecall`, `ContextualRelevancy`, `Faithfulness`, `AnswerRelevancy`, and `GEval` are reported separately. Deterministic evidence and trace assertions run first.

Deterministic replay is not eligible for generation-quality scoring because it returns a local fallback answer. Model-backed replay uses separate `RAG_EVAL_AGENT_*` configuration and is rejected if it falls back to the judge configuration or if a public case is not genuinely model-generated.

All DeepEval metrics start as exploratory. `judge_policy.json` marks unstable metrics as `flaky`, and `strict_metrics` is empty until at least 50 human-labeled examples per metric, two independent annotators, adjudication, and Cohen's kappa >= 0.80 are recorded. Only calibrated metrics may become strict gates.

The PR workflow always runs deterministic gates. The cloud judge job runs only from trusted `workflow_dispatch` or a push to `main`, never from untrusted fork pull requests; the protected `rag-eval-judge` environment holds the judge secret.
