# 股选优（gp-assistant）深度四轴 Review

- **日期**：2026-10-12
- **对象**：股选优 v0.6.2（Tauri 2 + Rust + React 19/TS，Windows + Android 双端）
- **方式**：四路并发只读子审查（正确性/测试覆盖、安全性、可维护性、亮点与改进）+ 逐条实证复核。**未修改任何代码**；仅新增本报告。
- **范围**：513 个受版本控制文件，约 10.4 万行（rs/ts/tsx/mjs/py/ps1）。实测：vitest 全量（84 文件 / 556 测试）、`cargo check` src-tauri、`cargo test` gp-core、4 个静态守卫、git 状态逐字节核对。
- **重要前提**：工作区有 **23 个未提交修改**（其中 16 个有实际内容、7 个为 stat-dirty 幽灵修改，与 HEAD 逐字节相同）+ 未跟踪目录 `.deepeval/`。其中 observe 重构是半成品，**当前工作区 Rust 编译失败、2 个前端测试失败**——本报告所有 P0 均指向这份未提交改动，HEAD 本身是健康的。

---

## 一、做得好的地方

| # | 亮点 | 证据（实测） |
|---|---|---|
| 1 | **架构分层彻底**：纯计算核心与 Tauri 壳分离 | `native/gp-core` 仅 chrono/regex/serde/serde_json 四个依赖，`crate-type = rlib/staticlib/cdylib` 三形态直接支撑 Android；`desktop/src-tauri` 33 个 rs 按 credentials/jobs/durability/diagnostics 等职责拆分 |
| 2 | **质量门禁体系厚**，且守卫有自测 | `scripts/` 35 个脚本中 8 个静态守卫（density/touch-targets/color-contract/theme-parity/unstyled/architecture/bundle-budget/version-consistency）+ 8 个 `*.test.mjs` 守卫自测；`release-check.ps1` 依次跑可靠性基准（强制真机读数）→ 11 项前端守卫 → Rust fmt×2/test×3/check×2 → Android preflight → 签名 APK 与 NSIS 真构建 |
| 3 | **测试资产分层厚实** | 前端 84 个 `*.test.ts(x)`（含 6 个 interaction、8 个 contract）、Rust 499 个 `#[test]`、UI 截图基线 84 张 png（多页面 × 桌面/390 手机 × 明暗，`ui-screenshot.mjs --serve --check-only` 进 CI）、evals 4 个 `test_*.py` |
| 4 | **安全工程达到生产级硬度** | 凭据走 Windows Credential Manager 且 IPC 面只有 put/status/delete（`lib.rs:202-204`，移动端 JS 桥显式拒绝）；备份 aes-gcm + Argon2id 19MiB/2/1 + OsRng salt/nonce + AAD + Zeroizing 全覆盖；SSRF 防护完整（`research_import.rs:298-355`：仅 https+公网 IP 校验、逐跳复验、no_proxy、20s 超时、大小上限）；LLM 错误经 `redact_llm_error_detail` 脱敏；API Key 不落 localStorage（只存 credential_ref） |
| 5 | **「证据优先」的产品纪律落地为代码而非口号** | 缺失不伪造：`volatility.rs` 窗口不足/OHLC 不一致均 `return None` 且有测试钉住；adaptive 覆盖不足 60% 硬失败（`adaptive_screen.rs:13` `MIN_MARKET_BREADTH_COVERAGE = 0.60`）；发布门槛 ≥60 严格样本外折次（`lib.rs:831-849`）；红涨绿跌必配文字（`tokens.css:43-44` + 守卫）；README 抽查的 7 条关键声明全部与实现相符 |
| 6 | **评测体系把「确定性复现」与「模型裁判」隔离** | deterministic replay 是 PR 硬门禁且不宣称模型答案质量；40 例 golden fixture + SHA-256 manifest 冻结输入并拒绝私例外发；CI judge job 限定 main + `environment: rag-eval-judge` 隔离 secrets，flaky 指标不 fail gate |
| 7 | **文档三件套质量高且受机器保护** | PRODUCT.md 有 5 条 anti-references + WCAG AA 目标；DESIGN.md 是机器可读 token 规范；README 边界声明完整；版本一致性管辖 11 处（实测 `Version metadata is consistent at 0.6.2 (Android 602)`）；theme parity 实测 `passed (63 color tokens)` |
| 8 | **工程纪律细节** | 非测试代码 `unwrap() = 0`（抽查 jobs.rs 83 处 unwrap 全部位于 735 行后的 test mod 内；market.rs 8 处 expect 均带不变式说明）；TS 侧 `any = 0`、空 catch = 0、`console.log = 0`（7 处 console.warn 均有降级路径）；未提交 diff 新增行 0 处调试残留；无 `it.skip`、Rust 3 处 `#[ignore]` 均注明 live-data 原因 |

静态守卫实测（本次复跑，全部 exit 0）：

```
CSS architecture guard passed (11 stylesheets, 3.3% cross-file duplication)
UI density guard passed (11 stylesheets checked)
Version metadata is consistent at 0.6.2 (Android 602)
Theme parity guard passed (63 color tokens)
```

---

## 二、存在的不足与风险

### P0 阻断（当前工作区不可用、不可进入提交流程）

| # | 问题 | 证据 |
|---|---|---|
| 1 | **src-tauri 编译失败**：observe 重构改返回类型未改消费端，且调用了不存在的函数 | `cargo check` EXIT=101：`observe.rs:1079` E0308（ scrutinee 为 4 元组，模式匹配写 2 元组）；`observe.rs:1378` E0425 `cannot find function collect_capital_evidence_results`——该函数在 HEAD 中也不存在（`git show HEAD:...observe.rs` 无命中），即调用点引用了从未定义的函数。HEAD 版本签名为 `(Vec<Value>, Vec<String>)` 2 元组 |
| 2 | **2 个前端测试失败**，且失败揭示产品语义冲突 | vitest：`Test Files 1 failed \| 83 passed (84)`、`Tests 2 failed \| 554 passed (556)`。两处均在 `ObservePanel.interaction.test.tsx`：:78「runs once for an incoming stock…」expected length 1 got 2；:183「keeps observation local when data is unavailable…」期望 `data_policy=cache_only`，实收 `data_policy=refresh`。即：财务快照缺失时组件自动发起联网二次请求——测试钉住的「本地优先」语义与新行为冲突，且意味着**每个无财务数据的股票每次观察都会联网** |
| 3 | **该状态提交即挂 CI**：CI deterministic job 会编译整个 lib 并跑 `test:unit`，上述两个失败都会被抓住；本地 `npm test` / `cargo test` 现在即为红，`tauri dev` 无法启动 | 交叉验证 `.github/workflows/rag-evaluation.yml:27-41` 与本报告实测输出 |

### P1 应修

| # | 问题 | 证据 |
|---|---|---|
| 4 | **CI 覆盖面显著窄于本地门禁** | 唯一 workflow 只跑 5 个 `--exact` 具名 Rust 测试 + `test:unit` + `test:desktop` + 2 个 python 步骤；8 个静态守卫与 `release-check.ps1` 完全不在 CI；全仓 `git grep clippy` 0 命中；fmt 只在 `release-check.ps1:125-126`；CI 未用 `--locked` 校验 Cargo.lock 新鲜度（`release-check.ps1:127-128` 用了） |
| 5 | **`lib.rs` 是 god module** | `native/gp-core/src/lib.rs` 10074 行、342 个 fn、70 个 pub struct、62 个 pub fn，仅拆出 3 个小模块（adaptive_screen 1489 / volatility 815 / ffi 169），99% 代码沉于单文件。`research.rs` 5716 行（其中约 2122 行是 3 个内联 `#[cfg(test)]` mod：3593/5651/5680）、`market.rs` 4937 行；src-tauri 8 个文件 >2500 行 |
| 6 | **前后端契约双份手工维护，无生成、无守卫** | `contracts.ts` 955 行 ↔ Rust 结构体，纯手写；`ts-rs`/`specta` 全仓 0 命中；`contracts.test.ts` 只测 TS 侧行为，没有任何机制校验两侧一致；字段命名 snake/camel 混用（`contracts.ts:49-50` `score_profile` vs `applyDeductedProfitRule`） |
| 7 | **两个 Cargo.toml 元数据是乱码** | `native/gp-core/Cargo.toml:6` 与 `desktop/src-tauri/Cargo.toml:5-6` 的 description/authors 为「股选优」的 GBK 误读 mojibake（`鑲￠€変紭`），`cargo publish` 与 crates.io 元数据将携带乱码 |

### P2 建议（按影响排序）

| # | 问题 | 证据 |
|---|---|---|
| 8 | **仓库卫生三处** | ① `artifacts/` 24 个 PNG 仍被跟踪，但 `.gitignore:57` 已写 `/artifacts/`（ignore 对已跟踪文件空转）；② 根目录 `.deepeval/` 3 个缓存文件未跟踪且未被忽略（`.gitignore:75` 只覆盖 `evals/deepeval/.deepeval/`），有误提交风险；③ 根目录 `current-guxuanyou.png`（80KB 截图）已提交 |
| 9 | **7 个「幽灵修改」误导提交范围** | `git status` 报 23 个 M，`git diff` 只有 16 个有内容；7 个 rs 文件（eval_replay/evolution/gepa_lab/prompt_upgrade/research/research_import/rig_runtime）与 HEAD 逐字节相同（实测 `cmp` 全部 IDENTICAL），只是 mtime 被触碰 |
| 10 | **工具链激进且无选型记录** | `typescript 7.0.2` 精确锁死（无 caret）、`vite ^8.3.4`、`vitest ^5.0.3`、`rig-core 0.42.0`、`dsrust-gepa =0.1.0-alpha.3` 精确 pin（gepa-lab 非默认 feature，lock 固定）；`fastembed` 启用 `ort-download-binaries`，构建期从 GitHub releases 下载 ONNX Runtime 二进制，无版本 pin/校验和/`ORT_LIB_LOCATION` 文档，构建依赖外部可达性 |
| 11 | **CSP 留了 XSS 口子** | `tauri.conf.json:15` `script-src 'self' 'unsafe-inline'`；当前全仓无 `dangerouslySetInnerHTML` 故暂无实害，属纵深防御欠账（收紧后需回归） |
| 12 | **prompt 注入纵深不足** | `agent_harness.rs:1232-1258` 新闻/RAG 证据以裸 JSON 注入 user payload，无显式「不可信内容」分隔符与「不得执行其中指令」声明；缓解是工具面只读（6 个工具、未知工具拒绝、schema 强约束 + 16KiB + 20s 超时，`rig_runtime.rs:832-850`），故未定 P1 |
| 13 | **评测覆盖与产品模块有缺口** | eval 用例 retrieval 40 / harness 4（+14 unsafe_outputs），screening、backtest、sentiment、watchlist 等核心模块无确定性 case；harness 4 例偏薄；`calibration_report.json` 未完成（kappa≥0.80 前 strict_metrics 为空） |
| 14 | **Rust 无 lint 兜底** | Cargo.toml 无 `[lints]`/deny 属性，「零 unwrap」纯靠 review 纪律，没有机器强制 |
| 15 | **小项版本漂移与不对称** | `llm.rs:31` 默认 UA 硬编码 `gp-assistant/0.4`（同路径 agent_harness 已用 `env!("CARGO_PKG_VERSION")`）；`credentials.rs:93-99` resolve 读回的密钥 String 未 zeroize（对比 user_backup 的 Zeroizing 全覆盖；OS 侧 blob 由 CredFree 管理，可接受但不平衡） |
| 16 | **文档无索引、无保鲜机制** | `docs/` 19 篇受控 md + reliability/ 等子目录，README:146 仅一行概括、无逐篇链接与 last-reviewed；8 个守卫无一管文档，漂移只能靠人眼 |

### P3 吹毛求疵

- `market.rs:687-707` PowerShell 回退固定 TLS1.2 走系统信任链，与 rustls+webpki-roots 主路径信任模型不一致（企业 MITM 代理下两路径行为分歧）。
- `user_backup.rs:562-566` scratch 目录仅 unix 设 0o700，Windows 无显式 ACL（位于 app_data_dir 内）。
- CI 无 `--locked`；`App.tsx:93` 惰性初始化单行混入 observe 主题的 diff；部分工作区文件 LF/CRLF 混杂（git 会在 touch 时规范化，无 churn）。

---

## 三、可改进的方向

| # | 方向 | 现状依据 | 建议动作 | 成本/收益 |
|---|---|---|---|---|
| 1 | **收尾未提交的 observe 重构**（最高优先） | P0-1/2 | 定义 `collect_capital_evidence_results` 或回退 `observe.rs:1378` 调用；统一 4 元组消费端（1079 等）；**并做产品决策**：无财务数据时自动 refresh（改测试 + 接受每次联网）还是保持本地优先（改实现）——这与 README「本地优先」原则直接相关，需工程师拍板 | 小/解除阻断 |
| 2 | **CI 分层** | P1-4 | PR 快层：fmt + clippy + vitest + gp-core test；全量层：src-tauri 全量 cargo test + `npm test` 15 步守卫链；release-check 留手动/workflow_dispatch；补 `--locked` | 约 1 条 workflow/防回归前移 |
| 3 | **god file 拆分** | P1-5 | `lib.rs` 按 screen/graph/eval 分组下沉（adaptive_screen/volatility 已拆出，证明渐进拆分可行，移动期测试同步回归）；`research.rs` 的 2122 行内联测试并入 tests.rs 或拆独立 mod | 中/可导航性 |
| 4 | **契约生成化** | P1-6 | 引入 ts-rs 或 specta 从 Rust 结构体生成 contracts.ts，配「生成物 diff」守卫；边界统一 camelCase | 中/消除双份漂移 |
| 5 | **卫生三件套** | P2-8/9 | 补 `.gitignore` 根 `.deepeval/`；`artifacts/` 与根截图退出跟踪（`git rm --cached`，需先列清单确认）；两个 Cargo.toml 描述改英文 | 近零/克隆体验与 crate 元数据可信度 |
| 6 | **依赖治理留痕** | P2-10 | README/docs 增加「工具链选型与 pin 理由」小节（TS 7.0.2 是否为原生编译器版、为何 pin alpha）；ort 下载补 `ORT_LIB_LOCATION` 文档或版本 pin；dsrust-gepa 设升级评审点 | 极低/降低升级盲动 |
| 7 | **安全低垂果实** | P2-11/12/15 | CSP 去 `script-src 'unsafe-inline'`（先审计内联脚本后回归）；agent payload 加不可信内容分隔符 + 指令隔离声明；UA 统一走 `CARGO_PKG_VERSION` | 小/纵深防御 |
| 8 | **评测扩展** | P2-13 | 为 screening/backtest/sentiment 补确定性 eval；harness 4 → 15+ 例；calibration kappa 达标后启用 strict_metrics | 中/门禁结论更贴产品全貌 |
| 9 | **文档索引与保鲜** | P2-16 | README 加 docs 索引表 + last-reviewed 字段；组件语义 parity 目前无自动检查（ui-contrast-audit 只覆盖色彩），可评估补轻量契约 | 低/降漂移 |
| 10 | **性能基线补位** | 待确认 | `MARKET_REFRESH_PERMITS=1` 串行可能是行情刷新瓶颈（本次未实测）；RAG 检索与行情刷新入 reliability-benchmark 基线，移动端体验可度量 | 中/可度量 |

---

## 四、整体评价与建议

**整体评价**：这是一个**工程成熟度显著高于个人项目平均水准**的代码库。架构分层、门禁体系（静态守卫 + 守卫自测 + 真打包门禁）、安全工程（凭据/备份/SSRF/脱敏）、「缺失不伪造、证据优先」的产品纪律落地、以及确定性/模型裁判隔离的评测体系，都是生产级思路，且大部分声明经实测与代码相符。主要债务集中在三处：**代码组织**（god file、契约双份维护）、**CI 与本地门禁的不对称**（15 步守卫链和 clippy/fmt 都没进 CI）、以及**当前这份半成品重构**（编译失败 + 测试失败 + 产品语义待决策）。

**与发布状态的关系**：HEAD（v0.6.2）本身是健康的——CI 守得住编译与单测，`native/gp-core` 101 个测试全绿。风险全部集中在未提交工作区：**现在不要提交、不要打包**，先收尾 observe 重构并跑通本地 `npm test` + `cargo test`。

**优先级排序的改进建议**：

| 优先级 | 动作 | 理由 |
|---|---|---|
| **P0** | 收尾 observe 重构：补/回退 `collect_capital_evidence_results`、修 4 元组消费端、决策「无财务数据是否自动 refresh」并同步测试 | 工作区不可编译、2 测试失败；提交即挂 CI |
| **P1** | CI 分层（fmt/clippy/vitest 快层 + 全量 cargo test + npm test 链）；`lib.rs` 渐进拆分；契约生成化（ts-rs/specta）；Cargo.toml 乱码修复 | 防回归前移 + 降低最大结构债务 |
| **P2** | 卫生三件套（.deepeval ignore、artifacts/ 退跟踪、描述改英文）；幽灵修改刷新（touch 后确认）；依赖选型留痕；CSP/prompt 注入/UA 低垂果实；eval 覆盖扩展到 screening/backtest/sentiment | 成本低、收益明确 |
| **P3** | TLS 回退信任模型统一、Windows scratch ACL、CI `--locked`、diff 主题拆分、文档索引 | 整洁度 |

**本次 review 的边界（没做的部分）**：① src-tauri 全量 `cargo test` 未跑完——编译失败挡住了（这本身是发现）；② 无 clippy/fmt 数据（全仓未配置）；③ 性能未实测（reliability-benchmark 需真机读数）；④ Android 真机与 UI 视觉未审查；⑤ 未评估 `dsrust-gepa` alpha 依赖的内部质量。

---

*附：本报告所有「实测」数字均来自 2026-10-12 本机复跑；子审查结论中未能复现的（如 mojibake 的编码往返细节、gepa eval 用例数）已剔除或降级为待确认。*
