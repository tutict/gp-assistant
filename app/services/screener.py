from typing import List, Optional

from app.schemas import ScreenCriteria, ScreenResult, ScreenedStock, StockItem


def _matches(stock: StockItem, criteria: ScreenCriteria) -> Optional[List[str]]:
    reasons: List[str] = []
    if not criteria.include_st and stock.is_st:
        return None

    if criteria.industry and stock.industry.lower() != criteria.industry.lower():
        return None

    if criteria.min_roe is not None:
        if stock.roe is None or stock.roe < criteria.min_roe:
            return None
        reasons.append("roe_ok")

    if criteria.max_pe is not None:
        if stock.pe is None or stock.pe > criteria.max_pe:
            return None
        reasons.append("pe_ok")

    if criteria.max_pb is not None:
        if stock.pb is None or stock.pb > criteria.max_pb:
            return None
        reasons.append("pb_ok")

    if criteria.min_market_cap_billion is not None:
        if stock.market_cap_billion is None or stock.market_cap_billion < criteria.min_market_cap_billion:
            return None
        reasons.append("mcap_ok")

    return reasons


def _score(stock: StockItem, reasons: List[str]) -> float:
    score = float(len(reasons))
    if stock.pe:
        score += max(0.0, 10 / stock.pe)
    if stock.pb:
        score += max(0.0, 2 / stock.pb)
    if stock.roe:
        score += stock.roe * 2
    if stock.dividend_yield:
        score += min(stock.dividend_yield * 10, 1.0)
    return score


def screen_stocks(universe: List[StockItem], criteria: ScreenCriteria) -> ScreenResult:
    screened: List[ScreenedStock] = []
    for stock in universe:
        reasons = _matches(stock, criteria)
        if reasons is None:
            continue
        score = _score(stock, reasons)
        screened.append(ScreenedStock(stock=stock, score=score, reasons=reasons))

    reverse = criteria.sort_dir.lower() != "asc"
    if criteria.sort_by == "price":
        screened.sort(key=lambda x: x.stock.price, reverse=reverse)
    elif criteria.sort_by == "pe":
        screened.sort(key=lambda x: (x.stock.pe or 0), reverse=reverse)
    elif criteria.sort_by == "pb":
        screened.sort(key=lambda x: (x.stock.pb or 0), reverse=reverse)
    else:
        screened.sort(key=lambda x: x.score, reverse=reverse)

    items = screened[: criteria.limit]
    return ScreenResult(total=len(screened), returned=len(items), items=items)
