# 后端检查记录 — 2026-11-13

## 升级后的复测记录

下文保留原检查时的发现与证据。当前 v0.6.4 工作区已处理 B1–B5，并补齐“文件已替换但目录同步失败”的额外缓存边界：已替换的资料推进请求代次并保留同步告警，较早请求不能覆盖它；替换前失败仍允许较早请求保存。

2026-11-13 的实际复测结果：公司资料回归 12 项通过；核心 128 项通过，另有 8,000 只股票性能样例通过（10 次平均约 324ms，门槛 500ms）；研究模块独立锁定测试 80 项通过；Tauri 默认与全部可选特性各 819 项通过、8 项显式忽略；三个模块的格式与 Clippy 命令退出 0，Clippy 仍有既存警告。Tauri 完整测试包含响应长度／分块读取限制及存储替换故障回归。

本次复测日志与机器可读结果位于 `C:/Users/tutic/.codex/visualizations/2026/11/13/01a1d4dd-76fb-7663-9c0f-a1b4f9e10cfe/release-v0.6.4/`。上述结果证明本地回归与构建兼容性；设备性能、真实 PIT 数据与安装包发布另行核验。

## 原检查记录

本次 LoopPilot 检查已完成，确认 **5 项 P2 问题：3 项缓存／读取缺陷，2 项工程问题**。这些问题仍需修复；通过的现有测试不能替代缺失的边界验证。

检查当前工作区的 Rust/Tauri 后端、`native/gp-core` 及 `native/gp-research`。Python 目录主要提供提示词与评测材料，不是当前产品后端。采用 Lightweight 检查流程：确认入口与契约 → 执行现有检查 → 根据失败调整验证方法 → 隔离复现 → 核对证据并交付。本次没有修改产品源码、原锁文件或发布配置，已有页面优化与 A 股改动保留。

## 已确认问题

| 编号 | 优先级 | 问题 | 影响与来源 |
| --- | --- | --- | --- |
| B1 | P2 | 较早的公司资料请求能覆盖较新的缓存 | 并发刷新后资料与抓取时间回退，随后普通读取持续使用旧值。[cn_market.rs:72](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/cn_market.rs:72) |
| B2 | P2 | 公司资料缓存读写大小约束不一致 | 写入成功的资料，下一次读取会被同一模块拒绝。[cn_market.rs:60](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/cn_market.rs:60)、[写入:94](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/cn_market.rs:94) |
| B3 | P2 | 公开数据响应的 10MiB 限制检查过晚 | 完整响应先被缓冲，异常大响应在拒绝前已占用读取资源，并行分页放大影响。[cn_market.rs:611](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/cn_market.rs:611) |
| B4 | P2 | 研究模块锁文件保留旧核心版本 | 独立 `--locked` 构建／测试被阻断；Tauri 的依赖构建不会检查这份独立锁文件。[Cargo.lock:481](C:/Users/tutic/IdeaProjects/gp-assistant/native/gp-research/Cargo.lock:481) |
| B5 | P2 | Tauri Rust 格式检查不通过 | 现有发布流程在格式门槛处失败。[release-check.ps1:126](C:/Users/tutic/IdeaProjects/gp-assistant/scripts/release-check.ps1:126) |

### B1：公司资料缓存更新没有请求顺序保护

`load_cn_company_profile` 读取缓存后等待网络结果，再直接原子替换文件。它没有按股票串行化，也没有在发布时检查请求代次／更新版本。桌面与 Android 的网络并发许可均允许两个请求同时进行。

复现中，让请求 A 先开始并等待，让较新的 B 先返回并写入，再放行 A：文件内的 `fetched_at` 从 `1794568760713` 回退到 `1794568759713`，公司业务文字从较新值回退为较早值。随后的普通读取返回较早资料，`stale=false`。这里较早资料仍处于七天 TTL 内，问题是更新回退，而非已经过期的内容被伪装成有效。

建议：按股票串行化缓存更新，或在提交阶段使用请求代次／版本比较。重新检查当前缓存后再发布，避免较早响应覆盖已提交的新值。回归应涵盖后发先完成和快速离开再返回详情的情况。

### B2：缓存会拒绝自己刚写入的内容

读取上限为 `262144` 字节；写入前只检查字段和 JSON 是否有效，没有使用同样的大小限制。上游公开数据读取允许到 10MiB，因此这类输入没有被外层的尺寸约束挡住。

复现写入了一份 `262403` 字节的有效资料：首次返回 `error=null`，下一次普通读取却返回“公司资料缓存无效，请刷新公司资料。”。

建议：写入前检查序列化后的字节数；超限应明确报告，保留现有缓存。不要无提示地截断原始公司资料。回归须验证阈值边界及“任何成功保存的缓存都可再次读取”。

### B3：尺寸上限不能限制实际读取

`reference_bytes` 先执行 `response.bytes().await`，随后才比较长度与 10MiB 上限。时间超时也不能阻止快速大响应在期限内被完整缓冲。

用回环 HTTP 服务返回带 `Content-Length` 的 25MiB 内容，服务端完整发送 `26214400` 字节，函数才返回“公开数据单页超过读取上限”。这是实际传输验证，没有测量或宣称进程 RSS 数值。

建议：先检查可用的 `Content-Length`，再按流式分块读取，在扩展缓冲区前检查累计字节数。未知长度和分块响应也要受相同约束。已有文章摘要抓取路径使用流式上限，可作为实现参考。

### B4：独立研究构建无法按锁文件执行

`native/gp-core/Cargo.toml` 声明 `0.6.3`，研究模块锁文件仍记录 `stock-optimizer-core 0.6.2`。原工作区执行以下命令退出 `101`，未进入测试：

```powershell
cargo test --locked --offline --manifest-path native/gp-research/Cargo.toml
```

在隔离复制的两个原生 crate 中重新生成锁文件，仅产生这一处核心版本变化。随后研究模块的 **78 个测试通过**。这证明是锁文件同步问题；原文件没有被修改，也不能把隔离结果描述为原锁定构建已通过。

建议：同步该锁文件，并把研究库的独立测试加入常规验证。当前 `release-check.ps1` 的 Rust 检查只覆盖 gp-core 和 Tauri，无法发现这一独立锁文件问题。

### B5：当前 Rust 发布门槛被格式检查阻断

gp-core 和 gp-research 的 `cargo fmt -- --check` 通过，Tauri 检查退出 `1`。格式日志有 9981 行，覆盖多处格式差异；这是格式门槛失败，不是 9981 个业务缺陷。本次没有为消除差异自动改写这些文件。

建议：在保留现有改动的前提下统一 Rust 格式，核对实际 diff，再执行原发布检查。不要关闭格式门槛。

## 实际验证结果

| 检查 | 结果 | 证据 |
| --- | --- | --- |
| gp-core 锁定、离线测试 | 128 通过，1 个显式忽略的性能基准 | [日志](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/gp-core-tests.log) |
| Tauri 默认功能测试 | 811 通过，8 个显式忽略测试 | [日志](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/tauri-tests.log) |
| Tauri 全功能测试 | 811 通过，8 个显式忽略测试；包含 GEPA 与 eval-replay 编译路径 | [日志](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/tauri-all-features-tests.log) |
| gp-research 原锁定测试 | 退出 101，锁文件需要更新 | [日志](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/gp-research-tests.log) |
| gp-research 隔离修正锁文件后 | 78 通过，包括长窗口校准及 PIT／数据库边界用例 | [日志](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/gp-research-isolated-tests.log)、[唯一锁文件差异](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/gp-research-lock-diff.patch) |
| 三个模块格式检查 | 核心和研究通过，Tauri 失败 | [结果](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/format-results.json) |
| Clippy | 全部运行命令退出 0；Tauri lib-test 汇总 114 条警告（含 75 条重复），核心 lib-test 25 条（含 21 条重复）；隔离研究检查无警告 | [Tauri](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/tauri-clippy.log)、[核心](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/gp-core-clippy.log)、[研究](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/gp-research-isolated-clippy.log) |
| 三项控制复现 | 并发缓存回退、不可再次读取的保存、响应限额检查过晚均复现 | [实际结果](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/profile-probe/evidence/report.json)、[运行日志](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/profile-probe.log) |

复现采用从当前源码直接提取的缓存函数、响应读取函数及原 `durability.rs`。股票规范化只使用规范代码 `600519.SH` 的桩，不参与上述缺陷。缓存文件位于检查材料目录，网络用例仅访问 `127.0.0.1`，没有访问线上提供方或真实用户资料。复现程序退出 0 表示预期缺陷现象已观察到。[源函数摘要](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/profile-probe/source-manifest.json)记录提取边界及散列。

## 复核与范围

- **Spec**：公司资料更新一致性、缓存读写边界和响应限制存在上述缺陷，需要修复。
- **Standards**：Tauri 格式门槛和研究独立锁定构建未通过；测试门槛没有被放宽。
- **Implementation Quality**：原子替换保证单次文件发布，不保证并发更新顺序；读写与读取资源限制需统一。Clippy 的未使用代码、简化表达式等警告未全部算作业务问题。
- **Verification & Evidence**：本地测试、实际退出码和控制复现均已记录；隔离修正与原工作区结果明确区分。

重新核对了此前的后端存储审查：自选数据库损坏时被重新创建、Prompt overlay 损坏后用默认值覆盖、研究库未来 schema 在拒绝前被修改，这些旧项当前已有修复与通过的回归。行情补丁写入入口也有全局更新锁，未把仅观察到内部读改写代码的表面风险列为并发缺陷。

尚未验证：显式忽略的线上源／真实缓存测试、核心人工性能基准；Android 特有运行分支、真实提供方和真实模型集成；真实 PIT 数据的发布门槛。此次本地检查不能替代这些结果。

[机器可读检查汇总](C:/Users/tutic/IdeaProjects/gp-assistant/artifacts/backend-audit-2026-11-13/verification-summary.json)保留各命令退出码、测试数量、格式结果和控制复现。

本次停止原因：检查、复现与问题清单已经交付。下一步建议先为 B1–B3 添加生产模块内的失败回归并修复，再同步研究锁文件和 Rust 格式，运行同样的测试与完整发布检查。
