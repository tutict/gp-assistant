# 检索增强生成设计

当前 RAG 路径由 Tauri/Rust 维护，不再依赖 Python/FastAPI、ONNX Runtime 或 sqlite-vec 的旧实现。桌面端提供轻量 Rust lexical RAG pack 构建、查询和上游证据包内联导入 JSON；移动端通过扫码或粘贴 manifest/descriptor 导入数据包。

## 目标

- 回答 A 股产业链问题时给出可追溯证据。
- 将检索范围限定在已有股票池、关系图和用户导入的数据包内。
- 区分事实证据、社区讨论和市场传闻。
- 支持移动端离线检索，不在 APK 中嵌入额外后端。
- 第一版移动端避免双向同步，便于调试和复现。

第一版不做：

- 面向任意文档的通用股票问答。
- 从新闻正文自动发现供应链关系。
- 桌面端和移动端之间的双向同步。
- 移动端写入 RAG 索引后的冲突解决。
- 把社区帖子当作事实证据使用。

## 用户流程

桌面端流程：

1. 抓取或导入新闻、公告和社区帖子。
2. 将每条内容规范化为 document。
3. 将文档切分为 chunks。
4. 构建轻量 Rust RAG pack 和 manifest。
5. 生成可扫码或可粘贴的内联 descriptor JSON。

移动端流程：

1. 扫码或粘贴 descriptor JSON。
2. 校验 manifest。
3. 原子替换本地只读数据包。
4. 用 Rust lexical 检索返回相关 chunks。
5. 返回标题、URL、来源层级、置信度和待核查项。

## 当前接口

本项目仍保留 `/api/rag-pack/*` 和 `/api/upstream-rag/*` 形式的前端调用语义，但在 Tauri 中这些请求会桥接到 Rust command，而不是发往独立 HTTP 服务。

- `api_rag_pack_status`
- `api_rag_pack_build`
- `api_rag_pack_build_from_news_cache`
- `api_rag_pack_query`
- `api_upstream_rag_status`
- `api_upstream_rag_build`
- `api_upstream_rag_transfer_start`
- `core_upstream_rag_import`
- `core_upstream_rag_list`
- `core_upstream_rag_detail`
- `core_upstream_rag_rollback`

默认数据包路径位于 Tauri AppData，由 Rust command 负责创建和查询。

## 来源层级

- `filing`：公告、财报、交易所披露。
- `news`：传统财经媒体、券商研报摘要、公司新闻。
- `community`：股吧、社区讨论、市场传闻。
- `manual`：用户手工粘贴或扫码导入的证据。

事实判断优先使用 `filing` 和 `news`。`community` 只作为情绪、讨论热度和待核查线索，不直接当作事实证据。

## 检索单元

每个 document 至少包含：

```json
{
  "source": "东方财富股吧",
  "source_tier": "community",
  "title": "讨论标题",
  "text": "正文或清洗后的片段",
  "url": "https://example.test/news",
  "published_at": "2026-06-28",
  "stock_codes": ["000100.SZ"],
  "relation_types": ["supply_chain"],
  "sentiment": "neutral"
}
```

每个 chunk 至少保留：

- `chunk_id`
- `document_id`
- `text`
- `title`
- `url`
- `source`
- `source_tier`
- `published_at`
- `stock_codes`
- `relation_types`
- `sentiment`

## 移动端边界

移动端只消费已经构建好的轻量数据包或 descriptor，不承担复杂采集任务。移动端可以做：

- 校验 manifest。
- 导入、列出、回滚 RAG 包。
- 基于本地包做 lexical 检索。
- 在消息页面把命中证据合并到利好/利空分栏。

移动端不做：

- 批量抓取全市场新闻。
- 建立长期后台采集服务。
- 自动生成供应链关系。
- 把社区帖子升级为事实证据。

## 后续方向

- 在 Rust 侧继续增强 tokenizer 和字段权重。
- 给导入包增加更明确的版本兼容检查。
- 为 Android 弱网场景增加更短链路的数据源和超时降级。
- 保留 schema 稳定性，后续如引入向量索引，只替换检索实现。