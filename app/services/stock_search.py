from __future__ import annotations

from typing import Iterable, List

from app.schemas import StockItem


def search_stock_items(stocks: Iterable[StockItem], query: str, limit: int = 3) -> List[StockItem]:
    normalized = _normalize_query(query)
    if not normalized["raw"]:
        return []

    limit = max(1, min(limit, 20))
    scored: list[tuple[int, str, StockItem]] = []
    for stock in stocks:
        score = _score_stock(stock, normalized)
        if score > 0:
            scored.append((score, stock.code, stock))

    scored.sort(key=lambda item: (-item[0], item[1]))
    return [stock for _, _, stock in scored[:limit]]


def _normalize_query(query: str) -> dict[str, str]:
    raw = str(query or "").strip()
    upper = raw.upper().replace(" ", "")
    return {
        "raw": raw,
        "text": raw.casefold(),
        "compact_text": _compact_text(raw),
        "code": upper.replace(".", ""),
        "digits": "".join(char for char in upper if char.isdigit()),
    }


def _score_stock(stock: StockItem, query: dict[str, str]) -> int:
    code = stock.code.upper()
    compact_code = code.replace(".", "")
    digits = "".join(char for char in code if char.isdigit())
    name = (stock.name or "").casefold()
    compact_name = _compact_text(stock.name or "")
    industry = (stock.industry or "").casefold()
    compact_industry = _compact_text(stock.industry or "")

    query_code = query["code"]
    query_digits = query["digits"]
    query_text = query["text"]
    compact_query = query["compact_text"]

    if query_code and query_code in {code, compact_code}:
        return 120
    if query_digits and query_digits == digits:
        return 115
    if query_digits and digits.startswith(query_digits):
        return 100
    if query_code and compact_code.startswith(query_code):
        return 95
    if query_digits and query_digits in digits:
        return 80
    if query_code and query_code in compact_code:
        return 75

    if compact_query and compact_query == compact_name:
        return 72
    if compact_query and compact_name.startswith(compact_query):
        return 68
    if query_text and name.startswith(query_text):
        return 65
    if compact_query and compact_query in compact_name:
        return 60
    if query_text and query_text in name:
        return 55
    if compact_query and _is_ordered_subsequence(compact_query, compact_name):
        return 45
    if compact_query and compact_query in compact_industry:
        return 35
    if query_text and query_text in industry:
        return 32
    return 0


def _compact_text(value: str) -> str:
    return "".join(str(value or "").casefold().split())


def _is_ordered_subsequence(query: str, target: str) -> bool:
    if not query or not target:
        return False
    position = 0
    for char in target:
        if char == query[position]:
            position += 1
            if position == len(query):
                return True
    return False