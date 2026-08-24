# Rig Agent Runtime

Agent 页面采用“本地工具取证 → 模型综合 → 安全合并”的执行链。模型只能解释工具结果，不能替换工具返回的 `action`、`data` 和证据事实；工具的原始摘要、结构化区块、警告和下一步动作会保留，模型回复以“模型综合”追加，模型区块单独标记为“模型推断”。模型未配置、请求失败、JSON 无效或输出越界时，页面返回带风险提示的本地工具结果。

## 模式与提示词版本

| 页面模式 | profile_id | 作用 |
| --- | --- | --- |
| 快速模式 | `deterministic_v1` | 简洁整理本地工具事实，不调用 LLM，不引入人物方法 |
| 专家模式 | `hot_money_early_v1` | 借鉴公开的游资早期研究框架，检查市场环境、主线、领先性、情绪周期和失效条件 |
| 研报模式 | `value_compounder_v1` | 借鉴格雷厄姆、费雪、巴菲特、芒格的公开长期研究框架，检查企业质量、护城河、所有者收益、资本配置、估值假设和安全边际 |

两套人物方法卡都只决定“研究哪些问题”，不模拟人物口吻，不把转述当作一手事实，也不产生买卖、仓位、目标价或收益承诺。共同边界由 `app/prompts/stock_soul.md` 提供，版本化方法卡位于 `app/prompts/hot_money_early_v1.md` 和 `app/prompts/value_compounder_v1.md`。

## 真实模型链路

前端把当前模型连接、最近 12 条对话历史、模式和最多 50 只自选股上下文传给 Tauri。后端通过 `rig-agent` 运行 Agent 状态机和工具循环，通过 `rig-core` 选择 OpenAI Chat Completions、OpenAI Responses、Anthropic 或 OpenAI-compatible model；协议请求、流式响应和 provider 错误不再由 `agent_harness` 手写：

1. 校验模型地址与凭据边界；公网端点必须使用 HTTPS，基础地址按所选协议规范化。OpenAI-compatible 的 `full_url` 通过 Rig provider extension 覆盖 completion path，可使用 `/generate` 等自定义路径；Responses/Anthropic 的自定义 full URL 会安全拒绝并回退本地结果。
2. 将历史转换为 Rig `Message`，将工具结果和证据作为 Rig context/document，并由 `AgentRunner` 管理多轮状态。
3. Rig 负责 JSON/schema、请求、流式响应、工具调用和 provider 错误；应用边界仍校验 endpoint、大小、凭据脱敏、代理和取消。模型请求上下文、模型输出、工具参数和工具结果均有独立上限。
4. 基础地址和完整 URL 配置在 Rig provider builder 前归一化，应用不再拼接 provider request body 或解析 SSE。
5. 仅接收 `reply`、`answer_sections`、`warnings`、`next_actions` 四类模型字段；摘要和每条事实 bullet 都必须邻近引用证据目录中的有效 `[E#]`，缺失或未知编号会回退本地结果。
6. 拒绝直接交易、收益承诺和操纵市场内容，并始终补上“仅供选股研究，不构成投资建议”。

本地 Ollama、LM Studio、vLLM 等兼容服务可以不填 API Key。HTTP 仅允许 loopback 或私有局域网 IP 的本地模型；公共远程模型必须使用 HTTPS。专家和研报模式只会使用应用内显式配置后传入的连接及其自身凭据，未传连接配置时会回退本地工具结果，绝不会继承 `OPENAI_*` 环境密钥。IPC payload 限制为 512 KiB、问题限制为 8000 字符、模型请求与响应体各限制为 2 MiB；工具上下文被压缩时会向模型标记 `tool_result_truncated=true`。远程服务错误会脱敏。

执行期间 Tauri 会依次推送 Rig 兼容的本地工具、模型综合、证据校验和完成状态；界面不会在长模型请求期间一直停留在“准备中”。`quick / deterministic_v1` 使用同一 Rig 工具 registry，但跳过 provider 和模型调用。

## 对照评测

`app/prompts/agent_harness_eval_cases.json` 固化了四类共同问题、两套 profile 的必需/禁止词、逐案例黄金输出和越界样例。以下测试会对每个问题同时生成专家模式与研报模式 prompt，并将对应输出实际送入安全合并器，验证：

- profile 版本映射正确且方法不串台；
- 共同的证据、风险边界存在；
- 模型综合不会覆盖工具事实；
- 直接交易、收益承诺和操纵市场输出在两套模式下都会被拒绝。

运行：

```powershell
cargo test agent_harness_prompt_profiles_pass_contrast_evaluation --lib
```

该评测是可复现的离线契约评测，不依赖某个在线模型的随机输出。更换方法卡或升级 profile id 时，应同步更新评测夹具并保留旧版本结果，避免静默改变既有对话语义。
