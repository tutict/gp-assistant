# 检索增强生成设计

本文说明本项目中“有证据支撑的产业链检索增强生成”的产品设计和工程边界。当前实现支持限定范围的消息检索、SQLite 缓存、来源层级标签、基于规则的结论，以及可复用的离线数据包构建与查询路径。产品构建使用本地 `bge-small-zh-v1.5` ONNX/INT8 向量模型，并通过 ONNX Runtime 运行。确定性哈希向量器只作为显式测试夹具保留，默认不会用于产品路径。

## 目标

- 回答 A 股产业链问题时给出可追溯证据。
- 将检索范围限定在已有股票池和关系图内。
- 区分事实证据、社区讨论和市场传闻。
- 支持移动端离线检索，而不在移动端内嵌 Python/FastAPI 后端。
- 第一版移动端避免双向同步，便于调试和复现。

第一版不做：

- 面向任意文档的通用股票问答。
- 从新闻正文自动发现供应链关系。
- 桌面端和移动端之间的双向同步。
- 移动端写入 RAG 索引后的冲突解决。
- 把社区帖子当作事实证据使用。

## 用户流程

桌面端或后端流程：

1. 抓取新闻、公告和社区帖子。
2. 将每条内容规范化为 `document`。
3. 将每个文档切分为 `chunks`。
4. 使用约定的向量模型生成 chunk 向量。
5. 构建带版本号的 `rag_pack.sqlite`。
6. 发布该数据包，供移动端下载或本地导入。

移动端流程：

1. 下载或导入 `rag_pack.sqlite`。
2. 校验数据包 manifest。
3. 原子替换本地只读数据包。
4. 使用同一模型为用户查询生成向量。
5. 运行带过滤条件的 sqlite-vec 最近邻检索。
6. 返回带标题、URL、来源层级和待核查项的 chunks。

## 当前接口用法

产品接口有意保持显式：调用方可以传入规范化文档，也可以从现有消息缓存构建数据包，然后查询本地只读数据包。查询时以只读方式打开 `rag_pack.sqlite`，不会调用云服务。

构建产品数据包前，先下载本地向量模型资源：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-rag-embedding-model.ps1
```

默认资源目录为 `models/bge-small-zh-v1.5-int8`。该目录被 git 忽略；如果目录存在，会被打包进桌面端随行进程。

查看本地数据包状态：

```http
GET /api/rag-pack/status
```

构建本地数据包：

```http
POST /api/rag-pack/build
Content-Type: application/json
```

```json
{
  "pack_version": "2026-06-05-local",
  "target_chars": 500,
  "overlap_chars": 80,
  "documents": [
    {
      "source": "交易所公告",
      "source_tier": "filing",
      "title": "宁德时代订单公告",
      "text": "公告正文或清洗后的正文片段...",
      "url": "https://example.test/filing",
      "published_at": "2026-06-01",
      "stock_codes": ["300750.SZ"],
      "relation_types": ["supply_chain"],
      "sentiment": "positive"
    }
  ]
}
```

默认输出路径由 `GP_RAG_PACK_PATH` 控制，默认值为 `data/cache/rag_pack.sqlite`。

从现有消息缓存构建数据包：

```http
POST /api/rag-pack/build-from-news-cache
Content-Type: application/json
```

```json
{
  "pack_version": "2026-06-05-news-cache",
  "days": 30,
  "stock_codes": ["300750.SZ"],
  "relation_types": ["supply_chain"],
  "source_tiers": ["filing", "news", "community"],
  "limit": 1000
}
```

查询本地数据包：

```http
POST /api/rag-pack/query
Content-Type: application/json
```

```json
{
  "query": "宁德时代上游订单有什么利好证据",
  "stock_codes": ["300750.SZ"],
  "relation_types": ["supply_chain"],
  "source_tiers": ["filing", "news"],
  "published_after": "2026-01-01",
  "top_k": 8
}
```

响应命中项是 chunk 级证据：

```json
{
  "hits": [
    {
      "chunk_id": "chunk_xxx",
      "document_id": "doc_xxx",
      "score": 0.42,
      "title": "宁德时代订单公告",
      "text": "召回的 chunk 文本...",
      "source": "交易所公告",
      "source_tier": "filing",
      "published_at": "2026-06-01",
      "url": "https://example.test/filing",
      "stock_codes": ["300750.SZ"],
      "relation_types": ["supply_chain"],
      "sentiment": "positive"
    }
  ],
  "manifest": {},
  "notes": []
}
```

也可以通过 `app.services.rag_pack` 直接在 Python 中调用：

```python
from pathlib import Path

from app.schemas import RagPackBuildRequest, RagPackDocument, RagPackQueryRequest
from app.services.rag_pack import build_rag_pack, query_rag_pack

build_rag_pack(
    RagPackBuildRequest(
        pack_version="local-dev",
        documents=[
            RagPackDocument(
                source="财经新闻",
                source_tier="news",
                title="宁德时代供应链跟踪",
                text="清洗后的正文...",
                stock_codes=["300750.SZ"],
                relation_types=["supply_chain"],
            )
        ],
    ),
    path=Path("data/cache/rag_pack.sqlite"),
)

result = query_rag_pack(
    RagPackQueryRequest(query="宁德时代供应链订单", stock_codes=["300750.SZ"])
)
```

## 推荐技术栈

- 存储：SQLite。
- 向量索引：目标移动端或运行时实现中使用 sqlite-vec。
- 向量模型：兼容 INT8 的 `bge-small-zh-v1.5` ONNX 资源。
- 桌面端运行时：ONNX Runtime + `tokenizers`。
- 桌面端向量生成：生成 document 和 chunk 向量。
- 移动端向量生成：生成查询向量，后续可选择支持少量本地增量。

桌面端和移动端必须使用相同的模型、维度、量化方式和归一化规则。应把这些设置视为检索协议的一部分，而不是普通实现细节。

产品向量提供器：

- `app.services.rag_pack.OnnxEmbeddingProvider`
- 默认模型 ID：`BAAI/bge-small-zh-v1.5`
- 默认后端：`onnxruntime`
- 默认量化标签：`int8`
- 默认维度：`512`

测试专用向量提供器：

- `app.services.rag_pack.HashingEmbeddingProvider`
- 仅在测试显式注入，或同时设置 `GP_RAG_EMBEDDING_BACKEND=hashing` 与 `GP_RAG_ALLOW_HASH_EMBEDDING=true` 时启用。
- 产品 API 路径默认永不启用。

当 manifest 元数据和查询模型元数据不匹配时，数据包校验必须失败。

## 来源层级

每个 document 和 chunk 都有 `source_tier`。

| 层级 | 含义 | 是否可提升置信度 |
| --- | --- | --- |
| `filing` | 官方公告、交易所披露、上市公司公告 | 是 |
| `news` | 新闻或事实性市场报道 | 是 |
| `community` | 论坛、社交帖子、讨论、传闻、情绪 | 否 |

社区内容可以帮助发现风险信号、情绪变化和待验证传闻。界面中必须标记为 `community / pending verification`，并增加二次核查项。

## 检索单元

只使用 `chunks` 作为向量检索单元。

第一版不要对整篇文档做向量检索。整篇文档只是来源容器，chunk 才是证据单元。

建议的第一版切分规则：

- 目标长度：300 到 600 个中文字符。
- 重叠长度：50 到 100 个中文字符。
- 每个 chunk 保留标题、来源、股票代码、关系类型、发布时间和 URL。
- 使用 `chunk_version` 管理切分器版本，初始值为 `v1`。

## 检索增强生成数据包

移动端数据包是带版本号的只读 SQLite 快照，不是同步数据库。

建议生命周期：

```text
desktop build -> rag_pack.sqlite.tmp -> validate -> publish
mobile download -> validate -> replace rag_pack.sqlite atomically
```

移动端应用可以使用独立本地数据库保存用户设置、自选、最近查询和界面状态。第一版不应写入主 RAG 数据包。

## 数据包清单

每个数据包必须包含且只包含一条激活的 manifest 记录。

建议字段：

```sql
CREATE TABLE rag_manifest (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  schema_version TEXT NOT NULL,
  pack_version TEXT NOT NULL,
  created_at TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  document_count INTEGER NOT NULL,
  chunk_count INTEGER NOT NULL,
  embedding_model TEXT NOT NULL,
  embedding_backend TEXT NOT NULL,
  embedding_quantization TEXT NOT NULL,
  embedding_dim INTEGER NOT NULL,
  embedding_normalized INTEGER NOT NULL,
  chunk_version TEXT NOT NULL,
  sqlite_vec_version TEXT
);
```

校验规则：

- 拒绝未知的 `schema_version`。
- 拒绝未知的 `embedding_model`。
- 拒绝不匹配的 `embedding_backend`。
- 拒绝不匹配的 `embedding_quantization`。
- 拒绝不匹配的 `embedding_dim`。
- 拒绝不匹配的归一化规则。
- 拒绝损坏或非预期的 `content_hash`。
- 拒绝没有 chunk 的数据包。

## 建议数据结构

文档表：

```sql
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL,
  source_tier TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT,
  url TEXT,
  published_at TEXT,
  fetched_at TEXT NOT NULL,
  sentiment TEXT NOT NULL DEFAULT 'uncertain',
  raw_hash TEXT NOT NULL
);
```

Chunk 表：

```sql
CREATE TABLE chunks (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES documents(id),
  chunk_index INTEGER NOT NULL,
  title TEXT NOT NULL,
  text TEXT NOT NULL,
  source_tier TEXT NOT NULL,
  published_at TEXT,
  chunk_version TEXT NOT NULL,
  token_count INTEGER,
  char_count INTEGER NOT NULL
);
```

实体和关系表：

```sql
CREATE TABLE chunk_entities (
  chunk_id TEXT NOT NULL REFERENCES chunks(id),
  entity_type TEXT NOT NULL,
  entity_value TEXT NOT NULL,
  PRIMARY KEY (chunk_id, entity_type, entity_value)
);

CREATE TABLE chunk_relations (
  chunk_id TEXT NOT NULL REFERENCES chunks(id),
  relation_type TEXT NOT NULL,
  PRIMARY KEY (chunk_id, relation_type)
);
```

向量表形态取决于 sqlite-vec 绑定细节，但每条向量记录必须与 `chunks.id` 一一对应。

概念结构如下：

```sql
CREATE VIRTUAL TABLE chunk_embeddings USING vec0(
  chunk_id TEXT PRIMARY KEY,
  embedding FLOAT[512]
);
```

如果所选 `bge-small-zh` 资源使用其他维度，schema 和 manifest 必须使用该实际维度。

当前桌面端产品路径把向量存储在 `chunk_embeddings(chunk_id, embedding BLOB)` 中，内容是 JSON 编码的浮点向量，并在本地用余弦相似度为候选项打分。这样可以简化 GitHub beta 包装，并在构建数据包后完全离线运行。后续集成 sqlite-vec 时，应保留 chunk、document、entity schema，只替换向量存储和查询实现。

## 查询计划

针对产业链问题：

1. 解析股票代码、股票名称、行业、时间窗口和意图。
2. 使用现有股票池和关系图解析检索范围。
3. 构建元数据过滤条件：
   - `stock_codes`
   - `relation_types`
   - `published_at >= cutoff`
   - 允许的 `source_tier`
4. 使用 `bge-small-zh` INT8 为查询生成向量。
5. 使用 sqlite-vec 检索 top-K chunks。
6. 应用后置过滤，并按文档去重。
7. 证据相近时优先使用事实性来源层级，而不是社区内容。
8. 返回包含来源层级、标题、发布时间、URL 和股票代码的证据。

当前桌面端查询行为：

1. 以只读方式打开 `rag_pack.sqlite`。
2. 校验 manifest 与查询向量器是否兼容。
3. 使用 SQLite 元数据过滤股票、关系类型、来源层级和日期。
4. 在 Python 中用余弦相似度为候选 chunks 打分。
5. 应用较小的来源层级加分或惩罚。
6. 返回 chunk 命中项。

这是 GitHub beta 的桌面端检索路径。后续可以用 sqlite-vec 最近邻检索替换候选打分，同时保持兼容。

建议的第一版排序策略：

```text
score = vector_score
      + freshness_boost
      + relation_match_boost
      + source_tier_boost
      - community_confidence_penalty
```

不要隐藏社区 chunks。应降低其排序，并给出清晰标签。

## 答案生成

检索层必须在没有云端大模型时仍然可用。

第一版答案生成可以使用模板：

- 方向：利好、利空、中性、不确定。
- 置信度：高、中、低。
- 影响链路：上游或下游关系路径。
- 证据列表：top chunks。
- 待核查项：公告、财报、价量、订单数据、库存、毛利率。

如果可用大模型，应把检索到的 chunks 作为上下文，并要求引用证据。大模型不得编造已有关系图之外的关系。

## 调试策略

保持数据包可复现。

必要调试材料：

- `rag_pack.sqlite`
- manifest 记录
- 查询文本
- 查询向量模型元数据
- 解析出的股票和关系范围
- SQL 过滤条件
- top-K 原始向量结果
- 最终重排结果

这些材料可以让桌面端用同一个 SQLite 文件复现移动端检索问题。

## 移动端边界

移动端应该做：

- 校验并打开只读数据包。
- 生成查询向量。
- 执行 sqlite-vec 检索。
- 渲染证据和待核查项。
- 在独立本地数据库中保存用户设置。

第一版移动端不应该做：

- 重建完整文档向量索引。
- 合并桌面端和移动端的索引写入。
- 解决同步冲突。
- 把社区帖子当成已验证事实。

## 实施阶段

第一阶段：桌面端数据包构建器

- 构建 `documents`、`chunks`、实体表和 manifest。
- 使用确定性切分。
- 使用 `bge-small-zh` INT8 生成向量。
- 构建完成后校验数据包。

当前状态：已具备 schema、确定性切分、manifest、原子替换数据包、只读查询、元数据过滤、ONNX Runtime 向量提供器、消息缓存数据包构建器、界面入口和测试。

第二阶段：桌面端查询对齐

- 增加读取数据包并返回 chunks 的桌面端查询路径。
- 使用固定文档和预期 top-K 行为添加测试。
- 尽量保持当前 `/api/news-rag` 响应形态兼容。

第三阶段：移动端只读检索

- 打包 sqlite-vec。
- 加载 `rag_pack.sqlite`。
- 校验 manifest。
- 生成查询向量。
- 返回排序后的 chunks 和模板化结论。

第四阶段：数据包分发

- 增加 `/api/rag-pack/latest`。
- 使用内容哈希和原子替换。
- 校验失败时回滚到上一个数据包。

第五阶段：可选本地增量

- 允许移动端只在独立本地覆盖数据库中保存少量文档。
- 覆盖库向量与主数据包分开保存。
- 查询时合并结果，不直接修改主数据包。

## 待决问题

- 移动端使用哪种 `bge-small-zh` 运行时。
- Android 和 iOS 上 sqlite-vec 的打包方式。
- 公告是否应在向量检索前后独立成 `filing` 来源适配器。
- 重排继续使用规则，还是后续增加小型本地重排模型。
