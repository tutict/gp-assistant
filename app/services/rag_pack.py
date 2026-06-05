from __future__ import annotations

import hashlib
import json
import math
import os
import re
import sqlite3
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Iterable, Sequence

from app.schemas import (
    RagPackBuildRequest,
    RagPackBuildResult,
    RagPackChunkHit,
    RagPackDocument,
    RagPackQueryRequest,
    RagPackQueryResult,
)


DEFAULT_PACK_PATH = "data/cache/rag_pack.sqlite"
SCHEMA_VERSION = "rag-pack-v1"
CHUNK_VERSION = "chunk-v1"
DEFAULT_EMBEDDING_MODEL = os.getenv("GP_RAG_EMBEDDING_MODEL", "bge-small-zh-int8-protocol")
DEFAULT_EMBEDDING_DIM = int(os.getenv("GP_RAG_EMBEDDING_DIM", "384"))


@dataclass(frozen=True)
class ChunkRecord:
    id: str
    document_id: str
    chunk_index: int
    title: str
    text: str
    source: str
    source_tier: str
    published_at: str | None
    url: str | None
    sentiment: str
    stock_codes: tuple[str, ...]
    relation_types: tuple[str, ...]


class EmbeddingProvider:
    model_id = DEFAULT_EMBEDDING_MODEL
    quantization = "int8"
    dim = DEFAULT_EMBEDDING_DIM
    normalized = True

    def embed(self, text: str) -> list[float]:
        raise NotImplementedError


class HashingEmbeddingProvider(EmbeddingProvider):
    """Deterministic local placeholder until the bge-small-zh INT8 runtime is wired in."""

    def embed(self, text: str) -> list[float]:
        vector = [0.0] * self.dim
        tokens = _tokenize(text)
        if not tokens:
            return vector
        for token in tokens:
            digest = hashlib.blake2b(token.encode("utf-8"), digest_size=8).digest()
            bucket = int.from_bytes(digest[:4], "little") % self.dim
            sign = 1.0 if digest[4] & 1 else -1.0
            vector[bucket] += sign
        return _normalize(vector)


def build_rag_pack(
    request: RagPackBuildRequest,
    path: Path | None = None,
    embedding: EmbeddingProvider | None = None,
) -> RagPackBuildResult:
    pack_path = path or _pack_path()
    embedder = embedding or HashingEmbeddingProvider()
    documents = _dedupe_documents(request.documents)
    chunks = [
        chunk
        for document in documents
        for chunk in chunk_document(document, request.target_chars, request.overlap_chars)
    ]
    content_hash = _content_hash(documents, chunks, embedder)

    pack_path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = pack_path.with_suffix(pack_path.suffix + ".tmp")
    if tmp_path.exists():
        tmp_path.unlink()

    conn = sqlite3.connect(tmp_path)
    try:
        conn.row_factory = sqlite3.Row
        _init_pack_db(conn)
        _store_manifest(conn, request, documents, chunks, content_hash, embedder)
        _store_documents(conn, documents)
        _store_chunks(conn, chunks, embedder)
        _validate_pack(conn)
        conn.commit()
    except Exception:
        conn.close()
        if tmp_path.exists():
            tmp_path.unlink()
        raise
    conn.close()
    tmp_path.replace(pack_path)

    return RagPackBuildResult(
        path=str(pack_path),
        document_count=len(documents),
        chunk_count=len(chunks),
        content_hash=content_hash,
        embedding_model=embedder.model_id,
        embedding_dim=embedder.dim,
        notes=[
            "已构建版本化只读 RAG pack；当前 embedding provider 为 deterministic hashing 测试替身。",
            "接入 bge-small-zh INT8 时必须保持 manifest 的 model/dim/normalize 与手机端一致。",
        ],
    )


def query_rag_pack(
    request: RagPackQueryRequest,
    path: Path | None = None,
    embedding: EmbeddingProvider | None = None,
) -> RagPackQueryResult:
    pack_path = path or _pack_path()
    embedder = embedding or HashingEmbeddingProvider()
    if not pack_path.exists():
        return RagPackQueryResult(hits=[], notes=[f"RAG pack 不存在：{pack_path}"])

    conn = sqlite3.connect(f"file:{pack_path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        manifest = _load_manifest(conn)
        _validate_embedder_compatibility(manifest, embedder)
        query_vector = embedder.embed(request.query)
        candidates = _candidate_chunks(conn, request)
        scored = [
            (_cosine_similarity(query_vector, _decode_vector(row["embedding"])) + _tier_boost(row["source_tier"]), row)
            for row in candidates
        ]
        scored.sort(key=lambda item: item[0], reverse=True)
        hits = [_row_to_hit(conn, row, score) for score, row in scored[: request.top_k]]
        return RagPackQueryResult(
            hits=hits,
            manifest=manifest,
            notes=["当前查询使用 SQLite 元数据过滤 + 本地向量相似度；sqlite-vec 接入后替换候选召回实现。"],
        )
    finally:
        conn.close()


def chunk_document(document: RagPackDocument, target_chars: int = 500, overlap_chars: int = 80) -> list[ChunkRecord]:
    text = _normalize_text(document.text)
    if not text:
        return []
    target = max(target_chars, 1)
    overlap = min(max(overlap_chars, 0), max(target - 1, 0))
    step = max(target - overlap, 1)
    document_id = _document_id(document)
    chunks: list[ChunkRecord] = []
    for index, start in enumerate(range(0, len(text), step)):
        chunk_text = text[start : start + target]
        if not chunk_text:
            continue
        chunk_id = _stable_id("chunk", document_id, str(index), chunk_text)
        chunks.append(
            ChunkRecord(
                id=chunk_id,
                document_id=document_id,
                chunk_index=index,
                title=document.title,
                text=chunk_text,
                source=document.source,
                source_tier=document.source_tier,
                published_at=document.published_at,
                url=document.url,
                sentiment=document.sentiment,
                stock_codes=tuple(_normalize_code(code) for code in document.stock_codes if code),
                relation_types=tuple(sorted({item for item in document.relation_types if item})),
            )
        )
        if start + target >= len(text):
            break
    return chunks


def _init_pack_db(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        PRAGMA foreign_keys = ON;

        CREATE TABLE rag_manifest (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          schema_version TEXT NOT NULL,
          pack_version TEXT NOT NULL,
          created_at TEXT NOT NULL,
          content_hash TEXT NOT NULL,
          document_count INTEGER NOT NULL,
          chunk_count INTEGER NOT NULL,
          embedding_model TEXT NOT NULL,
          embedding_quantization TEXT NOT NULL,
          embedding_dim INTEGER NOT NULL,
          embedding_normalized INTEGER NOT NULL,
          chunk_version TEXT NOT NULL,
          sqlite_vec_version TEXT
        );

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

        CREATE TABLE chunks (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL REFERENCES documents(id),
          chunk_index INTEGER NOT NULL,
          title TEXT NOT NULL,
          text TEXT NOT NULL,
          source TEXT NOT NULL,
          source_tier TEXT NOT NULL,
          published_at TEXT,
          url TEXT,
          sentiment TEXT NOT NULL DEFAULT 'uncertain',
          chunk_version TEXT NOT NULL,
          token_count INTEGER,
          char_count INTEGER NOT NULL
        );

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

        CREATE TABLE chunk_embeddings (
          chunk_id TEXT PRIMARY KEY REFERENCES chunks(id),
          embedding BLOB NOT NULL
        );

        CREATE INDEX idx_chunks_source_tier ON chunks(source_tier);
        CREATE INDEX idx_chunks_published_at ON chunks(published_at);
        CREATE INDEX idx_chunk_entities_lookup ON chunk_entities(entity_type, entity_value);
        CREATE INDEX idx_chunk_relations_lookup ON chunk_relations(relation_type);
        """
    )


def _pack_path() -> Path:
    return Path(os.getenv("GP_RAG_PACK_PATH", DEFAULT_PACK_PATH))


def _store_manifest(
    conn: sqlite3.Connection,
    request: RagPackBuildRequest,
    documents: Sequence[RagPackDocument],
    chunks: Sequence[ChunkRecord],
    content_hash: str,
    embedder: EmbeddingProvider,
) -> None:
    conn.execute(
        """
        INSERT INTO rag_manifest (
          id, schema_version, pack_version, created_at, content_hash,
          document_count, chunk_count, embedding_model, embedding_quantization,
          embedding_dim, embedding_normalized, chunk_version, sqlite_vec_version
        )
        VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            SCHEMA_VERSION,
            request.pack_version,
            datetime.now().isoformat(timespec="seconds"),
            content_hash,
            len(documents),
            len(chunks),
            embedder.model_id,
            embedder.quantization,
            embedder.dim,
            1 if embedder.normalized else 0,
            CHUNK_VERSION,
            None,
        ),
    )


def _store_documents(conn: sqlite3.Connection, documents: Sequence[RagPackDocument]) -> None:
    fetched_at = datetime.now().isoformat(timespec="seconds")
    for document in documents:
        conn.execute(
            """
            INSERT INTO documents (
              id, source, source_tier, title, summary, url, published_at,
              fetched_at, sentiment, raw_hash
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                _document_id(document),
                document.source,
                document.source_tier,
                document.title,
                document.summary,
                document.url,
                document.published_at,
                document.fetched_at or fetched_at,
                document.sentiment,
                _raw_hash(document),
            ),
        )


def _store_chunks(
    conn: sqlite3.Connection,
    chunks: Sequence[ChunkRecord],
    embedder: EmbeddingProvider,
) -> None:
    for chunk in chunks:
        conn.execute(
            """
            INSERT INTO chunks (
              id, document_id, chunk_index, title, text, source, source_tier,
              published_at, url, sentiment, chunk_version, token_count, char_count
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                chunk.id,
                chunk.document_id,
                chunk.chunk_index,
                chunk.title,
                chunk.text,
                chunk.source,
                chunk.source_tier,
                chunk.published_at,
                chunk.url,
                chunk.sentiment,
                CHUNK_VERSION,
                len(_tokenize(chunk.text)),
                len(chunk.text),
            ),
        )
        for code in chunk.stock_codes:
            _insert_chunk_entity(conn, chunk.id, "stock", code)
        for relation_type in chunk.relation_types:
            conn.execute(
                "INSERT OR IGNORE INTO chunk_relations (chunk_id, relation_type) VALUES (?, ?)",
                (chunk.id, relation_type),
            )
        conn.execute(
            "INSERT INTO chunk_embeddings (chunk_id, embedding) VALUES (?, ?)",
            (chunk.id, _encode_vector(embedder.embed(chunk.text))),
        )


def _insert_chunk_entity(conn: sqlite3.Connection, chunk_id: str, entity_type: str, entity_value: str) -> None:
    conn.execute(
        "INSERT OR IGNORE INTO chunk_entities (chunk_id, entity_type, entity_value) VALUES (?, ?, ?)",
        (chunk_id, entity_type, entity_value),
    )


def _candidate_chunks(conn: sqlite3.Connection, request: RagPackQueryRequest) -> list[sqlite3.Row]:
    clauses = []
    params: list[str] = []
    joins = ["JOIN chunk_embeddings embedding ON embedding.chunk_id = chunk.id"]

    stock_codes = [_normalize_code(code) for code in request.stock_codes if code]
    if stock_codes:
        joins.append("JOIN chunk_entities stock_entity ON stock_entity.chunk_id = chunk.id")
        clauses.append(
            f"stock_entity.entity_type = 'stock' AND stock_entity.entity_value IN ({_placeholders(stock_codes)})"
        )
        params.extend(stock_codes)

    relation_types = [item for item in request.relation_types if item]
    if relation_types:
        joins.append("JOIN chunk_relations relation ON relation.chunk_id = chunk.id")
        clauses.append(f"relation.relation_type IN ({_placeholders(relation_types)})")
        params.extend(relation_types)

    source_tiers = [item for item in request.source_tiers if item]
    if source_tiers:
        clauses.append(f"chunk.source_tier IN ({_placeholders(source_tiers)})")
        params.extend(source_tiers)

    if request.published_after:
        clauses.append("COALESCE(chunk.published_at, '') >= ?")
        params.append(request.published_after)

    where = f"WHERE {' AND '.join(clauses)}" if clauses else ""
    rows = conn.execute(
        f"""
        SELECT DISTINCT
          chunk.*,
          document.summary,
          embedding.embedding
        FROM chunks chunk
        JOIN documents document ON document.id = chunk.document_id
        {' '.join(joins)}
        {where}
        LIMIT 500
        """,
        params,
    ).fetchall()
    return list(rows)


def _row_to_hit(conn: sqlite3.Connection, row: sqlite3.Row, score: float) -> RagPackChunkHit:
    entities = conn.execute(
        "SELECT entity_type, entity_value FROM chunk_entities WHERE chunk_id = ?",
        (row["id"],),
    ).fetchall()
    relations = conn.execute(
        "SELECT relation_type FROM chunk_relations WHERE chunk_id = ?",
        (row["id"],),
    ).fetchall()
    return RagPackChunkHit(
        chunk_id=row["id"],
        document_id=row["document_id"],
        score=score,
        title=row["title"],
        text=row["text"],
        source=row["source"],
        source_tier=row["source_tier"],
        published_at=row["published_at"],
        url=row["url"],
        stock_codes=sorted({item["entity_value"] for item in entities if item["entity_type"] == "stock"}),
        relation_types=sorted({item["relation_type"] for item in relations}),
        sentiment=row["sentiment"],
    )


def _validate_pack(conn: sqlite3.Connection) -> None:
    manifest = _load_manifest(conn)
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise ValueError("RAG pack schema_version 不匹配")
    if int(manifest.get("chunk_count") or 0) <= 0:
        raise ValueError("RAG pack 没有可检索 chunk")
    orphan_count = conn.execute(
        """
        SELECT COUNT(*) AS count
        FROM chunks chunk
        LEFT JOIN chunk_embeddings embedding ON embedding.chunk_id = chunk.id
        WHERE embedding.chunk_id IS NULL
        """
    ).fetchone()["count"]
    if orphan_count:
        raise ValueError("RAG pack 存在缺少 embedding 的 chunk")


def _load_manifest(conn: sqlite3.Connection) -> dict:
    row = conn.execute("SELECT * FROM rag_manifest WHERE id = 1").fetchone()
    if row is None:
        raise ValueError("RAG pack 缺少 manifest")
    return dict(row)


def _validate_embedder_compatibility(manifest: dict, embedder: EmbeddingProvider) -> None:
    if manifest.get("embedding_model") != embedder.model_id:
        raise ValueError("RAG pack embedding_model 与查询模型不匹配")
    if int(manifest.get("embedding_dim") or 0) != embedder.dim:
        raise ValueError("RAG pack embedding_dim 与查询模型不匹配")
    if bool(manifest.get("embedding_normalized")) != embedder.normalized:
        raise ValueError("RAG pack embedding_normalized 与查询模型不匹配")


def _dedupe_documents(documents: Sequence[RagPackDocument]) -> list[RagPackDocument]:
    seen: set[str] = set()
    result: list[RagPackDocument] = []
    for document in documents:
        key = _document_id(document)
        if key in seen:
            continue
        seen.add(key)
        result.append(document)
    return result


def _document_id(document: RagPackDocument) -> str:
    return _stable_id(
        "doc",
        document.source,
        document.title,
        document.url or "",
        document.published_at or "",
        _normalize_text(document.text),
    )


def _raw_hash(document: RagPackDocument) -> str:
    return hashlib.sha256(_normalize_text(document.text).encode("utf-8")).hexdigest()


def _content_hash(
    documents: Sequence[RagPackDocument],
    chunks: Sequence[ChunkRecord],
    embedder: EmbeddingProvider,
) -> str:
    payload = {
        "schema_version": SCHEMA_VERSION,
        "embedding_model": embedder.model_id,
        "embedding_dim": embedder.dim,
        "chunk_version": CHUNK_VERSION,
        "documents": [_document_id(document) for document in documents],
        "chunks": [chunk.id for chunk in chunks],
    }
    raw = json.dumps(payload, ensure_ascii=False, sort_keys=True)
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _stable_id(prefix: str, *parts: str) -> str:
    raw = "|".join(parts)
    digest = hashlib.sha256(raw.encode("utf-8")).hexdigest()[:24]
    return f"{prefix}_{digest}"


def _normalize_text(text: str) -> str:
    return re.sub(r"\s+", " ", text or "").strip()


def _tokenize(text: str) -> list[str]:
    return re.findall(r"[\u4e00-\u9fff]|[A-Za-z0-9_]+", text.lower())


def _normalize(vector: Sequence[float]) -> list[float]:
    norm = math.sqrt(sum(value * value for value in vector))
    if norm <= 0:
        return list(vector)
    return [value / norm for value in vector]


def _cosine_similarity(left: Sequence[float], right: Sequence[float]) -> float:
    if not left or not right or len(left) != len(right):
        return 0.0
    return sum(a * b for a, b in zip(left, right))


def _tier_boost(source_tier: str) -> float:
    return {
        "filing": 0.04,
        "news": 0.03,
        "demo": 0.0,
        "community": -0.02,
    }.get(source_tier, 0.0)


def _encode_vector(vector: Sequence[float]) -> bytes:
    return json.dumps([round(value, 8) for value in vector], separators=(",", ":")).encode("utf-8")


def _decode_vector(value: bytes) -> list[float]:
    return [float(item) for item in json.loads(value.decode("utf-8"))]


def _placeholders(values: Sequence[str]) -> str:
    return ",".join("?" for _ in values)


def _normalize_code(code: str) -> str:
    raw = (code or "").strip().upper()
    if "." in raw:
        return raw
    digits = "".join(char for char in raw if char.isdigit())
    if len(digits) != 6:
        return raw
    if digits.startswith("6"):
        return f"{digits}.SH"
    if digits.startswith(("4", "8")):
        return f"{digits}.BJ"
    return f"{digits}.SZ"
