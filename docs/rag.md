# RAG 2.0 研究消息中心

RAG 2.0 由 Tauri/Rust 统一维护新闻、公告、财务快照、研报、社区线索和用户导入资料。消息页是面向自选股的研究工作区，而不是独立的建包工具页。

## 数据库

默认数据库为 Tauri AppData 下的 `research/research.sqlite`，`PRAGMA user_version=2`。核心表包括：

- `documents`、`chunks`、`chunks_fts` 和 `embeddings`；
- `research_messages` 及应用内未读状态；
- `research_threads`、`research_answers` 和 `research_answer_citations`；
- `research_metadata`，记录迁移状态。

文档更新采用内容哈希增量导入。内容未变化时保留消息已读状态和向量；已被历史回答引用的文档发生变化时创建确定性的 revision，旧分块不会被删除。

首次打开时会事务化迁移 `news-cache.json`、旧 `rag-pack.json` 及实际为 JSON 的旧 `.sqlite` 包。迁移完成前不删除旧文件。

## 检索

中文文本先由 `jieba-rs` 分词，再写入 SQLite FTS5。BM25 字段权重为标题 3、实体/股票 2、正文 1。股票代码过滤在 Top 50 截断前完成。

Windows 同时运行本地向量检索：

- 模型：BAAI/bge-small-zh-v1.5 的 Xenova INT8 ONNX 版本；
- 输出：512 维归一化 `f32`，以 BLOB 存储；
- 资源：约 24 MB，构建前逐文件校验 SHA-256；
- 执行：每批 256 个分块，后台生成，不阻塞 UI。

BM25 和向量各取 Top 50，以 RRF `k=60` 融合。每篇文档最多返回两个分块，最终最多八条引用。Android 不加载 ONNX/fastembed，只运行同一套 Jieba + FTS5 BM25。

## 证据门禁

来源优先级固定为：

1. `filing`：公告、财报、交易所或公司正式披露；
2. `financial_snapshot`：财务快照；
3. `news`：可信新闻；
4. `research_report`：研报和用户导入资料；
5. `community`：社区讨论与传闻。

社区证据不能单独支撑事实结论。仅命中社区时保持证据模式，也不会调用聊天模型。配置聊天模型后，模型只能使用当前返回的 `[C1]` 等引用；缺失引用或生成未知引用编号时自动退回证据模式。

URL 导入仅接受公网 HTTPS。每次重定向都重新解析和校验 IP，拒绝凭据、回环、内网、链路本地和 IPv4-mapped IPv6 私网地址。URL/PDF 用户导入固定按 `research_report` 入库，调用方不能冒充公告层级。

PDF 限制为 25 MB、500 页，并使用受限解压逐页提取文字和页码。扫描件没有可提取文本时提示需要 OCR，首期不内置 OCR。

## 研究接口

前端通过 Tauri bridge 使用：

- `/api/research/overview`、`messages`、`mark-read`、`refresh`；
- `/api/research/query`、`threads`、`threads/create`、`threads/detail`；
- `/api/research/index-status`、`rebuild-index`、`rebuild-embeddings`；
- `/api/research/import-url`、`import-pdf`；
- `/api/research/pack/export`、`pack/import`、`pack/rollback`。

`api_news_rag`、`api_rag_pack_*` 和旧同步接口保留一个兼容周期，由适配器继续服务 Agent 页面和旧调用。

## 页面行为

桌面消息页为三栏：自选股收件箱、事件流与问答、引用证据检查器。知识库导入、同步、回滚、索引和模型设置集中在管理抽屉，高级诊断中才显示原始 JSON。

Android 将自选股收件箱改为抽屉、证据检查器改为底部面板，并隐藏建库和 embedding 管理。应用在前台且在线时每 15 分钟增量刷新，隐藏或离线时暂停；只维护应用内未读数，不发系统通知。

## 保留策略

新闻和社区默认保留 365 天；索引最多 50,000 个分块。公告、用户导入、已固定或已被引用的资料不会被自动清理。

## 首期边界

首期不做 OCR、全市场后台爬取、系统通知、聊天跨设备同步、Android 向量推理和本地生成式大模型。自然语言综合回答继续使用用户配置的聊天模型。
