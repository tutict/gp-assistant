from __future__ import annotations

import math
from typing import List, Optional

from app.providers.base import StockProvider
from app.schemas import (
    ScreenCriteria,
    ScreenResult,
    ScreenResultGroup,
    ScreenedStock,
    SectorScreenGroup,
    SectorScreenRequest,
    SectorScreenResult,
    StockItem,
)
from app.services.institution_signals import apply_institution_buy_ratio_filter
from app.services.screening_rules import (
    ScreeningRules,
    concept_group_for_stock,
    concept_rank,
    is_cold_sector,
    load_screening_rules,
)
from app.services.screening_score import score_stock


def screening_universe(provider: StockProvider) -> tuple[List[StockItem], List[str]]:
    return provider.list_stocks_for_screen()


def screen_stocks(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: Optional[List[str]] = None,
) -> ScreenResult:
    rules = load_screening_rules()
    result_notes = [*(notes or [])]
    _append_deducted_profit_rule_note(universe, criteria, result_notes)
    screened = _screened_stocks(universe, criteria, result_notes, rules=rules)
    items, promoted = _primary_items(screened, criteria, rules)
    groups = _screen_result_groups(screened, rules)
    if promoted:
        result_notes.append("已按共享筛选规则均衡评估主题、基本面、估值、规模和风险，并优先展示热门主题候选。")
    result_notes.append(
        f"普通筛选已拆成热门股和综合分大于 {rules.potential_score_threshold:g} 的潜力股，每类最多 {rules.group_limit} 只。"
    )
    return ScreenResult(total=len(screened), returned=len(items), items=items, groups=groups, notes=result_notes)


def screen_stocks_by_sector(
    universe: List[StockItem],
    request: SectorScreenRequest,
    notes: Optional[List[str]] = None,
) -> SectorScreenResult:
    rules = load_screening_rules()
    result_notes = [*(notes or []), "按共享筛选规则中的股票名称和行业关键词映射概念分组；未命中概念时归入其他概念。"]
    _append_deducted_profit_rule_note(universe, request.criteria, result_notes)
    screened = _screened_stocks(universe, request.criteria, result_notes, rules=rules)
    by_concept: dict[str, List[ScreenedStock]] = {}
    for item in screened:
        concept = item.concept or concept_group_for_stock(item.stock, rules)
        by_concept.setdefault(concept, []).append(item)

    groups: List[SectorScreenGroup] = []
    for concept, items in by_concept.items():
        if len(items) < request.min_sector_candidates:
            continue
        selected = items[: request.per_sector_limit]
        average_score = sum(item.score for item in selected) / len(selected)
        groups.append(
            SectorScreenGroup(
                sector=concept,
                total=len(items),
                returned=len(selected),
                average_score=average_score,
                items=selected,
            )
        )

    groups.sort(key=lambda group: (concept_rank(group.sector, rules), -group.average_score, -group.total, group.sector))
    groups = groups[: request.max_sectors]
    returned = sum(group.returned for group in groups)
    if request.criteria.industry:
        result_notes.append(f"已限制行业：{request.criteria.industry}")
    result_notes.append("分组口径为概念筛选，概念和关键词来自共享筛选规则。")

    return SectorScreenResult(
        total=len(screened),
        returned=returned,
        sector_count=len(groups),
        groups=groups,
        notes=result_notes,
    )


def _screened_stocks(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: list[str],
    *,
    rules: ScreeningRules,
) -> List[ScreenedStock]:
    screened: List[ScreenedStock] = []
    for stock in universe:
        reasons = _matches(stock, criteria)
        if reasons is None:
            continue
        screened.append(score_stock(stock, reasons, rules=rules))

    _sort_screened(screened, criteria)
    return apply_institution_buy_ratio_filter(screened, criteria, notes)


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

    if criteria.min_deducted_net_profit_billion is not None:
        if (
            stock.deducted_net_profit_billion is None
            or stock.deducted_net_profit_billion <= criteria.min_deducted_net_profit_billion
        ):
            return None
        reasons.append("deducted_net_profit_ok")

    if criteria.min_deducted_net_profit_margin is not None:
        margin = _as_percent(stock.deducted_net_profit_margin)
        if margin is None or margin <= criteria.min_deducted_net_profit_margin:
            return None
        reasons.append("deducted_net_profit_margin_ok")

    if criteria.min_deducted_net_profit_growth_rate is not None:
        growth = _as_percent(stock.deducted_net_profit_growth_rate)
        if growth is None or growth <= criteria.min_deducted_net_profit_growth_rate:
            return None
        reasons.append("deducted_net_profit_growth_rate_ok")

    return reasons


def _append_deducted_profit_rule_note(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: list[str],
) -> None:
    if (
        criteria.min_deducted_net_profit_billion is None
        and criteria.min_deducted_net_profit_margin is None
        and criteria.min_deducted_net_profit_growth_rate is None
    ):
        return
    with_metrics = sum(
        1
        for stock in universe
        if _has_deducted_profit_rule_metrics(stock, criteria)
    )
    if with_metrics < len(universe):
        notes.append(
            f"扣非净利润规则已启用；当前股票池 {with_metrics}/{len(universe)} 只股票带扣非财务字段，缺字段股票按不达标处理。"
        )


def _has_deducted_profit_rule_metrics(stock: StockItem, criteria: ScreenCriteria) -> bool:
    if criteria.min_deducted_net_profit_billion is not None and stock.deducted_net_profit_billion is None:
        return False
    if criteria.min_deducted_net_profit_margin is not None and stock.deducted_net_profit_margin is None:
        return False
    if (
        criteria.min_deducted_net_profit_growth_rate is not None
        and stock.deducted_net_profit_growth_rate is None
    ):
        return False
    return True


def _industry_matches(stock_industry: str, selected_industry: str) -> bool:
    stock_value = (stock_industry or "").strip().lower()
    selected_value = (selected_industry or "").strip().lower()
    if not selected_value:
        return True
    if not stock_value:
        return False
    return stock_value == selected_value or selected_value in stock_value or stock_value in selected_value


def _sort_screened(screened: List[ScreenedStock], criteria: ScreenCriteria) -> None:
    reverse = (criteria.sort_dir or "desc").lower() != "asc"
    sort_by = (criteria.sort_by or "score").lower()
    if sort_by == "price":
        screened.sort(key=lambda item: item.stock.price, reverse=reverse)
    elif sort_by == "pe":
        screened.sort(key=lambda item: _optional_sort_key(item.stock.pe, reverse))
    elif sort_by == "pb":
        screened.sort(key=lambda item: _optional_sort_key(item.stock.pb, reverse))
    else:
        screened.sort(
            key=lambda item: (
                item.score,
                item.factor_scores.get("theme", 0.0),
                item.factor_scores.get("fundamental", 0.0),
                item.factor_scores.get("valuation", 0.0),
            ),
            reverse=reverse,
        )


def _optional_sort_key(value: Optional[float], descending: bool) -> tuple[int, float]:
    if value is None:
        return (1, 0.0)
    return (0, -value if descending else value)


def _as_percent(value: float | None) -> float | None:
    if value is None:
        return None
    try:
        numeric = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(numeric):
        return None
    return numeric * 100 if -1 <= numeric <= 1 else numeric


def _primary_items(
    screened: List[ScreenedStock],
    criteria: ScreenCriteria,
    rules: ScreeningRules,
) -> tuple[List[ScreenedStock], bool]:
    limit = criteria.limit
    if not _should_promote_themes(criteria) or limit <= 0:
        return screened[:limit], False

    selected: List[ScreenedStock] = []
    used_codes: set[str] = set()
    for category in rules.theme_promotion_order:
        candidate = _first_by_theme(screened, category, used_codes)
        if candidate is not None:
            _append_selected(selected, used_codes, candidate)
            if len(selected) >= limit:
                return selected, True

    for category in rules.theme_fill_order:
        for item in screened:
            if item.stock.code in used_codes or item.theme_category != category:
                continue
            _append_selected(selected, used_codes, item)
            if len(selected) >= limit:
                return selected, True

    for item in screened:
        if item.stock.code in used_codes or is_cold_sector(item.stock.industry, rules):
            continue
        _append_selected(selected, used_codes, item)
        if len(selected) >= limit:
            return selected, True

    for item in screened:
        if item.stock.code in used_codes:
            continue
        _append_selected(selected, used_codes, item)
        if len(selected) >= limit:
            break
    original_codes = [item.stock.code for item in screened[:limit]]
    selected_codes = [item.stock.code for item in selected]
    return selected, selected_codes != original_codes


def _should_promote_themes(criteria: ScreenCriteria) -> bool:
    return (
        not (criteria.industry or "").strip()
        and (criteria.sort_by or "score").strip().lower() == "score"
        and (criteria.sort_dir or "desc").strip().lower() != "asc"
    )


def _first_by_theme(
    screened: List[ScreenedStock],
    category: str,
    used_codes: set[str],
) -> ScreenedStock | None:
    return next((item for item in screened if item.stock.code not in used_codes and item.theme_category == category), None)


def _append_selected(selected: List[ScreenedStock], used_codes: set[str], item: ScreenedStock) -> None:
    selected.append(item)
    used_codes.add(item.stock.code)


def _screen_result_groups(screened: List[ScreenedStock], rules: ScreeningRules) -> List[ScreenResultGroup]:
    hot_items = _hot_group_items(screened, rules.group_limit, rules)
    hot_codes = {item.stock.code for item in hot_items}
    potential_items = _potential_group_items(screened, rules.group_limit, hot_codes, rules.potential_score_threshold)
    return [
        ScreenResultGroup(
            key="hot",
            title="热门股",
            description="AI上下游、芯片、算力、能源、新材料、游戏等热门方向，按综合分和主题优先级展示。",
            total=sum(1 for item in screened if item.theme_category),
            returned=len(hot_items),
            items=hot_items,
        ),
        ScreenResultGroup(
            key="potential",
            title="潜力股",
            description=f"综合分大于 {rules.potential_score_threshold:g} 的候选，已尽量避免与热门股重复。",
            total=sum(1 for item in screened if item.score > rules.potential_score_threshold),
            returned=len(potential_items),
            items=potential_items,
        ),
    ]


def _hot_group_items(screened: List[ScreenedStock], limit: int, rules: ScreeningRules) -> List[ScreenedStock]:
    selected: List[ScreenedStock] = []
    used_codes: set[str] = set()
    by_score = sorted(screened, key=lambda item: item.score, reverse=True)
    for category in rules.theme_promotion_order:
        candidate = _first_by_theme(by_score, category, used_codes)
        if candidate is not None:
            _append_selected(selected, used_codes, candidate)
            if len(selected) >= limit:
                return selected
    for category in rules.theme_fill_order:
        for item in by_score:
            if item.stock.code in used_codes or item.theme_category != category:
                continue
            _append_selected(selected, used_codes, item)
            if len(selected) >= limit:
                return selected
    return selected


def _potential_group_items(
    screened: List[ScreenedStock],
    limit: int,
    exclude_codes: set[str],
    threshold: float,
) -> List[ScreenedStock]:
    selected: List[ScreenedStock] = []
    for item in sorted(screened, key=lambda candidate: candidate.score, reverse=True):
        if item.stock.code in exclude_codes:
            continue
        if item.score <= threshold:
            continue
        selected.append(item)
        if len(selected) >= limit:
            break
    return selected
