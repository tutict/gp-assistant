from typing import Any, Dict, List, Optional

import numpy as np
import pandas as pd

from app.providers.base import StockProvider
from app.schemas import (
    ScreenCriteria,
    StockItem,
    TrendIndicatorPoint,
    TrendIndicatorRequest,
    TrendIndicatorResult,
    TrendIndicatorSignal,
    TrendScreenRequest,
    TrendScreenResult,
    TrendStockSignal,
)
from app.services.screener import screen_stocks, screening_universe


QUANT_SCORE_MAX = 90
TREND_NOTES = [
    "WINNER(C) 需要筹码分布数据，当前未纳入；量化分按 90 分制计算。",
    "SWS 使用公式中的成交量/流通股本项作为 DMA 百分比系数，并限制在 1%-100%。",
]


def analyze_trend(provider: StockProvider, request: TrendIndicatorRequest) -> TrendIndicatorResult:
    stock = provider.get_stock(request.code)
    history = provider.get_history(stock.code, request.start_date, request.end_date)
    frame = _prepare_history(history, stock)
    if frame.empty:
        raise ValueError(f"{stock.code} 没有可用历史行情")

    computed = _compute_indicator_frame(frame, stock)
    signal = _latest_signal(stock.code, computed)
    series = _series_points(computed, request.series_limit)
    return TrendIndicatorResult(stock=stock, signal=signal, series=series)


def trend_screen_stocks(provider: StockProvider, request: TrendScreenRequest) -> TrendScreenResult:
    universe, screen_notes = screening_universe(provider)
    candidate_pool = _screen_candidate_pool(universe, request.criteria, request.limit)

    items: List[TrendStockSignal] = []
    skipped = 0
    for candidate in candidate_pool:
        try:
            analysis = analyze_trend(
                provider,
                TrendIndicatorRequest(
                    code=candidate.stock.code,
                    start_date=request.start_date,
                    end_date=request.end_date,
                    series_limit=60,
                ),
            )
        except (KeyError, ValueError):
            skipped += 1
            continue

        trend_score = _trend_score(analysis.signal)
        final_score = _combined_score(candidate.score, trend_score)
        items.append(
            TrendStockSignal(
                stock=candidate.stock,
                base_score=round(candidate.score, 6),
                trend_score=round(trend_score, 6),
                final_score=round(final_score, 6),
                signal=analysis.signal,
                reasons=[*candidate.reasons, *analysis.signal.reasons],
            )
        )

    items.sort(key=lambda item: item.final_score, reverse=True)
    selected = items[: request.limit]
    notes = [*screen_notes, *TREND_NOTES]
    if skipped:
        notes.append(f"Skipped {skipped} stocks without usable OHLC history.")

    return TrendScreenResult(
        total=len(candidate_pool),
        returned=len(selected),
        items=selected,
        notes=notes,
    )


def _screen_candidate_pool(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    requested_limit: int,
):
    pool_size = min(200, max(requested_limit * 5, criteria.limit, 50))
    expanded_criteria = criteria.model_copy(update={"limit": pool_size})
    return screen_stocks(universe, expanded_criteria).items


def _prepare_history(history: Any, stock: StockItem) -> pd.DataFrame:
    if history is None:
        return pd.DataFrame()
    frame = pd.DataFrame(history).copy()
    if frame.empty:
        return frame

    frame = _normalize_columns(frame)
    if "date" not in frame or "close" not in frame:
        return pd.DataFrame()

    frame["date"] = pd.to_datetime(frame["date"], errors="coerce")
    frame["close"] = pd.to_numeric(frame["close"], errors="coerce")
    frame = frame.dropna(subset=["date", "close"]).sort_values("date").reset_index(drop=True)
    if frame.empty:
        return frame

    frame["open"] = _numeric_or_default(frame, "open", frame["close"])
    frame["high"] = _numeric_or_default(frame, "high", frame[["open", "close"]].max(axis=1))
    frame["low"] = _numeric_or_default(frame, "low", frame[["open", "close"]].min(axis=1))
    frame["volume"] = _numeric_or_default(frame, "volume", pd.Series(0.0, index=frame.index))

    if "capital" in frame:
        frame["capital"] = pd.to_numeric(frame["capital"], errors="coerce")
    else:
        frame["capital"] = np.nan

    if frame["capital"].isna().all() and stock.market_cap_billion and stock.price:
        shares = stock.market_cap_billion * 1_000_000_000 / max(stock.price, 0.01)
        frame["capital"] = shares

    fallback_capital = max(float(frame["volume"].rolling(120, min_periods=1).max().max() or 0) * 20, 1.0)
    frame["capital"] = frame["capital"].replace([np.inf, -np.inf], np.nan).fillna(fallback_capital)
    frame["capital"] = frame["capital"].clip(lower=1.0)
    return frame


def _normalize_columns(frame: pd.DataFrame) -> pd.DataFrame:
    aliases: Dict[str, str] = {
        "日期": "date",
        "date": "date",
        "时间": "date",
        "开盘": "open",
        "open": "open",
        "最高": "high",
        "high": "high",
        "最低": "low",
        "low": "low",
        "收盘": "close",
        "close": "close",
        "成交量": "volume",
        "volume": "volume",
        "vol": "volume",
        "总股本": "capital",
        "流通股本": "capital",
        "capital": "capital",
    }
    rename = {}
    for column in frame.columns:
        key = str(column).strip()
        lower_key = key.lower()
        if key in aliases:
            rename[column] = aliases[key]
        elif lower_key in aliases:
            rename[column] = aliases[lower_key]
    return frame.rename(columns=rename)


def _numeric_or_default(frame: pd.DataFrame, column: str, default: pd.Series) -> pd.Series:
    if column not in frame:
        return default.astype(float)
    values = pd.to_numeric(frame[column], errors="coerce")
    return values.fillna(default).astype(float)


def _compute_indicator_frame(frame: pd.DataFrame, stock: StockItem) -> pd.DataFrame:
    close = frame["close"].astype(float)
    open_ = frame["open"].astype(float)
    high = frame["high"].astype(float)
    low = frame["low"].astype(float)
    volume = frame["volume"].astype(float)
    capital = frame["capital"].astype(float)

    result = frame[["date", "open", "high", "low", "close", "volume", "capital"]].copy()
    ema10 = _ema(close, 10)
    ema20 = _ema(close, 20)
    result["swl"] = (ema10 * 7 + ema20 * 3) / 10
    alpha_percent = (100 * volume.rolling(5, min_periods=1).sum() / (3 * capital)).clip(lower=1.0)
    result["sws"] = _dma(ema20, (alpha_percent / 100).clip(lower=0.01, upper=1.0))

    ref1 = close.shift(1)
    ref2 = close.shift(2)
    var1 = (close > ref1) & (close > ref2)
    vard = (close < ref1) & (close < ref2)
    red_states = _alternating_states(var1, close, starts_with_le_ge=True)
    cyan_states = _alternating_states(vard, close, starts_with_le_ge=False)
    result["red_hold"] = _or_states(red_states)
    result["cyan_watch"] = _or_states(cyan_states)
    result["short_buy"] = result["cyan_watch"].shift(1, fill_value=False) & var1
    result["white_exit"] = result["red_hold"].shift(1, fill_value=False) & vard

    ma34 = _ma(close, 34)
    result["oversold"] = ((close - ma34) / ma34 * 100) < -14

    ytsl = (3 * close + low + open_ + high) / 6
    star_line = pd.Series(0.0, index=frame.index)
    for weight, offset in zip(range(20, 1, -1), range(0, 19)):
        star_line = star_line + ytsl.shift(offset) * weight
    star_line = star_line + ytsl.shift(20)
    result["star_line"] = star_line / 211
    result["bull_line"] = _ma(close, 26)
    ma3 = _ma(close, 3)
    result["wait_line"] = np.where(ma3 > result["star_line"], result["star_line"], ma3)

    e_value = (high + low + open_ + 2 * close) / 5
    result["resistance"] = 2 * e_value - low
    result["support"] = 2 * e_value - high
    result["breakout"] = e_value + (high - low)
    result["reversal"] = e_value - (high - low)

    result["quant_score"] = _quant_score(close, high, low, volume)
    result["swl_above_sws"] = result["swl"] > result["sws"]
    result["trend_score"] = result.apply(lambda row: _trend_score(_row_to_signal(stock.code, row)), axis=1)
    return result


def _alternating_states(primary: pd.Series, close: pd.Series, starts_with_le_ge: bool) -> List[pd.Series]:
    states = [primary.fillna(False)]
    ref1 = close.shift(1)
    ref2 = close.shift(2)
    for index in range(1, 12):
        le_ge = starts_with_le_ge if index % 2 == 1 else not starts_with_le_ge
        if le_ge:
            condition = (close <= ref1) & (close >= ref2)
        else:
            condition = (close >= ref1) & (close <= ref2)
        states.append(states[-1].shift(1, fill_value=False) & condition)
    return states


def _or_states(states: List[pd.Series]) -> pd.Series:
    combined = pd.Series(False, index=states[0].index)
    for state in states:
        combined = combined | state.fillna(False)
    return combined


def _ema(series: pd.Series, span: int) -> pd.Series:
    return series.ewm(span=span, adjust=False, min_periods=1).mean()


def _ma(series: pd.Series, window: int) -> pd.Series:
    return series.rolling(window, min_periods=window).mean()


def _hhv(series: pd.Series, window: int) -> pd.Series:
    return series.rolling(window, min_periods=1).max()


def _llv(series: pd.Series, window: int) -> pd.Series:
    return series.rolling(window, min_periods=1).min()


def _dma(series: pd.Series, alpha: pd.Series) -> pd.Series:
    values: List[float] = []
    previous: Optional[float] = None
    for value, coefficient in zip(series, alpha):
        if not np.isfinite(value):
            values.append(np.nan)
            continue
        coefficient = float(coefficient) if np.isfinite(coefficient) else 0.01
        coefficient = min(max(coefficient, 0.0), 1.0)
        previous = value if previous is None or not np.isfinite(previous) else coefficient * value + (1 - coefficient) * previous
        values.append(previous)
    return pd.Series(values, index=series.index)


def _tdx_sma(series: pd.Series, window: int, weight: int = 1) -> pd.Series:
    values: List[float] = []
    previous: Optional[float] = None
    for value in series.fillna(50.0):
        previous = value if previous is None else (weight * value + (window - weight) * previous) / window
        values.append(previous)
    return pd.Series(values, index=series.index)


def _macd(close: pd.Series):
    dif = _ema(close, 12) - _ema(close, 26)
    dea = _ema(dif, 9)
    macd = 2 * (dif - dea)
    return dif, dea, macd


def _kdj(close: pd.Series, high: pd.Series, low: pd.Series):
    low_min = _llv(low, 9)
    high_max = _hhv(high, 9)
    spread = (high_max - low_min).replace(0, np.nan)
    rsv = ((close - low_min) / spread * 100).clip(lower=0, upper=100)
    k = _tdx_sma(rsv, 3, 1)
    d = _tdx_sma(k, 3, 1)
    j = 3 * k - 2 * d
    return k, d, j


def _quant_score(close: pd.Series, high: pd.Series, low: pd.Series, volume: pd.Series) -> pd.Series:
    k, _d, j = _kdj(close, high, low)
    dif, dea, macd = _macd(close)
    scores = pd.Series(0, index=close.index, dtype=int)
    scores = scores + np.where(_ma(close, 5) > _ma(close, 10), 20, 0)
    scores = scores + np.where(_ma(close, 20) > _ma(close, 60), 10, 0)
    scores = scores + np.where(j > k, 10, 0)
    scores = scores + np.where(dif > dea, 10, 0)
    scores = scores + np.where(macd > 0, 10, 0)
    scores = scores + np.where(volume > _ma(volume, 60), 10, 0)
    scores = scores + np.where(close / close.shift(1) > 1.03, 10, 0)
    return scores.astype(int)


def _latest_signal(code: str, frame: pd.DataFrame) -> TrendIndicatorSignal:
    return _row_to_signal(code, frame.iloc[-1])


def _row_to_signal(code: str, row: Any) -> TrendIndicatorSignal:
    reasons = _signal_reasons(row)
    return TrendIndicatorSignal(
        code=code,
        date=_format_date(row["date"]),
        close=_rounded(row["close"]) or 0.0,
        swl=_rounded(row.get("swl")),
        sws=_rounded(row.get("sws")),
        star_line=_rounded(row.get("star_line")),
        bull_line=_rounded(row.get("bull_line")),
        wait_line=_rounded(row.get("wait_line")),
        support=_rounded(row.get("support")),
        resistance=_rounded(row.get("resistance")),
        breakout=_rounded(row.get("breakout")),
        reversal=_rounded(row.get("reversal")),
        swl_above_sws=bool(row.get("swl_above_sws", False)),
        red_hold=bool(row.get("red_hold", False)),
        cyan_watch=bool(row.get("cyan_watch", False)),
        short_buy=bool(row.get("short_buy", False)),
        white_exit=bool(row.get("white_exit", False)),
        oversold=bool(row.get("oversold", False)),
        quant_score=int(row.get("quant_score", 0) or 0),
        quant_score_max=QUANT_SCORE_MAX,
        status=_signal_status(row),
        reasons=reasons,
        notes=list(TREND_NOTES),
    )


def _signal_reasons(row: Any) -> List[str]:
    reasons: List[str] = []
    if bool(row.get("short_buy", False)):
        reasons.append("short_buy_signal")
    if bool(row.get("red_hold", False)):
        reasons.append("red_hold")
    if bool(row.get("swl_above_sws", False)):
        reasons.append("swl_above_sws")
    if int(row.get("quant_score", 0) or 0) >= 60:
        reasons.append("high_quant_score")
    if bool(row.get("white_exit", False)):
        reasons.append("white_exit")
    if bool(row.get("cyan_watch", False)):
        reasons.append("cyan_watch")
    if bool(row.get("oversold", False)):
        reasons.append("oversold")
    return reasons


def _signal_status(row: Any) -> str:
    if bool(row.get("short_buy", False)):
        return "buy_setup"
    if bool(row.get("white_exit", False)):
        return "exit"
    if bool(row.get("red_hold", False)) and bool(row.get("swl_above_sws", False)):
        return "uptrend"
    if bool(row.get("red_hold", False)):
        return "hold"
    if bool(row.get("cyan_watch", False)):
        return "watch"
    if bool(row.get("oversold", False)):
        return "oversold"
    return "neutral"


def _trend_score(signal: TrendIndicatorSignal) -> float:
    score = (signal.quant_score / max(signal.quant_score_max, 1)) * 58
    if signal.swl_above_sws:
        score += 10
    if signal.red_hold:
        score += 12
    if signal.short_buy:
        score += 16
    if signal.close and signal.star_line and signal.close > signal.star_line:
        score += 4
    if signal.close and signal.bull_line and signal.close > signal.bull_line:
        score += 4
    if signal.white_exit:
        score -= 26
    if signal.cyan_watch:
        score -= 10
    if signal.oversold:
        score -= 8
    return min(max(score, 0.0), 100.0)


def _combined_score(base_score: float, trend_score: float) -> float:
    base_component = min(max(base_score, 0.0), 10.0) * 4
    return min(base_component + trend_score * 0.75, 100.0)


def _series_points(frame: pd.DataFrame, limit: int) -> List[TrendIndicatorPoint]:
    points = []
    for _index, row in frame.tail(limit).iterrows():
        points.append(
            TrendIndicatorPoint(
                date=_format_date(row["date"]),
                close=_rounded(row["close"]) or 0.0,
                swl=_rounded(row.get("swl")),
                sws=_rounded(row.get("sws")),
                red_hold=bool(row.get("red_hold", False)),
                cyan_watch=bool(row.get("cyan_watch", False)),
                short_buy=bool(row.get("short_buy", False)),
                white_exit=bool(row.get("white_exit", False)),
            )
        )
    return points


def _rounded(value: Any) -> Optional[float]:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if not np.isfinite(number):
        return None
    return round(number, 4)


def _format_date(value: Any) -> str:
    if hasattr(value, "strftime"):
        return value.strftime("%Y-%m-%d")
    return str(value)
