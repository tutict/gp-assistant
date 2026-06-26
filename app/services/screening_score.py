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
    "theme": "\u4e3b\u9898",
    "fundamental": "\u57fa\u672c\u9762",
    "valuation": "\u4f30\u503c",
    "market_heat": "\u8f6e\u52a8\u70ed\u5ea6",
    "size": "\u89c4\u6a21",
    "risk": "\u98ce\u9669",
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
    score_profile: str = "quality",
) -> ScreenedStock:
    active_rules = rules or load_screening_rules()
    breakdown = score_breakdown(stock, base_reasons, rules=active_rules, score_profile=score_profile)
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
    score_profile: str = "quality",
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
    if _is_rotation_profile(score_profile):
        factor_scores["market_heat"] = _market_heat_score(stock)
    weighted = _weighted_score(factor_scores, active_rules, score_profile)
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
    quality_bonus = 0.0
    if stock.deducted_net_profit_billion is not None and stock.deducted_net_profit_billion > 0:
        quality_bonus += 0.08
    deducted_growth = _as_percent(stock.deducted_net_profit_growth_rate)
    if deducted_growth is not None and deducted_growth >= 10:
        quality_bonus += 0.08
    return _clamp(roe_score + dividend_bonus + quality_bonus)


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


def _is_rotation_profile(score_profile: str) -> bool:
    return (score_profile or "").strip().lower() == "rotation"


def _weighted_score(factor_scores: dict[str, float], rules: ScreeningRules, score_profile: str) -> float:
    if _is_rotation_profile(score_profile):
        weights = {
            "theme": 0.20,
            "fundamental": 0.20,
            "valuation": 0.20,
            "market_heat": 0.22,
            "size": 0.08,
            "risk": 0.10,
        }
    else:
        weights = rules.factor_weights
    return sum(float(weights.get(key, 0.0)) * value for key, value in factor_scores.items())


def _market_heat_score(stock: StockItem) -> float:
    change_pct = _as_percent(stock.change_pct) or 0.0
    if change_pct >= 0:
        positive_momentum = _clamp(min(change_pct, 10.0) / 10.0)
    else:
        positive_momentum = _clamp(0.45 + max(change_pct, -10.0) / 20.0)

    raw_volume_ratio = _finite_or_none(stock.volume_ratio)
    volume_ratio = (
        _clamp((min(raw_volume_ratio, 3.0) - 1.0) / 2.0)
        if raw_volume_ratio is not None and raw_volume_ratio > 0
        else 0.45
    )

    turnover_percent = _as_percent(stock.turnover_rate)
    turnover = _clamp(min(max(turnover_percent, 0.0), 8.0) / 8.0) if turnover_percent is not None else 0.45

    amount_yuan = _finite_or_none(stock.amount)
    amount = _clamp(min(amount_yuan / 1_000_000_000.0, 1.0)) if amount_yuan is not None and amount_yuan > 0 else 0.45
    return _clamp(positive_momentum * 0.46 + volume_ratio * 0.24 + turnover * 0.18 + amount * 0.12)

def _factor_reasons(theme, cold: bool, factor_scores: dict[str, float]) -> list[str]:
    reasons: list[str] = []
    if theme is not None:
        reasons.append(f"\u4e3b\u9898:{theme.label}")
    if factor_scores["valuation"] >= 0.74:
        reasons.append("\u4f30\u503c\u8f83\u4f4e")
    elif factor_scores["valuation"] <= 0.38:
        reasons.append("\u4f30\u503c\u504f\u9ad8")
    if factor_scores["fundamental"] >= 0.72:
        reasons.append("\u57fa\u672c\u9762\u8f83\u5f3a")
    if factor_scores.get("market_heat", 0.0) >= 0.72:
        reasons.append("\u8f6e\u52a8\u70ed\u5ea6\u8f83\u9ad8")
    if cold:
        reasons.append("\u4f4e\u70ed\u5ea6\u964d\u6743")
    return reasons


def _explain(stock: StockItem, theme_label: str | None, cold: bool, factor_scores: dict[str, float]) -> str:
    parts: list[str] = []
    if theme_label:
        parts.append(f"\u4e3b\u9898\u547d\u4e2d{theme_label}")
    else:
        parts.append("\u4e3b\u9898\u70ed\u5ea6\u4e00\u822c")
    parts.append(f"\u4f30\u503c{_tier_word(factor_scores['valuation'])}")
    parts.append(f"\u57fa\u672c\u9762{_tier_word(factor_scores['fundamental'])}")
    if stock.market_cap_billion is None:
        parts.append("\u5e02\u503c\u7f3a\u5931\u6309\u4e2d\u6027\u5904\u7406")
    else:
        parts.append(f"\u5e02\u503c\u89c4\u6a21{_tier_word(factor_scores['size'])}")
    if "market_heat" in factor_scores:
        parts.append(f"\u8f6e\u52a8\u70ed\u5ea6{_tier_word(factor_scores['market_heat'])}")
    parts.append("\u94f6\u884c/\u57fa\u5efa\u7b49\u4f4e\u70ed\u5ea6\u65b9\u5411\u5df2\u964d\u6743" if cold else "\u98ce\u9669\u60e9\u7f5a\u4f4e")
    return "\uff1b".join(parts) + "\u3002"


def _tier_word(value: float) -> str:
    if value >= 0.72:
        return "\u5f3a"
    if value >= 0.5:
        return "\u4e2d\u6027"
    return "\u504f\u5f31"


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


def _finite_or_none(value: float | None) -> float | None:
    if value is None:
        return None
    try:
        numeric = float(value)
    except (TypeError, ValueError):
        return None
    return numeric if math.isfinite(numeric) else None


def _clamp(value: float) -> float:
    return max(0.0, min(1.0, value))
