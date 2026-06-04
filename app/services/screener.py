from typing import Dict, List, Optional

from app.providers.base import StockProvider
from app.schemas import (
    ScreenCriteria,
    ScreenResult,
    ScreenedStock,
    SectorScreenGroup,
    SectorScreenRequest,
    SectorScreenResult,
    StockItem,
)


def screening_universe(provider: StockProvider) -> tuple[List[StockItem], List[str]]:
    return provider.list_stocks_for_screen()


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


def _screened_stocks(universe: List[StockItem], criteria: ScreenCriteria) -> List[ScreenedStock]:
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

    return screened


def screen_stocks(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: Optional[List[str]] = None,
) -> ScreenResult:
    screened = _screened_stocks(universe, criteria)
    items = screened[: criteria.limit]
    return ScreenResult(total=len(screened), returned=len(items), items=items, notes=notes or [])


def screen_stocks_by_sector(
    universe: List[StockItem],
    request: SectorScreenRequest,
    notes: Optional[List[str]] = None,
) -> SectorScreenResult:
    screened = _screened_stocks(universe, request.criteria)
    by_sector: Dict[str, List[ScreenedStock]] = {}
    for item in screened:
        sector = (item.stock.industry or "未知板块").strip() or "未知板块"
        by_sector.setdefault(sector, []).append(item)

    groups: List[SectorScreenGroup] = []
    for sector, items in by_sector.items():
        if len(items) < request.min_sector_candidates:
            continue
        selected = items[: request.per_sector_limit]
        average_score = sum(item.score for item in selected) / len(selected)
        groups.append(
            SectorScreenGroup(
                sector=sector,
                total=len(items),
                returned=len(selected),
                average_score=average_score,
                items=selected,
            )
        )

    groups.sort(key=lambda group: (-group.average_score, -group.total, group.sector))
    groups = groups[: request.max_sectors]
    returned = sum(group.returned for group in groups)
    result_notes = [*(notes or []), "按股票行业字段作为板块分组。"]
    if request.criteria.industry:
        result_notes.append(f"已限制行业：{request.criteria.industry}")

    return SectorScreenResult(
        total=len(screened),
        returned=returned,
        sector_count=len(groups),
        groups=groups,
        notes=result_notes,
    )
