from __future__ import annotations

import hashlib
import os
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timedelta
from pathlib import Path
from typing import Iterable, List, Sequence

from app.providers.base import StockProvider
from app.schemas import (
    NewsEvidence,
    NewsImpactFinding,
    NewsRagRequest,
    NewsRagResult,
    ScreenedStock,
    StockItem,
    StockRelation,
)
from app.services.knowledge_graph import build_knowledge_graph
from app.services.screener import screen_stocks, screening_universe


CACHE_PATH = Path(os.getenv("GP_NEWS_CACHE", "data/cache/news.sqlite"))
CHAIN_RELATION_TYPES = {"supply_chain", "manufacturing_chain", "upstream_material"}


@dataclass(frozen=True)
class RawNewsItem:
    title: str
    summary: str
    source: str
    url: str
    published_at: str
    stock_codes: tuple[str, ...]
    industries: tuple[str, ...]
    relation_types: tuple[str, ...]
    sentiment: str


def analyze_supply_chain_news(provider: StockProvider, request: NewsRagRequest) -> NewsRagResult:
    universe, screen_notes = screening_universe(provider)
    stock_by_code = {stock.code: stock for stock in universe}
    graph = build_knowledge_graph(universe, provider.list_relations())
    scope_codes = _scope_codes(provider, request, universe, stock_by_code)
    chain_relations = _scope_chain_relations(scope_codes, graph.relations)
    related_codes = sorted(
        set(scope_codes)
        | {
            code
            for relation in chain_relations
            for code in (relation.source_code, relation.target_code)
        }
    )
    related_stocks = [stock_by_code[code] for code in related_codes if code in stock_by_code]

    fetched, adapter_notes = _fetch_news_items(related_stocks, chain_relations, request.days)
    conn = _connect()
    try:
        _init_db(conn)
        _store_news(conn, fetched)
        evidence = _query_evidence(conn, related_codes, request.days, request.max_items)
    finally:
        conn.close()

    findings = _build_findings(scope_codes, chain_relations, evidence, stock_by_code)
    notes = [
        *screen_notes,
        *adapter_notes,
        "RAG 只在已有股票关系图范围内分析消息，不从新闻自动发明上下游关系。",
        f"消息缓存：{CACHE_PATH}。",
    ]
    if not chain_relations:
        notes.append("当前范围没有供应链、制造链或上游材料关系，结果仅保留可解释提示。")
    if not evidence:
        notes.append("当前时间窗口没有命中可用消息。")

    return NewsRagResult(
        scope_codes=scope_codes,
        relation_count=len(chain_relations),
        message_count=len(evidence),
        findings=findings,
        notes=notes,
    )


def _scope_codes(
    provider: StockProvider,
    request: NewsRagRequest,
    universe: Sequence[StockItem],
    stock_by_code: dict[str, StockItem],
) -> List[str]:
    explicit_codes = [_normalize_code(code) for code in [request.code, *request.seed_codes] if code]
    explicit_codes = _dedupe_codes([code for code in explicit_codes if code in stock_by_code])
    if explicit_codes:
        return explicit_codes[:10]

    screened: List[ScreenedStock] = screen_stocks(
        list(universe),
        request.criteria.model_copy(update={"limit": min(max(request.criteria.limit, 5), 20)}),
    ).items
    return [item.stock.code for item in screened[:10]]


def _dedupe_codes(codes: Sequence[str]) -> List[str]:
    seen: set[str] = set()
    result: List[str] = []
    for code in codes:
        if code in seen:
            continue
        seen.add(code)
        result.append(code)
    return result


def _scope_chain_relations(scope_codes: Sequence[str], relations: Sequence[StockRelation]) -> List[StockRelation]:
    scope = set(scope_codes)
    chain_relations = [
        relation
        for relation in relations
        if relation.relation_type in CHAIN_RELATION_TYPES
        and (relation.source_code in scope or relation.target_code in scope)
    ]
    chain_relations.sort(key=lambda item: item.weight, reverse=True)
    return chain_relations[:30]


def _fetch_news_items(
    stocks: Sequence[StockItem],
    relations: Sequence[StockRelation],
    days: int,
) -> tuple[List[RawNewsItem], List[str]]:
    notes: List[str] = []
    items: List[RawNewsItem] = []
    if os.getenv("GP_NEWS_ENABLE_AKSHARE", "").strip().lower() in {"1", "true", "yes", "on"}:
        try:
            items.extend(_AkshareStockNewsAdapter().fetch(stocks, relations, days))
            if items:
                notes.append("已通过 AkShare 东方财富个股新闻接口抓取并缓存消息。")
        except Exception as exc:
            notes.append(f"AkShare 新闻抓取不可用，已保留本地演示消息适配器：{str(exc)[:120]}")
    else:
        notes.append("AkShare 新闻抓取未启用；可设置 GP_NEWS_ENABLE_AKSHARE=true 后接入真实个股新闻。")

    demo_items = _DemoNewsAdapter().fetch(stocks, relations, days)
    existing_keys = {_news_key(item) for item in items}
    for item in demo_items:
        if _news_key(item) not in existing_keys:
            items.append(item)
    if demo_items:
        notes.append("已写入本地可复现演示消息；来源为本地演示时不代表实时新闻。")
    return items, notes


class _AkshareStockNewsAdapter:
    source = "AkShare 东方财富个股新闻"

    def fetch(
        self,
        stocks: Sequence[StockItem],
        relations: Sequence[StockRelation],
        days: int,
    ) -> List[RawNewsItem]:
        import akshare as ak

        cutoff = datetime.now() - timedelta(days=days)
        relation_map = _relation_map(relations)
        items: List[RawNewsItem] = []
        for stock in stocks[:8]:
            symbol = stock.code.split(".")[0]
            frame = ak.stock_news_em(symbol=symbol)
            if frame is None or frame.empty:
                continue
            for row in frame.head(8).to_dict("records"):
                title = _coalesce_row(row, ["新闻标题", "标题", "title"])
                if not title:
                    continue
                published_at = _parse_news_time(
                    _coalesce_row(row, ["发布时间", "时间", "datetime", "date"])
                )
                if _parse_dt(published_at) < cutoff:
                    continue
                url = _coalesce_row(row, ["新闻链接", "链接", "url"]) or f"akshare://stock_news_em/{symbol}"
                summary = _coalesce_row(row, ["新闻内容", "内容", "摘要", "summary"]) or title
                items.append(
                    RawNewsItem(
                        title=title,
                        summary=summary,
                        source=self.source,
                        url=url,
                        published_at=published_at,
                        stock_codes=(stock.code,),
                        industries=(stock.industry,),
                        relation_types=tuple(sorted(relation_map.get(stock.code, set()))),
                        sentiment=_infer_sentiment(title + " " + summary),
                    )
                )
        return items


class _DemoNewsAdapter:
    source = "本地演示消息"

    def fetch(
        self,
        stocks: Sequence[StockItem],
        relations: Sequence[StockRelation],
        days: int,
    ) -> List[RawNewsItem]:
        stock_by_code = {stock.code: stock for stock in stocks}
        now = datetime.now()
        items: List[RawNewsItem] = []
        for index, relation in enumerate(relations[:10]):
            left = stock_by_code.get(relation.source_code)
            right = stock_by_code.get(relation.target_code)
            if left is None or right is None:
                continue
            published_at = (now - timedelta(days=min(index * 2 + 1, max(days - 1, 0)))).isoformat(timespec="seconds")
            title, summary, sentiment = _demo_message(left, right, relation)
            items.append(
                RawNewsItem(
                    title=title,
                    summary=summary,
                    source=self.source,
                    url=f"local://news/{relation.relation_type}/{relation.source_code}/{relation.target_code}",
                    published_at=published_at,
                    stock_codes=(left.code, right.code),
                    industries=tuple(sorted({left.industry, right.industry})),
                    relation_types=(relation.relation_type,),
                    sentiment=sentiment,
                )
            )
        return items


def _demo_message(left: StockItem, right: StockItem, relation: StockRelation) -> tuple[str, str, str]:
    if relation.relation_type == "upstream_material":
        return (
            f"{left.name}相关材料需求改善，可能影响{right.name}成本与供给预期",
            f"本地演示消息：{left.name}与{right.name}存在上游材料关系，需结合材料价格、订单和财报验证。",
            "mixed",
        )
    if relation.relation_type == "manufacturing_chain":
        return (
            f"{left.name}与{right.name}制造链订单预期升温",
            "本地演示消息：制造链需求改善通常利好产能利用率，但需要验证交付、毛利率和库存变化。",
            "positive",
        )
    return (
        f"{left.name}与{right.name}供应链景气度出现改善信号",
        "本地演示消息：供应链景气改善可能带来收入弹性，同时需要核验订单兑现和价格传导。",
        "positive",
    )


def _connect() -> sqlite3.Connection:
    CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(CACHE_PATH)
    conn.row_factory = sqlite3.Row
    return conn


def _init_db(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS news_items (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            url TEXT,
            published_at TEXT,
            fetched_at TEXT NOT NULL,
            sentiment TEXT NOT NULL
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS news_entities (
            news_id TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            entity_value TEXT NOT NULL,
            relation_type TEXT,
            PRIMARY KEY (news_id, entity_type, entity_value, relation_type)
        )
        """
    )
    conn.execute("CREATE INDEX IF NOT EXISTS idx_news_published ON news_items(published_at)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_news_entities ON news_entities(entity_type, entity_value)")


def _store_news(conn: sqlite3.Connection, items: Iterable[RawNewsItem]) -> None:
    fetched_at = datetime.now().isoformat(timespec="seconds")
    for item in items:
        news_id = _news_key(item)
        conn.execute(
            """
            INSERT OR REPLACE INTO news_items
            (id, source, title, summary, url, published_at, fetched_at, sentiment)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (news_id, item.source, item.title, item.summary, item.url, item.published_at, fetched_at, item.sentiment),
        )
        conn.execute("DELETE FROM news_entities WHERE news_id = ?", (news_id,))
        for code in item.stock_codes:
            _insert_entity(conn, news_id, "stock", code, None)
        for industry in item.industries:
            _insert_entity(conn, news_id, "industry", industry, None)
        for relation_type in item.relation_types:
            _insert_entity(conn, news_id, "relation_type", relation_type, relation_type)
    conn.commit()


def _insert_entity(
    conn: sqlite3.Connection,
    news_id: str,
    entity_type: str,
    entity_value: str,
    relation_type: str | None,
) -> None:
    conn.execute(
        """
        INSERT OR IGNORE INTO news_entities
        (news_id, entity_type, entity_value, relation_type)
        VALUES (?, ?, ?, ?)
        """,
        (news_id, entity_type, entity_value, relation_type or ""),
    )


def _query_evidence(
    conn: sqlite3.Connection,
    codes: Sequence[str],
    days: int,
    limit: int,
) -> List[NewsEvidence]:
    if not codes:
        return []
    cutoff = (datetime.now() - timedelta(days=days)).isoformat(timespec="seconds")
    placeholders = ",".join("?" for _ in codes)
    rows = conn.execute(
        f"""
        SELECT DISTINCT item.*
        FROM news_items item
        JOIN news_entities entity ON entity.news_id = item.id
        WHERE entity.entity_type = 'stock'
          AND entity.entity_value IN ({placeholders})
          AND COALESCE(item.published_at, item.fetched_at) >= ?
        ORDER BY COALESCE(item.published_at, item.fetched_at) DESC
        LIMIT ?
        """,
        [*codes, cutoff, limit],
    ).fetchall()
    return [_row_to_evidence(conn, row) for row in rows]


def _row_to_evidence(conn: sqlite3.Connection, row: sqlite3.Row) -> NewsEvidence:
    entities = conn.execute(
        "SELECT entity_type, entity_value, relation_type FROM news_entities WHERE news_id = ?",
        (row["id"],),
    ).fetchall()
    return NewsEvidence(
        title=row["title"],
        source=row["source"],
        published_at=row["published_at"],
        url=row["url"],
        stock_codes=sorted({item["entity_value"] for item in entities if item["entity_type"] == "stock"}),
        relation_types=sorted(
            {
                item["entity_value"]
                for item in entities
                if item["entity_type"] == "relation_type"
            }
        ),
        sentiment=row["sentiment"],
    )


def _build_findings(
    scope_codes: Sequence[str],
    relations: Sequence[StockRelation],
    evidence: Sequence[NewsEvidence],
    stock_by_code: dict[str, StockItem],
) -> List[NewsImpactFinding]:
    if not scope_codes:
        return []
    evidence_by_code = {
        code: [item for item in evidence if code in item.stock_codes]
        for code in scope_codes
    }
    findings: List[NewsImpactFinding] = []
    for code in scope_codes[:8]:
        stock = stock_by_code.get(code)
        stock_name = stock.name if stock else code
        related = [
            relation
            for relation in relations
            if relation.source_code == code or relation.target_code == code
        ]
        direct_evidence = evidence_by_code.get(code, [])
        neighbor_codes = {
            relation.target_code if relation.source_code == code else relation.source_code
            for relation in related
        }
        neighbor_evidence = [
            item for item in evidence if any(neighbor in item.stock_codes for neighbor in neighbor_codes)
        ]
        selected = _dedupe_evidence([*direct_evidence, *neighbor_evidence])[:5]
        direction = _direction(selected)
        confidence = _confidence(selected, related)
        chain = _impact_chain(code, stock_name, related, stock_by_code, selected)
        findings.append(
            NewsImpactFinding(
                target=f"{stock_name}（{code}）",
                direction=direction,
                confidence=confidence,
                impact_chain=chain,
                evidence=list(selected),
                pending_checks=_pending_checks(direction),
            )
        )
    return findings


def _impact_chain(
    code: str,
    stock_name: str,
    relations: Sequence[StockRelation],
    stock_by_code: dict[str, StockItem],
    evidence: Sequence[NewsEvidence],
) -> str:
    if not relations:
        return f"{stock_name} 暂无已建模上下游关系，不能从消息推出产业链影响。"
    strongest = max(relations, key=lambda item: item.weight)
    other_code = strongest.target_code if strongest.source_code == code else strongest.source_code
    other = stock_by_code.get(other_code)
    relation_label = _relation_type_label(strongest.relation_type)
    tone = _direction(evidence)
    return f"{other.name if other else other_code} -> {relation_label} -> {stock_name}；当前消息影响判断为{tone}。"


def _direction(evidence: Sequence[NewsEvidence]) -> str:
    if not evidence:
        return "不确定"
    positives = sum(1 for item in evidence if item.sentiment == "positive")
    negatives = sum(1 for item in evidence if item.sentiment == "negative")
    mixed = sum(1 for item in evidence if item.sentiment == "mixed")
    if positives > negatives and positives >= mixed:
        return "利好"
    if negatives > positives:
        return "利空"
    if mixed:
        return "中性"
    return "不确定"


def _confidence(evidence: Sequence[NewsEvidence], relations: Sequence[StockRelation]) -> str:
    if not evidence or not relations:
        return "低"
    if len(evidence) >= 3 and max(relation.weight for relation in relations) >= 0.6:
        return "中"
    return "低"


def _pending_checks(direction: str) -> List[str]:
    checks = ["核验公告和财报是否支持该消息影响。", "结合日线趋势、成交量和估值变化复查。"]
    if direction in {"利好", "中性"}:
        checks.append("关注订单兑现、价格传导和毛利率变化。")
    if direction in {"利空", "中性"}:
        checks.append("关注成本压力、库存和需求下滑风险。")
    return checks


def _dedupe_evidence(evidence: Sequence[NewsEvidence]) -> List[NewsEvidence]:
    seen: set[tuple[str, str]] = set()
    unique: List[NewsEvidence] = []
    for item in evidence:
        key = (item.title, item.source)
        if key in seen:
            continue
        seen.add(key)
        unique.append(item)
    return unique


def _relation_map(relations: Sequence[StockRelation]) -> dict[str, set[str]]:
    result: dict[str, set[str]] = {}
    for relation in relations:
        result.setdefault(relation.source_code, set()).add(relation.relation_type)
        result.setdefault(relation.target_code, set()).add(relation.relation_type)
    return result


def _coalesce_row(row: dict[str, object], keys: Sequence[str]) -> str:
    for key in keys:
        value = row.get(key)
        if value is not None and str(value).strip():
            return str(value).strip()
    return ""


def _parse_news_time(value: str) -> str:
    if not value:
        return datetime.now().isoformat(timespec="seconds")
    raw = str(value).strip()
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%d", "%Y/%m/%d %H:%M:%S", "%Y/%m/%d"):
        try:
            return datetime.strptime(raw, fmt).isoformat(timespec="seconds")
        except ValueError:
            continue
    return raw


def _parse_dt(value: str) -> datetime:
    for fmt in ("%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"):
        try:
            return datetime.strptime(value[:19], fmt)
        except ValueError:
            continue
    return datetime.now()


def _infer_sentiment(text: str) -> str:
    lowered = text.lower()
    if any(word in lowered for word in ["下滑", "亏损", "风险", "承压", "下降", "减产", "调查"]):
        return "negative"
    if any(word in lowered for word in ["增长", "改善", "突破", "订单", "中标", "扩产", "景气", "利好"]):
        return "positive"
    if any(word in lowered for word in ["成本", "涨价", "波动"]):
        return "mixed"
    return "uncertain"


def _news_key(item: RawNewsItem) -> str:
    raw = "|".join([item.source, item.title, item.url or "", item.published_at or ""])
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()[:24]


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


def _relation_type_label(relation_type: str) -> str:
    labels = {
        "supply_chain": "供应链",
        "manufacturing_chain": "制造链",
        "upstream_material": "上游材料",
    }
    return labels.get(relation_type, relation_type)
