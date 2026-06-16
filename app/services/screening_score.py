from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Sequence

from app.schemas import ScreenedStock, StockItem
from app.services.screening_rules import (
    ScreeningRules,
    concept_group_for_stock,
    is_cold_sector,
    load_screening_rules,
    theme_match_for_stock,
)


FACTOR_LABELS = {
    "theme": "主题",
    "fundamental": "基本面",
    "valuation": "估值",
    "size": "规模",
    "risk": "风险",
}


@dataclass(frozen=True)
class ScoreBreakdown:
    score: float
    factor_scores: dict[str, float]
    explanation: str
    concept: str
    theme_category: str | None
    reasons: list[str]


def score_stock(
    stock: StockItem,
    base_reasons: Sequence[str] | None = None,
    *,
    rules: ScreeningRules | None = None,
) -> ScreenedStock:
    active_rules = rules or load_screening_rules()
    breakdown = score_breakdown(stock, base_reasons, rules=active_rules)
    return ScreenedStock(
        stock=stock,
        score=round(breakdown.score, 6),
        reasons=breakdown.reasons,
        factor_scores=breakdown.factor_scores,
        score_explanation=breakdown.explanation,
        concept=breakdown.concept,
        theme_category=breakdown.theme_category,
    )


def score_breakdown(
    stock: StockItem,
    base_reasons: Sequence[str] | None = None,
    *,
    rules: ScreeningRules | None = None,
) -> ScoreBreakdown:
    active_rules = rules or load_screening_rules()
    theme = theme_match_for_stock(stock, active_rules)
    cold = is_cold_sector(stock.industry, active_rules)
    factor_scores = {
        "theme": _theme_score(theme),
        "fundamental": _fundamental_score(stock),
        "valuation": _valuation_score(stock),
        "size": _size_score(stock),
        "risk": _risk_score(stock, cold),
    }
    weighted = sum(active_rules.factor_weights.get(key, 0.0) * value for key, value in factor_scores.items())
    score = max(0.0, min(active_rules.score_scale, weighted * active_rules.score_scale))
    reasons = list(base_reasons or [])
    reasons.extend(_factor_reasons(theme, cold, factor_scores))
    explanation = _explain(stock, theme.label if theme else None, cold, factor_scores)
    return ScoreBreakdown(
        score=score,
        factor_scores={key: round(value, 4) for key, value in factor_scores.items()},
        explanation=explanation,
        concept=concept_group_for_stock(stock, active_rules),
        theme_category=theme.key if theme else None,
        reasons=reasons,
    )


def _theme_score(theme) -> float:
    return _clamp(float(theme.score)) if theme is not None and theme.score > 0 else 0.35


def _fundamental_score(stock: StockItem) -> float:
    roe_percent = _as_percent(stock.roe)
    if roe_percent is None:
        roe_score = 0.5
    elif roe_percent >= 15:
        roe_score = 1.0
    elif roe_percent >= 8:
        roe_score = 0.72 + (roe_percent - 8) / 7 * 0.18
    elif roe_percent >= 0:
        roe_score = 0.42 + roe_percent / 8 * 0.25
    else:
        roe_score = 0.2

    dividend = _as_percent(stock.dividend_yield)
    dividend_bonus = 0.0 if dividend is None else min(max(dividend, 0.0) / 6.0, 1.0) * 0.12
    return _clamp(roe_score + dividend_bonus)


def _valuation_score(stock: StockItem) -> float:
    return _clamp((_pe_score(stock.pe) + _pb_score(stock.pb)) / 2)


def _pe_score(value: float | None) -> float:
    if value is None:
        return 0.52
    if value <= 0:
        return 0.25
    if value <= 15:
        return 1.0
    if value <= 30:
        return 0.72
    if value <= 60:
        return 0.45
    return 0.28


def _pb_score(value: float | None) -> float:
    if value is None:
        return 0.52
    if value <= 0:
        return 0.25
    if value <= 1.5:
        return 1.0
    if value <= 3:
        return 0.74
    if value <= 6:
        return 0.5
    if value <= 10:
        return 0.34
    return 0.22


def _size_score(stock: StockItem) -> float:
    market_cap = stock.market_cap_billion
    if market_cap is None:
        return 0.55
    if market_cap < 20:
        return 0.36
    if market_cap < 100:
        return 0.68
    if market_cap < 500:
        return 0.88
    if market_cap < 2000:
        return 0.78
    return 0.62


def _risk_score(stock: StockItem, cold: bool) -> float:
    if stock.is_st:
        return 0.05
    if cold:
        return 0.36
    return 1.0


def _factor_reasons(theme, cold: bool, factor_scores: dict[str, float]) -> list[str]:
    reasons: list[str] = []
    if theme is not None:
        reasons.append(f"主题:{theme.label}")
    if factor_scores["valuation"] >= 0.74:
        reasons.append("估值较低")
    elif factor_scores["valuation"] <= 0.38:
        reasons.append("估值偏高")
    if factor_scores["fundamental"] >= 0.72:
        reasons.append("基本面较强")
    if cold:
        reasons.append("低热度降权")
    return reasons


def _explain(stock: StockItem, theme_label: str | None, cold: bool, factor_scores: dict[str, float]) -> str:
    parts: list[str] = []
    if theme_label:
        parts.append(f"主题命中{theme_label}")
    else:
        parts.append("主题热度一般")
    parts.append(f"估值{_tier_word(factor_scores['valuation'])}")
    parts.append(f"基本面{_tier_word(factor_scores['fundamental'])}")
    if stock.market_cap_billion is None:
        parts.append("市值缺失按中性处理")
    else:
        parts.append(f"市值规模{_tier_word(factor_scores['size'])}")
    parts.append("银行/基建等低热度方向已降权" if cold else "风险惩罚低")
    return "；".join(parts) + "。"


def _tier_word(value: float) -> str:
    if value >= 0.72:
        return "强"
    if value >= 0.5:
        return "中性"
    return "偏弱"


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


def _clamp(value: float) -> float:
    return max(0.0, min(1.0, value))
