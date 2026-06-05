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
        "text": raw.lower(),
        "code": upper.replace(".", ""),
        "digits": "".join(char for char in upper if char.isdigit()),
    }


def _score_stock(stock: StockItem, query: dict[str, str]) -> int:
    code = stock.code.upper()
    compact_code = code.replace(".", "")
    digits = "".join(char for char in code if char.isdigit())
    name = (stock.name or "").lower()
    industry = (stock.industry or "").lower()

    query_code = query["code"]
    query_digits = query["digits"]
    query_text = query["text"]

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
    if query_text and name.startswith(query_text):
        return 65
    if query_text and query_text in name:
        return 55
    if query_text and query_text in industry:
        return 35
    return 0
