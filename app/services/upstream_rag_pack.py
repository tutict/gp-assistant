from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import sqlite3
from collections import Counter
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta
from html import unescape
from html.parser import HTMLParser
from pathlib import Path
from typing import Sequence

import requests

from app.providers.base import StockProvider
from app.providers.tdx import TdxProvider
from app.schemas import StockItem


UPSTREAM_RAG_SCHEMA_VERSION = "upstream-rag-pack-v1"
VALID_SOURCE_TIERS = {
    "filing",
    "financial_snapshot",
    "news",
    "research",
    "community",
    "manual_url",
}
FACT_SOURCE_TIERS = {"filing", "news"}
FACT_RELATION_STATUSES = {"confirmed", "supported"}
VALID_RELATION_STATUSES = {"confirmed", "supported", "inferred", "rumor"}
DEFAULT_OUTPUT_ROOT = Path(os.getenv("GP_UPSTREAM_RAG_ROOT", "data/cache/upstream_rag"))
DEFAULT_REQUEST_TIMEOUT = float(os.getenv("GP_UPSTREAM_RAG_HTTP_TIMEOUT", "8"))
DEFAULT_USER_AGENT = "Mozilla/5.0 GP-Assistant/0.2"


@dataclass(frozen=True)
class UpstreamCollectedDocument:
    source_tier: str
    source_name: str
    title: str
    text: str
    source_url: str | None = None
    local_path: str | None = None
    published_at: str | None = None
    metadata_only: bool = False
    parse_quality: str = "parsed"


@dataclass(frozen=True)
class UpstreamSourceDocument:
    document_id: str
    source_tier: str
    title: str
    source_name: str = ""
    source_url: str | None = None
    local_path: str | None = None
    published_at: str | None = None
    metadata_only: bool = False
    test_source: bool = False


@dataclass(frozen=True)
class UpstreamEvidenceChunk:
    chunk_id: str
    document_id: str
    evidence_text: str
    can_answer_fact: bool = True


@dataclass(frozen=True)
class UpstreamRelationEdge:
    edge_id: str
    source_entity_id: str
    target_entity_id: str
    relation_type: str
    status: str
    evidence_text: str
    source_ref: str
    confidence: float = 0.0


@dataclass(frozen=True)
class UpstreamPackQualityReport:
    valid: bool
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    document_count: int = 0
    evidence_count: int = 0
    fact_evidence_count: int = 0
    metadata_only_count: int = 0
    relation_edge_count: int = 0
    source_tier_counts: dict[str, int] = field(default_factory=dict)


@dataclass(frozen=True)
class UpstreamPackBuildResult:
    pack_path: str
    manifest_path: str
    manifest: dict
    quality: UpstreamPackQualityReport
    notes: list[str] = field(default_factory=list)


def init_upstream_rag_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS pack_meta (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          schema_version TEXT NOT NULL,
          target_stock_code TEXT NOT NULL,
          target_stock_name TEXT NOT NULL,
          data_until TEXT NOT NULL,
          built_at TEXT NOT NULL,
          pack_version TEXT NOT NULL,
          sha256 TEXT,
          file_size INTEGER,
          valid INTEGER NOT NULL DEFAULT 0,
          document_count INTEGER NOT NULL DEFAULT 0,
          evidence_count INTEGER NOT NULL DEFAULT 0,
          relation_edge_count INTEGER NOT NULL DEFAULT 0,
          source_tier_stats TEXT NOT NULL DEFAULT '{}'
        );

        CREATE TABLE IF NOT EXISTS source_documents (
          id TEXT PRIMARY KEY,
          source_tier TEXT NOT NULL,
          source_name TEXT NOT NULL,
          title TEXT NOT NULL,
          source_url TEXT,
          local_path TEXT,
          published_at TEXT,
          fetched_at TEXT NOT NULL,
          metadata_only INTEGER NOT NULL DEFAULT 0,
          parse_quality TEXT NOT NULL DEFAULT 'parsed',
          raw_hash TEXT
        );

        CREATE TABLE IF NOT EXISTS evidence_chunks (
          id TEXT PRIMARY KEY,
          document_id TEXT NOT NULL REFERENCES source_documents(id),
          chunk_index INTEGER NOT NULL DEFAULT 0,
          evidence_text TEXT NOT NULL,
          can_answer_fact INTEGER NOT NULL DEFAULT 1,
          stock_codes TEXT NOT NULL DEFAULT '',
          industries TEXT NOT NULL DEFAULT '',
          relation_types TEXT NOT NULL DEFAULT '',
          keywords TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS relation_entities (
          id TEXT PRIMARY KEY,
          entity_type TEXT NOT NULL,
          entity_name TEXT NOT NULL,
          stock_code TEXT,
          industry TEXT
        );

        CREATE TABLE IF NOT EXISTS relation_edges (
          id TEXT PRIMARY KEY,
          source_entity_id TEXT NOT NULL REFERENCES relation_entities(id),
          target_entity_id TEXT NOT NULL REFERENCES relation_entities(id),
          relation_type TEXT NOT NULL,
          status TEXT NOT NULL,
          confidence REAL NOT NULL DEFAULT 0,
          evidence_chunk_id TEXT REFERENCES evidence_chunks(id),
          evidence_text TEXT NOT NULL,
          source_ref TEXT NOT NULL,
          source_tier TEXT NOT NULL DEFAULT '',
          source_name TEXT NOT NULL DEFAULT '',
          published_at TEXT
        );

        CREATE TABLE IF NOT EXISTS light_search_index (
          id TEXT PRIMARY KEY,
          stock_code TEXT,
          industry TEXT,
          relation_type TEXT,
          keyword TEXT NOT NULL,
          target_table TEXT NOT NULL,
          target_id TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_source_documents_tier ON source_documents(source_tier);
        CREATE INDEX IF NOT EXISTS idx_evidence_chunks_document ON evidence_chunks(document_id);
        CREATE INDEX IF NOT EXISTS idx_relation_edges_type_status ON relation_edges(relation_type, status);
        CREATE INDEX IF NOT EXISTS idx_light_search_lookup ON light_search_index(keyword, stock_code, relation_type);
        """
    )
    _ensure_column(conn, "relation_edges", "source_tier", "TEXT NOT NULL DEFAULT ''")
    _ensure_column(conn, "relation_edges", "source_name", "TEXT NOT NULL DEFAULT ''")
    _ensure_column(conn, "relation_edges", "published_at", "TEXT")


def build_upstream_rag_pack(
    provider: StockProvider,
    code: str,
    data_until: str | None = None,
    filing_days: int = 1095,
    news_days: int = 180,
    manual_urls: Sequence[str] = (),
    output_root: Path | None = None,
) -> UpstreamPackBuildResult:
    stock = provider.get_stock(code)
    data_until_value = _normalize_date(data_until) or date.today().isoformat()
    notes: list[str] = []

    documents: list[UpstreamCollectedDocument] = []
    documents.extend(_collect_cninfo_documents(stock, data_until_value, filing_days, notes))
    documents.extend(_collect_akshare_news_documents(stock, data_until_value, news_days, notes))
    documents.extend(_collect_tdx_f10_documents(stock, provider, notes))
    documents.extend(_collect_public_url_documents(stock, manual_urls, news_days, notes))
    documents = _dedupe_collected_documents(documents)

    if not documents:
        notes.append("没有采集到可入库信源；请检查网络或添加公开新闻/公告 URL。")

    source_documents = [_source_document_from_collected(document) for document in documents]
    evidence_chunks = _build_evidence_chunks(stock, documents)
    entity_rows, relation_edges = _extract_relation_edges(stock, documents, evidence_chunks)
    quality = validate_upstream_pack(stock.code, stock.name, source_documents, evidence_chunks, relation_edges)
    notes.extend(quality.warnings)
    if quality.errors:
        notes.extend(quality.errors)

    built_at = datetime.now()
    content_hash = _content_hash(stock, documents, evidence_chunks, relation_edges)
    pack_version = build_upstream_pack_version(stock.code, data_until_value, built_at, content_hash)
    root = output_root or DEFAULT_OUTPUT_ROOT
    pack_dir = root / _safe_path_part(stock.code) / _safe_path_part(pack_version)
    tmp_dir = pack_dir.with_name(pack_dir.name + ".tmp")
    if tmp_dir.exists():
        shutil.rmtree(tmp_dir)
    tmp_dir.mkdir(parents=True, exist_ok=True)
    pack_path = tmp_dir / "rag_pack.sqlite"
    manifest_path = tmp_dir / "manifest.json"

    conn = sqlite3.connect(pack_path)
    try:
        conn.row_factory = sqlite3.Row
        init_upstream_rag_schema(conn)
        _store_upstream_pack(
            conn,
            stock,
            data_until_value,
            built_at,
            pack_version,
            quality,
            source_documents,
            evidence_chunks,
            entity_rows,
            relation_edges,
        )
        conn.commit()
    finally:
        conn.close()

    file_sha256 = _file_sha256(pack_path)
    file_size = pack_path.stat().st_size
    manifest = _build_manifest(
        stock=stock,
        data_until=data_until_value,
        built_at=built_at,
        pack_version=pack_version,
        file_sha256=file_sha256,
        file_size=file_size,
        quality=quality,
        source_documents=source_documents,
        evidence_chunks=evidence_chunks,
        entity_rows=entity_rows,
        relation_edges=relation_edges,
        notes=notes,
    )
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")

    if pack_dir.exists():
        shutil.rmtree(pack_dir)
    tmp_dir.replace(pack_dir)
    return UpstreamPackBuildResult(
        pack_path=str(pack_dir / "rag_pack.sqlite"),
        manifest_path=str(pack_dir / "manifest.json"),
        manifest=manifest,
        quality=quality,
        notes=notes,
    )


def latest_upstream_pack(root: Path | None = None) -> dict | None:
    base = root or DEFAULT_OUTPUT_ROOT
    if not base.exists():
        return None
    manifests = sorted(base.glob("*/*/manifest.json"), key=lambda path: path.stat().st_mtime, reverse=True)
    for manifest_path in manifests:
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        pack_path = manifest_path.with_name("rag_pack.sqlite")
        if pack_path.exists():
            manifest["_manifest_path"] = str(manifest_path)
            manifest["_pack_path"] = str(pack_path)
            return manifest
    return None


def validate_upstream_pack(
    target_stock_code: str,
    target_stock_name: str,
    documents: Sequence[UpstreamSourceDocument],
    evidence_chunks: Sequence[UpstreamEvidenceChunk],
    relation_edges: Sequence[UpstreamRelationEdge],
) -> UpstreamPackQualityReport:
    errors: list[str] = []
    warnings: list[str] = []

    if not target_stock_code.strip():
        errors.append("缺少目标股票代码。")
    if not target_stock_name.strip():
        errors.append("缺少目标股票名称。")

    tier_counts = Counter(_normalize_source_tier(document.source_tier) for document in documents)
    metadata_only_count = sum(1 for document in documents if document.metadata_only)
    fact_document_count = sum(
        1
        for document in documents
        if _normalize_source_tier(document.source_tier) in FACT_SOURCE_TIERS and not document.metadata_only
    )
    fact_evidence_count = sum(1 for chunk in evidence_chunks if chunk.can_answer_fact and chunk.evidence_text.strip())

    if not documents:
        errors.append("没有可打包的信源文档。")
    if fact_document_count <= 0:
        errors.append("缺少 filing 或 news 层的可回答事实证据。")
    if fact_evidence_count <= 0:
        errors.append("缺少可回答事实的证据片段。")

    for document in documents:
        tier = _normalize_source_tier(document.source_tier)
        if tier not in VALID_SOURCE_TIERS:
            warnings.append(f"未知信源层级：{document.source_tier}。")
        if not (document.source_url or document.local_path):
            errors.append(f"信源缺少 source_url 或 local_path：{document.document_id}。")
        if document.test_source:
            errors.append(f"不能同步测试数据源：{document.document_id}。")
        if document.metadata_only:
            warnings.append(f"metadata_only 文档只能提示存在相关披露，不能生成事实结论：{document.document_id}。")

    confirmed_or_supported = [
        edge for edge in relation_edges if _normalize_relation_status(edge.status) in FACT_RELATION_STATUSES
    ]
    if not confirmed_or_supported:
        errors.append("缺少 confirmed 或 supported 上下游关系边。")

    for edge in relation_edges:
        status = _normalize_relation_status(edge.status)
        if status not in VALID_RELATION_STATUSES:
            errors.append(f"关系边状态非法：{edge.edge_id}。")
        if not edge.evidence_text.strip():
            errors.append(f"关系边缺少 evidence_text：{edge.edge_id}。")
        if not edge.source_ref.strip():
            errors.append(f"关系边缺少 source_ref：{edge.edge_id}。")

    return UpstreamPackQualityReport(
        valid=not errors,
        errors=errors,
        warnings=warnings,
        document_count=len(documents),
        evidence_count=len(evidence_chunks),
        fact_evidence_count=fact_evidence_count,
        metadata_only_count=metadata_only_count,
        relation_edge_count=len(relation_edges),
        source_tier_counts=dict(sorted(tier_counts.items())),
    )


def build_upstream_pack_version(
    stock_code: str,
    data_until: str,
    built_at: datetime | None = None,
    content_sha256: str | None = None,
) -> str:
    timestamp = (built_at or datetime.now()).strftime("%H%M%S")
    digest = (content_sha256 or _stable_sha256(stock_code, data_until, timestamp))[:8]
    return f"{stock_code.upper()}_{data_until}_{timestamp}_{digest}"


def _collect_cninfo_documents(
    stock: StockItem,
    data_until: str,
    days: int,
    notes: list[str],
) -> list[UpstreamCollectedDocument]:
    try:
        import akshare as ak
    except ImportError as exc:
        notes.append(f"CNINFO 采集跳过：AkShare 不可用：{exc}")
        return []

    end = _date_key(data_until)
    start = _date_key((datetime.strptime(data_until, "%Y-%m-%d") - timedelta(days=days)).date().isoformat())
    rows: list[dict] = []
    for category in ("日常经营", "业绩预告", "年报", "半年报", "一季报", "三季报", "风险提示"):
        try:
            frame = ak.stock_zh_a_disclosure_report_cninfo(
                symbol=_code_digits(stock.code),
                market="沪深京",
                category=category,
                start_date=start,
                end_date=end,
            )
        except Exception as exc:
            notes.append(f"CNINFO {category} 采集失败：{_safe_error(exc)}")
            continue
        if frame is None or frame.empty:
            continue
        rows.extend(frame.head(12).to_dict("records"))

    documents: list[UpstreamCollectedDocument] = []
    for row in rows:
        title = _clean_text(str(_coalesce_row(row, ["公告标题", "title", "标题"]) or ""))
        if not title:
            continue
        published_at = _normalize_datetime_string(_coalesce_row(row, ["公告时间", "date", "日期"]))
        url = _coalesce_row(row, ["公告链接", "url", "链接"])
        text = title
        documents.append(
            UpstreamCollectedDocument(
                source_tier="filing",
                source_name="巨潮资讯",
                title=title,
                text=text,
                source_url=url or f"cninfo://{_code_digits(stock.code)}/{_stable_sha256(title)[:12]}",
                published_at=published_at,
                metadata_only=False,
                parse_quality="title_only",
            )
        )
    if documents:
        notes.append(f"CNINFO/巨潮采集 {len(documents)} 条公告标题和链接。")
    return documents


def _collect_tdx_f10_documents(
    stock: StockItem,
    provider: StockProvider,
    notes: list[str],
) -> list[UpstreamCollectedDocument]:
    if not isinstance(provider, TdxProvider):
        notes.append("通达信 F10 采集跳过：当前 provider 不是通达信。")
        return []

    documents: list[UpstreamCollectedDocument] = []
    market = TdxProvider._tdx_market_code(stock.code)
    digits = _code_digits(stock.code)
    if market is None:
        return []

    try:
        def fetch(api):
            result: list[UpstreamCollectedDocument] = []
            finance = api.get_finance_info(market, digits) or {}
            finance_text = _tdx_finance_summary(stock, finance)
            if finance_text:
                result.append(
                    UpstreamCollectedDocument(
                        source_tier="financial_snapshot",
                        source_name="通达信",
                        title=f"{stock.name} 财务快照",
                        text=finance_text,
                        source_url=f"tdx://finance/{stock.code}",
                        published_at=_tdx_finance_date(finance),
                        metadata_only=False,
                        parse_quality="parsed",
                    )
                )
            categories = api.get_company_info_category(market, digits) or []
            allowed = ("最新提示", "公司概况", "财务分析", "经营分析", "公司大事", "研究报告")
            for item in categories:
                name = str(item.get("name") or "").strip()
                if not name or not any(keyword in name for keyword in allowed):
                    continue
                try:
                    content = api.get_company_info_content(
                        market,
                        digits,
                        item.get("filename"),
                        int(item.get("start") or 0),
                        int(item.get("length") or 0),
                    )
                except Exception:
                    continue
                text = _clean_text(content if isinstance(content, str) else str(content or ""))
                if not text:
                    continue
                result.append(
                    UpstreamCollectedDocument(
                        source_tier="financial_snapshot",
                        source_name="通达信 F10",
                        title=f"{stock.name} {name}",
                        text=text[:3000],
                        source_url=f"tdx://f10/{stock.code}/{name}",
                        published_at=None,
                        metadata_only=False,
                        parse_quality="parsed",
                    )
                )
            return result

        documents = provider._with_connected_api(fetch)
    except Exception as exc:
        notes.append(f"通达信 F10 采集失败：{_safe_error(exc)}")
        return []

    if documents:
        notes.append(f"通达信 F10/财务快照采集 {len(documents)} 条。")
    return documents


def _collect_akshare_news_documents(
    stock: StockItem,
    data_until: str,
    days: int,
    notes: list[str],
) -> list[UpstreamCollectedDocument]:
    try:
        import akshare as ak
    except ImportError as exc:
        notes.append(f"新闻采集跳过：AkShare 不可用：{exc}")
        return []

    cutoff = datetime.strptime(data_until, "%Y-%m-%d") - timedelta(days=days)
    symbol = _code_digits(stock.code)
    try:
        frame = ak.stock_news_em(symbol=symbol)
    except Exception as exc:
        notes.append(f"新闻采集失败：{_safe_error(exc)}")
        return []
    if frame is None or frame.empty:
        return []

    documents: list[UpstreamCollectedDocument] = []
    for row in frame.head(24).to_dict("records"):
        title = _clean_text(_coalesce_row(row, ["新闻标题", "标题", "title"]))
        if not title:
            continue
        published_at = _normalize_datetime_string(_coalesce_row(row, ["发布时间", "时间", "datetime", "date"]))
        parsed_at = _parse_datetime_for_cutoff(published_at)
        if parsed_at and parsed_at < cutoff:
            continue
        summary = _clean_text(_coalesce_row(row, ["新闻内容", "内容", "摘要", "summary"]) or title)
        url = _coalesce_row(row, ["新闻链接", "链接", "url"]) or f"akshare://stock_news_em/{symbol}"
        documents.append(
            UpstreamCollectedDocument(
                source_tier="news",
                source_name="东方财富个股新闻",
                title=title,
                text=summary,
                source_url=url,
                published_at=published_at,
                metadata_only=False,
                parse_quality="parsed",
            )
        )
    if documents:
        notes.append(f"新闻采集 {len(documents)} 条。")
    return documents


def _collect_public_url_documents(
    stock: StockItem,
    urls: Sequence[str],
    days: int,
    notes: list[str],
) -> list[UpstreamCollectedDocument]:
    session = requests.Session()
    session.headers.update({"User-Agent": DEFAULT_USER_AGENT})
    documents: list[UpstreamCollectedDocument] = []
    for url in [item.strip() for item in urls if item and item.strip()][:12]:
        try:
            response = session.get(url, timeout=DEFAULT_REQUEST_TIMEOUT)
            response.raise_for_status()
        except Exception as exc:
            notes.append(f"公开 URL 抓取失败：{url}：{_safe_error(exc)}")
            continue
        html = response.text
        title = _html_title(html) or url
        text = _clean_text(_strip_html(html))
        if not text:
            text = title
        documents.append(
            UpstreamCollectedDocument(
                source_tier="manual_url",
                source_name=_host_from_url(url) or "公开 URL",
                title=title,
                text=text[:5000],
                source_url=url,
                published_at=None,
                metadata_only=False,
                parse_quality="parsed",
            )
        )
    if documents:
        notes.append(f"公开 URL 采集 {len(documents)} 条。")
    return documents


def _source_document_from_collected(document: UpstreamCollectedDocument) -> UpstreamSourceDocument:
    return UpstreamSourceDocument(
        document_id=_document_id(document),
        source_tier=_normalize_source_tier(document.source_tier),
        title=document.title,
        source_name=document.source_name,
        source_url=document.source_url,
        local_path=document.local_path,
        published_at=document.published_at,
        metadata_only=document.metadata_only,
        test_source=False,
    )


def _build_evidence_chunks(
    stock: StockItem,
    documents: Sequence[UpstreamCollectedDocument],
) -> list[UpstreamEvidenceChunk]:
    chunks: list[UpstreamEvidenceChunk] = []
    for document in documents:
        doc_id = _document_id(document)
        text = _clean_text(f"{document.title}。{document.text}")
        if not text:
            continue
        for index, part in enumerate(_chunk_text(text, 520)):
            chunks.append(
                UpstreamEvidenceChunk(
                    chunk_id=_stable_id("chunk", doc_id, str(index), part),
                    document_id=doc_id,
                    evidence_text=part,
                    can_answer_fact=not document.metadata_only,
                )
            )
    return chunks


def _extract_relation_edges(
    stock: StockItem,
    documents: Sequence[UpstreamCollectedDocument],
    chunks: Sequence[UpstreamEvidenceChunk],
) -> tuple[dict[str, dict], list[UpstreamRelationEdge]]:
    target_id = _stable_id("entity", stock.code, "target")
    entities: dict[str, dict] = {
        target_id: {
            "id": target_id,
            "entity_type": "target_stock",
            "entity_name": stock.name or stock.code,
            "stock_code": stock.code,
            "industry": stock.industry,
        }
    }
    chunk_by_doc: dict[str, list[UpstreamEvidenceChunk]] = {}
    for chunk in chunks:
        chunk_by_doc.setdefault(chunk.document_id, []).append(chunk)

    edges: list[UpstreamRelationEdge] = []
    for document in documents:
        doc_id = _document_id(document)
        doc_chunks = chunk_by_doc.get(doc_id, [])
        if not doc_chunks:
            continue
        evidence = doc_chunks[0].evidence_text
        matched_rules = _match_relation_rules(evidence)
        if not matched_rules and _normalize_source_tier(document.source_tier) in {"filing", "news", "manual_url"}:
            matched_rules = [("event", "披露事项", "event")]
        status = _status_for_source_tier(document.source_tier)
        for relation_type, entity_name, entity_type in matched_rules[:3]:
            entity_id = _stable_id("entity", stock.code, relation_type, entity_name)
            entities.setdefault(
                entity_id,
                {
                    "id": entity_id,
                    "entity_type": entity_type,
                    "entity_name": entity_name,
                    "stock_code": None,
                    "industry": None,
                },
            )
            source_entity_id, target_entity_id = _edge_direction(target_id, entity_id, relation_type)
            edge = UpstreamRelationEdge(
                edge_id=_stable_id("edge", doc_id, relation_type, source_entity_id, target_entity_id),
                source_entity_id=source_entity_id,
                target_entity_id=target_entity_id,
                relation_type=relation_type,
                status=status,
                evidence_text=evidence[:500],
                source_ref=doc_chunks[0].chunk_id,
                confidence=_confidence_for_status(status),
            )
            edges.append(edge)
    return entities, _dedupe_edges(edges)


def _store_upstream_pack(
    conn: sqlite3.Connection,
    stock: StockItem,
    data_until: str,
    built_at: datetime,
    pack_version: str,
    quality: UpstreamPackQualityReport,
    source_documents: Sequence[UpstreamSourceDocument],
    evidence_chunks: Sequence[UpstreamEvidenceChunk],
    entity_rows: dict[str, dict],
    relation_edges: Sequence[UpstreamRelationEdge],
) -> None:
    fetched_at = built_at.isoformat(timespec="seconds")
    conn.execute(
        """
        INSERT INTO pack_meta (
          id, schema_version, target_stock_code, target_stock_name, data_until, built_at,
          pack_version, sha256, file_size, valid, document_count, evidence_count,
          relation_edge_count, source_tier_stats
        )
        VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            UPSTREAM_RAG_SCHEMA_VERSION,
            stock.code,
            stock.name,
            data_until,
            fetched_at,
            pack_version,
            "",
            0,
            1 if quality.valid else 0,
            quality.document_count,
            quality.evidence_count,
            quality.relation_edge_count,
            json.dumps(quality.source_tier_counts, ensure_ascii=False, sort_keys=True),
        ),
    )
    for document in source_documents:
        conn.execute(
            """
            INSERT INTO source_documents (
              id, source_tier, source_name, title, source_url, local_path, published_at,
              fetched_at, metadata_only, parse_quality, raw_hash
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                document.document_id,
                _normalize_source_tier(document.source_tier),
                document.source_name,
                document.title,
                document.source_url,
                document.local_path,
                document.published_at,
                fetched_at,
                1 if document.metadata_only else 0,
                "metadata_only" if document.metadata_only else "parsed",
                _stable_sha256(document.title, document.source_url or "", document.published_at or ""),
            ),
        )
    for index, chunk in enumerate(evidence_chunks):
        relation_types = sorted(
            {
                edge.relation_type
                for edge in relation_edges
                if edge.source_ref == chunk.chunk_id
            }
        )
        conn.execute(
            """
            INSERT INTO evidence_chunks (
              id, document_id, chunk_index, evidence_text, can_answer_fact, stock_codes,
              industries, relation_types, keywords
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                chunk.chunk_id,
                chunk.document_id,
                index,
                chunk.evidence_text,
                1 if chunk.can_answer_fact else 0,
                stock.code,
                stock.industry,
                ",".join(relation_types),
                ",".join(_keywords(chunk.evidence_text, relation_types)),
            ),
        )
    for entity in entity_rows.values():
        conn.execute(
            """
            INSERT OR IGNORE INTO relation_entities (id, entity_type, entity_name, stock_code, industry)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                entity["id"],
                entity["entity_type"],
                entity["entity_name"],
                entity.get("stock_code"),
                entity.get("industry"),
            ),
        )
    docs_by_id = {document.document_id: document for document in source_documents}
    chunks_by_id = {chunk.chunk_id: chunk for chunk in evidence_chunks}
    for edge in relation_edges:
        chunk = chunks_by_id.get(edge.source_ref)
        document = docs_by_id.get(chunk.document_id) if chunk else None
        conn.execute(
            """
            INSERT INTO relation_edges (
              id, source_entity_id, target_entity_id, relation_type, status, confidence,
              evidence_chunk_id, evidence_text, source_ref, source_tier, source_name, published_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                edge.edge_id,
                edge.source_entity_id,
                edge.target_entity_id,
                edge.relation_type,
                edge.status,
                edge.confidence,
                edge.source_ref,
                edge.evidence_text,
                edge.source_ref,
                document.source_tier if document else "",
                document.source_name if document else "",
                document.published_at if document else None,
            ),
        )
    _store_light_search_index(conn, stock, evidence_chunks, relation_edges)


def _store_light_search_index(
    conn: sqlite3.Connection,
    stock: StockItem,
    chunks: Sequence[UpstreamEvidenceChunk],
    edges: Sequence[UpstreamRelationEdge],
) -> None:
    for chunk in chunks:
        for keyword in _keywords(chunk.evidence_text, []):
            conn.execute(
                """
                INSERT OR IGNORE INTO light_search_index
                (id, stock_code, industry, relation_type, keyword, target_table, target_id)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    _stable_id("idx", chunk.chunk_id, keyword),
                    stock.code,
                    stock.industry,
                    "",
                    keyword,
                    "evidence_chunks",
                    chunk.chunk_id,
                ),
            )
    for edge in edges:
        for keyword in _keywords(edge.evidence_text, [edge.relation_type]):
            conn.execute(
                """
                INSERT OR IGNORE INTO light_search_index
                (id, stock_code, industry, relation_type, keyword, target_table, target_id)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    _stable_id("idx", edge.edge_id, keyword),
                    stock.code,
                    stock.industry,
                    edge.relation_type,
                    keyword,
                    "relation_edges",
                    edge.edge_id,
                ),
            )


def _build_manifest(
    stock: StockItem,
    data_until: str,
    built_at: datetime,
    pack_version: str,
    file_sha256: str,
    file_size: int,
    quality: UpstreamPackQualityReport,
    source_documents: Sequence[UpstreamSourceDocument],
    evidence_chunks: Sequence[UpstreamEvidenceChunk],
    entity_rows: dict[str, dict],
    relation_edges: Sequence[UpstreamRelationEdge],
    notes: Sequence[str],
) -> dict:
    chunks_by_id = {chunk.chunk_id: chunk for chunk in evidence_chunks}
    docs_by_id = {document.document_id: document for document in source_documents}
    edges_preview = []
    for edge in sorted(relation_edges, key=lambda item: (_status_sort(item.status), -item.confidence))[:40]:
        chunk = chunks_by_id.get(edge.source_ref)
        document = docs_by_id.get(chunk.document_id) if chunk else None
        edges_preview.append(
            {
                "id": edge.edge_id,
                "source_entity": entity_rows.get(edge.source_entity_id, {}),
                "target_entity": entity_rows.get(edge.target_entity_id, {}),
                "relation_type": edge.relation_type,
                "status": edge.status,
                "confidence": edge.confidence,
                "evidence_text": edge.evidence_text,
                "source_ref": edge.source_ref,
                "source_tier": document.source_tier if document else "",
                "source_name": document.source_name if document else "",
                "published_at": document.published_at if document else None,
                "source_url": document.source_url if document else None,
            }
        )
    evidence_preview = []
    for chunk in evidence_chunks[:80]:
        document = docs_by_id.get(chunk.document_id)
        evidence_preview.append(
            {
                "id": chunk.chunk_id,
                "document_id": chunk.document_id,
                "title": document.title if document else "",
                "source_tier": document.source_tier if document else "",
                "source_name": document.source_name if document else "",
                "published_at": document.published_at if document else None,
                "source_url": document.source_url if document else None,
                "evidence_text": chunk.evidence_text,
                "can_answer_fact": chunk.can_answer_fact,
            }
        )
    return {
        "schema_version": UPSTREAM_RAG_SCHEMA_VERSION,
        "valid": quality.valid,
        "target_stock_code": stock.code,
        "target_stock_name": stock.name,
        "target_stock_industry": stock.industry,
        "data_until": data_until,
        "built_at": built_at.isoformat(timespec="seconds"),
        "pack_version": pack_version,
        "sha256": file_sha256,
        "file_size": file_size,
        "document_count": quality.document_count,
        "evidence_count": quality.evidence_count,
        "fact_evidence_count": quality.fact_evidence_count,
        "metadata_only_count": quality.metadata_only_count,
        "relation_edge_count": quality.relation_edge_count,
        "source_tier_stats": quality.source_tier_counts,
        "quality": {
            "valid": quality.valid,
            "errors": quality.errors,
            "warnings": quality.warnings,
        },
        "files": {"pack": "rag_pack.sqlite", "manifest": "manifest.json"},
        "relation_edges": edges_preview,
        "evidence_chunks": evidence_preview,
        "notes": list(dict.fromkeys(notes)),
    }


def _match_relation_rules(text: str) -> list[tuple[str, str, str]]:
    normalized = text.lower()
    rules = [
        ("raw_material_price", "原材料价格/供给", "raw_material", ("原材料", "碳酸锂", "电解液", "正极", "负极", "材料", "涨价", "价格")),
        ("supplier", "上游供应商/供给", "supplier", ("供应商", "采购", "供货", "上游", "供给", "供应链")),
        ("customer", "下游客户/订单", "customer", ("客户", "订单", "中标", "合同", "销售", "交付", "需求", "下游")),
        ("product", "主营产品/应用", "product", ("产品", "主营", "业务", "应用", "装机", "电池", "组件", "设备")),
        ("capacity", "产能/投产项目", "capacity", ("产能", "扩产", "投产", "项目", "基地", "建设")),
        ("financial", "财务表现", "financial_metric", ("营收", "收入", "净利润", "毛利", "现金流", "增长", "下滑", "亏损")),
        ("risk", "风险事项", "risk_event", ("风险", "处罚", "调查", "诉讼", "减值", "违约", "退市", "停产")),
    ]
    matches = []
    for relation_type, entity_name, entity_type, keywords in rules:
        if any(keyword.lower() in normalized for keyword in keywords):
            matches.append((relation_type, entity_name, entity_type))
    return matches


def _edge_direction(target_id: str, entity_id: str, relation_type: str) -> tuple[str, str]:
    upstream = {"supplier", "raw_material_price", "risk"}
    downstream = {"customer", "product", "capacity", "financial", "event"}
    if relation_type in upstream:
        return entity_id, target_id
    if relation_type in downstream:
        return target_id, entity_id
    return target_id, entity_id


def _stable_sha256(*parts: str) -> str:
    raw = "|".join(parts)
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _content_hash(
    stock: StockItem,
    documents: Sequence[UpstreamCollectedDocument],
    chunks: Sequence[UpstreamEvidenceChunk],
    edges: Sequence[UpstreamRelationEdge],
) -> str:
    payload = {
        "schema_version": UPSTREAM_RAG_SCHEMA_VERSION,
        "stock": stock.code,
        "documents": [
            {
                "tier": item.source_tier,
                "title": item.title,
                "url": item.source_url,
                "published_at": item.published_at,
                "hash": _stable_sha256(item.text),
            }
            for item in documents
        ],
        "chunks": [item.chunk_id for item in chunks],
        "edges": [item.edge_id for item in edges],
    }
    return hashlib.sha256(json.dumps(payload, ensure_ascii=False, sort_keys=True).encode("utf-8")).hexdigest()


def _file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _stable_id(prefix: str, *parts: str) -> str:
    return f"{prefix}_{_stable_sha256(*parts)[:24]}"


def _document_id(document: UpstreamCollectedDocument) -> str:
    return _stable_id(
        "doc",
        _normalize_source_tier(document.source_tier),
        document.source_name,
        document.title,
        document.source_url or "",
        document.published_at or "",
        document.text[:500],
    )


def _dedupe_collected_documents(documents: Sequence[UpstreamCollectedDocument]) -> list[UpstreamCollectedDocument]:
    seen: set[str] = set()
    result: list[UpstreamCollectedDocument] = []
    for document in documents:
        key = _document_id(document)
        if key in seen:
            continue
        seen.add(key)
        result.append(document)
    return result[:120]


def _dedupe_edges(edges: Sequence[UpstreamRelationEdge]) -> list[UpstreamRelationEdge]:
    seen: set[str] = set()
    result: list[UpstreamRelationEdge] = []
    for edge in edges:
        if edge.edge_id in seen:
            continue
        seen.add(edge.edge_id)
        result.append(edge)
    return result


def _chunk_text(text: str, target_chars: int) -> list[str]:
    normalized = _clean_text(text)
    if not normalized:
        return []
    if len(normalized) <= target_chars:
        return [normalized]
    parts: list[str] = []
    for start in range(0, len(normalized), target_chars):
        part = normalized[start : start + target_chars]
        if part:
            parts.append(part)
    return parts[:6]


def _keywords(text: str, relation_types: Sequence[str]) -> list[str]:
    base = set(relation_types)
    for keyword in (
        "上游",
        "下游",
        "供应商",
        "客户",
        "订单",
        "中标",
        "产能",
        "扩产",
        "投产",
        "原材料",
        "价格",
        "涨价",
        "财报",
        "公告",
        "风险",
        "产品",
        "应用",
    ):
        if keyword in text:
            base.add(keyword)
    return sorted(base)[:16]


def _status_for_source_tier(source_tier: str) -> str:
    tier = _normalize_source_tier(source_tier)
    if tier in {"filing", "financial_snapshot"}:
        return "confirmed"
    if tier in {"news", "research", "manual_url"}:
        return "supported"
    if tier == "community":
        return "rumor"
    return "inferred"


def _confidence_for_status(status: str) -> float:
    return {"confirmed": 0.9, "supported": 0.68, "inferred": 0.45, "rumor": 0.2}.get(status, 0.35)


def _status_sort(status: str) -> int:
    return {"confirmed": 0, "supported": 1, "inferred": 2, "rumor": 3}.get(status, 9)


def _tdx_finance_summary(stock: StockItem, finance: dict) -> str:
    if not finance:
        return ""
    fields = [
        ("更新日期", finance.get("updated_date")),
        ("总资产", finance.get("zongzichan")),
        ("净资产", finance.get("jingzichan")),
        ("主营收入", finance.get("zhuyingshouru")),
        ("主营利润", finance.get("zhuyinglirun")),
        ("营业利润", finance.get("yingyelirun")),
        ("净利润", finance.get("jinglirun")),
        ("经营现金流", finance.get("jingyingxianjinliu")),
        ("每股净资产", finance.get("meigujingzichan")),
        ("股东人数", finance.get("gudongrenshu")),
    ]
    values = [f"{label}：{value}" for label, value in fields if value not in (None, "")]
    if not values:
        return ""
    return f"{stock.name}（{stock.code}）通达信财务快照。 " + "；".join(values)


def _tdx_finance_date(finance: dict) -> str | None:
    raw = str(finance.get("updated_date") or "").strip()
    if re.fullmatch(r"\d{8}", raw):
        return f"{raw[:4]}-{raw[4:6]}-{raw[6:8]}"
    return None


def _normalize_date(value: str | None) -> str | None:
    if not value:
        return None
    raw = str(value).strip()
    for fmt in ("%Y-%m-%d", "%Y%m%d"):
        try:
            return datetime.strptime(raw, fmt).date().isoformat()
        except ValueError:
            continue
    return None


def _date_key(value: str) -> str:
    normalized = _normalize_date(value) or date.today().isoformat()
    return normalized.replace("-", "")


def _normalize_datetime_string(value: object) -> str | None:
    if value is None:
        return None
    raw = str(value).strip()
    if not raw or raw.lower() in {"nat", "none"}:
        return None
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%d", "%Y/%m/%d %H:%M:%S", "%Y/%m/%d"):
        try:
            return datetime.strptime(raw[:19], fmt).isoformat(timespec="seconds")
        except ValueError:
            continue
    return raw[:19]


def _parse_datetime_for_cutoff(value: str | None) -> datetime | None:
    if not value:
        return None
    raw = value.strip()
    for fmt in ("%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"):
        try:
            return datetime.strptime(raw[:19], fmt)
        except ValueError:
            continue
    return None


def _code_digits(code: str) -> str:
    raw = str(code or "").strip().upper()
    if "." in raw:
        raw = raw.split(".", 1)[0]
    if raw.startswith(("SH", "SZ", "BJ")):
        raw = raw[2:]
    return "".join(ch for ch in raw if ch.isdigit())[:6]


def _coalesce_row(row: dict, keys: Sequence[str]) -> str:
    for key in keys:
        value = row.get(key)
        if value is not None and str(value).strip():
            return str(value).strip()
    return ""


def _safe_path_part(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", value or "unknown")[:120]


def _safe_error(error: Exception) -> str:
    return re.sub(r"\s+", " ", str(error)).strip()[:180]


def _clean_text(value: str) -> str:
    return re.sub(r"\s+", " ", value or "").strip()


def _host_from_url(url: str) -> str:
    match = re.match(r"https?://([^/]+)", url or "", flags=re.I)
    return match.group(1) if match else ""


class _HtmlTextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.parts: list[str] = []
        self._skip_depth = 0

    def handle_starttag(self, tag: str, attrs) -> None:
        if tag.lower() in {"script", "style", "noscript"}:
            self._skip_depth += 1

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() in {"script", "style", "noscript"} and self._skip_depth:
            self._skip_depth -= 1

    def handle_data(self, data: str) -> None:
        if self._skip_depth:
            return
        text = data.strip()
        if text:
            self.parts.append(text)

    def text(self) -> str:
        return unescape(" ".join(self.parts)).strip()


def _strip_html(value: str) -> str:
    parser = _HtmlTextExtractor()
    parser.feed(value or "")
    return parser.text()


def _html_title(value: str) -> str:
    match = re.search(r"<title[^>]*>(.*?)</title>", value or "", flags=re.I | re.S)
    return _clean_text(_strip_html(match.group(1))) if match else ""


def _ensure_column(conn: sqlite3.Connection, table: str, column: str, definition: str) -> None:
    columns = {row[1] for row in conn.execute(f"PRAGMA table_info({table})").fetchall()}
    if column not in columns:
        conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {definition}")


def _normalize_source_tier(value: str) -> str:
    normalized = (value or "").strip().lower()
    if normalized in {"cninfo", "notice", "announcement"}:
        return "filing"
    if normalized in {"tdx", "f10"}:
        return "financial_snapshot"
    if not normalized:
        return "manual_url"
    return normalized


def _normalize_relation_status(value: str) -> str:
    return (value or "").strip().lower()
