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


HOT_SECTOR_KEYWORDS: tuple[tuple[str, float], ...] = (
    ("半导体", 0.55),
    ("芯片", 0.55),
    ("算力", 0.5),
    ("人工智能", 0.5),
    ("ai", 0.5),
    ("机器人", 0.46),
    ("软件", 0.44),
    ("通信", 0.42),
    ("科技", 0.42),
    ("电子", 0.38),
    ("新能源", 0.46),
    ("电池", 0.44),
    ("储能", 0.42),
    ("光伏", 0.4),
    ("电力", 0.34),
    ("能源", 0.34),
    ("油气", 0.28),
    ("煤炭", 0.24),
)

HOT_SECTOR_CATEGORIES: tuple[tuple[str, tuple[str, ...]], ...] = (
    (
        "preferred",
        ("多氟多", "氟化工", "氟材料", "锂电材料", "电解液", "六氟磷酸锂", "新能材"),
    ),
    (
        "tech",
        ("半导体", "芯片", "算力", "人工智能", "ai", "机器人", "软件", "通信", "科技", "电子"),
    ),
    ("energy", ("新能源", "电池", "储能", "光伏", "电力", "能源", "油气", "煤炭")),
)

HOT_STOCK_CODES = {"002407.SZ"}
HOT_STOCK_NAME_KEYWORDS = ("多氟多",)
HOT_SECTOR_PROMOTION_ORDER = ("preferred", "tech", "energy")
COLD_SECTOR_KEYWORDS = (
    "银行",
    "基建",
    "建筑",
    "建筑装饰",
    "工程建设",
    "基础建设",
    "水泥",
    "铁路",
    "公路",
)


def screening_universe(provider: StockProvider) -> tuple[List[StockItem], List[str]]:
    return provider.list_stocks_for_screen()


def _matches(stock: StockItem, criteria: ScreenCriteria) -> Optional[List[str]]:
    reasons: List[str] = []
    if not criteria.include_st and stock.is_st:
        return None

    if criteria.industry and not _industry_matches(stock.industry, criteria.industry):
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


def _industry_matches(stock_industry: str, selected_industry: str) -> bool:
    stock_value = (stock_industry or "").strip().lower()
    selected_value = (selected_industry or "").strip().lower()
    if not selected_value:
        return True
    if not stock_value:
        return False
    return stock_value == selected_value or selected_value in stock_value or stock_value in selected_value


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
    score += _hot_sector_bonus(stock.industry)
    score += _preferred_hot_stock_bonus(stock)
    score -= _cold_sector_penalty(stock.industry)
    return score


def _hot_sector_bonus(industry: str) -> float:
    normalized = (industry or "").strip().lower()
    if not normalized:
        return 0.0
    return max((weight for keyword, weight in HOT_SECTOR_KEYWORDS if keyword in normalized), default=0.0)


def _preferred_hot_stock_bonus(stock: StockItem) -> float:
    if stock.code in HOT_STOCK_CODES:
        return 1.2
    normalized_name = (stock.name or "").strip().lower()
    if any(keyword in normalized_name for keyword in HOT_STOCK_NAME_KEYWORDS):
        return 1.0
    return 0.0


def _cold_sector_penalty(industry: str) -> float:
    return 1.1 if _is_cold_sector(industry) else 0.0


def _is_cold_sector(industry: str) -> bool:
    normalized = (industry or "").strip().lower()
    return bool(normalized) and any(keyword in normalized for keyword in COLD_SECTOR_KEYWORDS)


def _hot_pick_category(stock: StockItem) -> Optional[str]:
    if _preferred_hot_stock_bonus(stock) > 0:
        return "preferred"
    return _hot_sector_category(f"{stock.name} {stock.industry}")


def _hot_sector_category(text: str) -> Optional[str]:
    normalized = (text or "").strip().lower()
    if not normalized:
        return None
    for category, keywords in HOT_SECTOR_CATEGORIES:
        if any(keyword in normalized for keyword in keywords):
            return category
    return None


def _should_promote_hot_sectors(criteria: ScreenCriteria) -> bool:
    return (
        not (criteria.industry or "").strip()
        and (criteria.sort_by or "score").strip().lower() == "score"
        and (criteria.sort_dir or "desc").strip().lower() != "asc"
    )


def _promote_hot_sector_items(
    screened: List[ScreenedStock],
    criteria: ScreenCriteria,
) -> tuple[List[ScreenedStock], bool]:
    limit = criteria.limit
    if not _should_promote_hot_sectors(criteria) or limit <= 0:
        return screened[:limit], False

    promoted: List[ScreenedStock] = []
    used_codes: set[str] = set()
    changed = False
    for category in HOT_SECTOR_PROMOTION_ORDER:
        candidate = next(
            (
                item
                for item in screened
                if item.stock.code not in used_codes and _hot_pick_category(item.stock) == category
            ),
            None,
        )
        if candidate is not None:
            promoted.append(candidate)
            used_codes.add(candidate.stock.code)
            changed = True
            if len(promoted) >= limit:
                return promoted, changed

    for item in screened:
        if item.stock.code in used_codes:
            continue
        if _is_cold_sector(item.stock.industry):
            changed = True
            continue
        promoted.append(item)
        if len(promoted) >= limit:
            return promoted, changed

    for item in screened:
        if item.stock.code in used_codes or any(existing.stock.code == item.stock.code for existing in promoted):
            continue
        promoted.append(item)
        if len(promoted) >= limit:
            break
    return promoted, changed


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
        screened.sort(key=lambda x: _optional_sort_key(x.stock.pe, reverse))
    elif criteria.sort_by == "pb":
        screened.sort(key=lambda x: _optional_sort_key(x.stock.pb, reverse))
    else:
        screened.sort(key=lambda x: x.score, reverse=reverse)

    return screened


def _optional_sort_key(value: Optional[float], descending: bool) -> tuple[int, float]:
    if value is None:
        return (1, 0.0)
    return (0, -value if descending else value)


def screen_stocks(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: Optional[List[str]] = None,
) -> ScreenResult:
    screened = _screened_stocks(universe, criteria)
    items, promoted = _promote_hot_sector_items(screened, criteria)
    result_notes = [*(notes or [])]
    if promoted:
        result_notes.append("已优先推送多氟多等新能材、科技与能源热门候选，并降低银行/基建优先级。")
    return ScreenResult(total=len(screened), returned=len(items), items=items, notes=result_notes)


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

    groups.sort(key=lambda group: (_is_cold_sector(group.sector), -group.average_score, -group.total, group.sector))
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
