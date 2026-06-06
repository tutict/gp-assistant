# RAG Design

This document describes the product design and engineering surface for evidence-backed industry-chain RAG in GP Assistant. The current implementation supports scoped news retrieval, SQLite caching, source-tier labels, rule-based findings, and a reusable offline RAG pack builder/query path. Product builds use a local `bge-small-zh-v1.5` ONNX/INT8 embedding runtime through ONNX Runtime. The deterministic hashing embedder is retained only as an explicit test fixture and is not used by default.

## Goals

- Answer A-share industry-chain questions with traceable evidence.
- Keep retrieval scoped to the existing stock and relation graph.
- Separate factual evidence from community discussion and rumors.
- Support mobile offline retrieval without embedding the Python/FastAPI backend.
- Keep the first mobile version debuggable by avoiding bidirectional sync.

Non-goals for the first version:

- General-purpose stock Q&A across arbitrary documents.
- Automatic discovery of supply-chain relations from news text.
- Bidirectional sync between desktop and mobile.
- Conflict resolution for mobile-written RAG index rows.
- Using community posts as factual evidence.

## User Flow

Desktop or backend flow:

1. Fetch news, filings, and community posts.
2. Normalize each item into a `document`.
3. Split each document into `chunks`.
4. Generate chunk embeddings with the agreed embedding model.
5. Build a versioned `rag_pack.sqlite`.
6. Publish the pack for mobile download or local import.

Mobile flow:

1. Download or import `rag_pack.sqlite`.
2. Validate the pack manifest.
3. Atomically replace the local read-only pack.
4. Embed the user query with the same model.
5. Run filtered sqlite-vec nearest-neighbor search.
6. Return chunks with titles, URLs, source tier, and pending checks.

## Current API Usage

The product surface is intentionally explicit: callers can either pass normalized documents or build from the existing message cache, then query a local read-only pack. Queries open `rag_pack.sqlite` in read-only mode and do not call cloud services.

Before building a product pack, download the local embedding assets:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\download-rag-embedding-model.ps1
```

By default the assets are stored under `models/bge-small-zh-v1.5-int8`. The directory is ignored by git and is bundled into the desktop sidecar when present.

Check local pack status:

```http
GET /api/rag-pack/status
```

Build a local pack:

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

The default output path is controlled by `GP_RAG_PACK_PATH`, defaulting to `data/cache/rag_pack.sqlite`.

Build a pack from the existing message cache:

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

Query the local pack:

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

Response hits are chunk-level evidence:

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

Direct Python usage is available through `app.services.rag_pack`:

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

## Recommended Technical Stack

- Storage: SQLite.
- Vector index: sqlite-vec in the target mobile/runtime implementation.
- Embedding model: `bge-small-zh-v1.5` INT8-compatible ONNX assets.
- Desktop runtime: ONNX Runtime + `tokenizers`.
- Desktop embedding: document and chunk embeddings.
- Mobile embedding: query embeddings, plus optional small local increments later.

The same model, dimension, quantization, and normalization rules must be used on desktop and mobile. Treat those settings as part of the retrieval protocol, not as an implementation detail.

Product embedding provider:

- `app.services.rag_pack.OnnxEmbeddingProvider`
- default model id: `BAAI/bge-small-zh-v1.5`
- default backend: `onnxruntime`
- default quantization label: `int8`
- default dimension: `512`

Test-only embedding provider:

- `app.services.rag_pack.HashingEmbeddingProvider`
- enabled only when explicitly injected in tests or when `GP_RAG_EMBEDDING_BACKEND=hashing` and `GP_RAG_ALLOW_HASH_EMBEDDING=true`
- never enabled by default in product API paths

Pack validation fails when manifest metadata and query model metadata do not match.

## Source Tiers

Every document and chunk has a `source_tier`.

| Tier | Meaning | Can raise confidence? |
| --- | --- | --- |
| `filing` | Official filings, exchange disclosures, announcements | Yes |
| `news` | News or factual market reports | Yes |
| `community` | Forums, social posts, discussion, rumors, sentiment | No |

Community content can help surface risk signals, sentiment changes, and rumors to verify. It must be shown as `community / pending verification` in the UI and must add a secondary verification check.

## Retrieval Unit

Use `chunks` as the only vector retrieval unit.

Do not vector-search whole documents in the first version. Whole documents are source containers; chunks are the evidence units.

Recommended first chunking rule:

- Target size: 300-600 Chinese characters.
- Overlap: 50-100 Chinese characters.
- Preserve title, source, stock codes, relation types, publish time, and URL on every chunk.
- Version the chunker with `chunk_version`, starting with `v1`.

## RAG Pack

The mobile pack is a versioned, read-only SQLite snapshot. It is not a sync database.

Recommended lifecycle:

```text
desktop build -> rag_pack.sqlite.tmp -> validate -> publish
mobile download -> validate -> replace rag_pack.sqlite atomically
```

The mobile app may keep a separate local database for user settings, favorites, recent queries, and UI state. It should not write into the main RAG pack in the first version.

## Manifest

Each pack must include exactly one active manifest row.

Suggested fields:

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

Validation rules:

- Reject unknown `schema_version`.
- Reject unknown `embedding_model`.
- Reject mismatched `embedding_backend`.
- Reject mismatched `embedding_quantization`.
- Reject mismatched `embedding_dim`.
- Reject mismatched normalization.
- Reject corrupted or unexpected `content_hash`.
- Reject packs with zero chunks.

## Suggested Schema

Documents:

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

Chunks:

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

Entities and relations:

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

Vector table shape depends on sqlite-vec binding details, but each vector row must map one-to-one to `chunks.id`.

Conceptually:

```sql
CREATE VIRTUAL TABLE chunk_embeddings USING vec0(
  chunk_id TEXT PRIMARY KEY,
  embedding FLOAT[512]
);
```

If the selected `bge-small-zh` artifact uses a different dimension, the schema and manifest must use that exact dimension.

The current desktop product path stores embeddings in `chunk_embeddings(chunk_id, embedding BLOB)` as JSON-encoded float vectors and scores candidates with local cosine similarity. This keeps GitHub beta packaging simple and fully offline after pack construction. When sqlite-vec is integrated, keep the chunk/document/entity schema and replace only the vector storage/query implementation.

## Query Plan

For an industry-chain question:

1. Parse stock code, stock name, industry, time window, and intent.
2. Resolve scope using the existing stock universe and relation graph.
3. Build metadata filters:
   - `stock_codes`
   - `relation_types`
   - `published_at >= cutoff`
   - allowed `source_tier`
4. Embed the query using `bge-small-zh` INT8.
5. Retrieve top-K chunks with sqlite-vec.
6. Apply post-filters and dedupe by document.
7. Prefer factual tiers over community when evidence is otherwise similar.
8. Return evidence with source tier, title, publish time, URL, and stock codes.

Current desktop query behavior:

1. Open `rag_pack.sqlite` read-only.
2. Validate manifest compatibility with the query embedder.
3. Apply SQLite metadata filters for stock, relation type, source tier, and date.
4. Score candidate chunks with cosine similarity in Python.
5. Apply a small source-tier boost/penalty.
6. Return chunk hits.

This is the GitHub beta desktop retrieval path. It is compatible with replacing candidate scoring with sqlite-vec nearest-neighbor search later.

Recommended first ranking policy:

```text
score = vector_score
      + freshness_boost
      + relation_match_boost
      + source_tier_boost
      - community_confidence_penalty
```

Do not hide community chunks. Rank them lower and label them clearly.

## Answer Generation

The retrieval layer must be useful without a cloud LLM.

First version answer generation can be template-based:

- Direction: positive, negative, neutral, uncertain.
- Confidence: high, medium, low.
- Impact chain: upstream/downstream relation path.
- Evidence list: top chunks.
- Pending checks: filings, financial reports, price/volume, order data, inventory, margin.

If an LLM is available, use retrieved chunks as context and require citations. The LLM must not invent relations outside the existing graph.

## Debugging Strategy

Keep the pack reproducible.

Required debug artifacts:

- `rag_pack.sqlite`
- manifest row
- query text
- query embedding model metadata
- resolved stock/relation scope
- SQL filters
- top-K raw vector results
- final reranked results

This allows desktop reproduction of mobile retrieval bugs with the exact same SQLite file.

## Mobile Boundaries

Mobile should do:

- Validate and open a read-only pack.
- Generate query embeddings.
- Run sqlite-vec retrieval.
- Render evidence and pending checks.
- Store user settings in a separate local database.

Mobile should not do in the first version:

- Rebuild the full document embedding index.
- Merge desktop and mobile index writes.
- Resolve sync conflicts.
- Treat community posts as verified facts.

## Implementation Phases

Phase 1: Desktop pack builder

- Build `documents`, `chunks`, entity tables, and manifest.
- Use deterministic chunking.
- Generate embeddings with `bge-small-zh` INT8.
- Validate pack after build.

Current status: schema, deterministic chunking, manifest, atomic pack replacement, read-only query, metadata filtering, ONNX Runtime embedding provider, message-cache pack builder, UI entry points, and tests exist.

Phase 2: Desktop query parity

- Add a desktop query path that reads the pack and returns chunks.
- Add tests with fixed documents and expected top-K behavior.
- Keep current `/api/news-rag` response shape compatible where possible.

Phase 3: Mobile read-only retrieval

- Bundle sqlite-vec.
- Load `rag_pack.sqlite`.
- Validate manifest.
- Generate query embeddings.
- Return ranked chunks and template findings.

Phase 4: Pack distribution

- Add `/api/rag-pack/latest`.
- Use content hash and atomic replacement.
- Add rollback to previous pack if validation fails.

Phase 5: Optional local increments

- Allow small mobile-only documents in a separate local overlay database.
- Keep overlay embeddings separate from the main pack.
- Merge results at query time, not by modifying the pack.

## Open Decisions

- Exact `bge-small-zh` runtime on mobile.
- sqlite-vec packaging for Android and iOS.
- Whether filings become a separate `filing` source adapter before or after vector retrieval.
- Whether reranking remains rule-based or adds a small local reranker later.
