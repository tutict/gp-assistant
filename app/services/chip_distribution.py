from __future__ import annotations

import math
from typing import Any

import pandas as pd

from app.schemas import ChipDistributionResult


def estimate_chip_distribution_from_history(history: Any) -> ChipDistributionResult:
    frame = pd.DataFrame(history).copy() if history is not None else pd.DataFrame()
    records = _records_from_history(frame)
    if len(records) < 20:
        return ChipDistributionResult(
            source="本地日线成本分布估算",
            status="unavailable",
            note="本地日线不足 20 条，暂不估算成本分布",
        )

    metrics = calculate_chip_metrics(len(records) - 1, records)
    latest = records[-1]
    return ChipDistributionResult(
        source="本地日线成本分布估算",
        status="estimated",
        date=latest["date"],
        winner_ratio=_percent(metrics["winner_ratio"]),
        avg_cost=_rounded(metrics["avg_cost"]),
        cost_90_low=_rounded(metrics["cost_90_low"]),
        cost_90_high=_rounded(metrics["cost_90_high"]),
        concentration_90=_percent(metrics["concentration_90"]),
        note="基于 OHLCV、成交额和股本换手率估算，不等同真实持仓明细",
    )


def eastmoney_klines_to_chip_frame(klines: list[str]) -> pd.DataFrame:
    records = [record for item in klines if (record := _eastmoney_kline_record(item)) is not None]
    if not records:
        return pd.DataFrame()

    rows = []
    for index in range(max(0, len(records) - 90), len(records)):
        metrics = calculate_chip_metrics(index, records)
        rows.append(
            {
                "日期": records[index]["date"],
                "获利比例": metrics["winner_ratio"],
                "平均成本": metrics["avg_cost"],
                "90成本-低": metrics["cost_90_low"],
                "90成本-高": metrics["cost_90_high"],
                "90集中度": metrics["concentration_90"],
                "70成本-低": metrics["cost_70_low"],
                "70成本-高": metrics["cost_70_high"],
                "70集中度": metrics["concentration_70"],
            }
        )
    return pd.DataFrame(rows)


def calculate_chip_metrics(index: int, records: list[dict]) -> dict[str, float]:
    factor = 150
    kdata = records[max(0, index - 119) : index + 1]
    max_price = max(record["high"] for record in kdata)
    min_price = min(record["low"] for record in kdata)
    accuracy = max(0.01, (max_price - min_price) / (factor - 1))
    xdata = [0.0] * factor

    for record in kdata:
        open_price = record["open"]
        close = record["close"]
        high = record["high"]
        low = record["low"]
        avg = (open_price + close + high + low) / 4
        turnover_rate = min(1.0, max(0.0, record.get("turnover_rate") or 0.0))

        xdata = [value * (1 - turnover_rate) for value in xdata]
        if high == low:
            bucket = int(math.floor((avg - min_price) / accuracy))
            if 0 <= bucket < factor:
                xdata[bucket] += (factor - 1) * turnover_rate / 2
            continue

        high_bucket = int(math.floor((high - min_price) / accuracy))
        low_bucket = int(math.ceil((low - min_price) / accuracy))
        scale = 2 / (high - low)
        for bucket in range(max(0, low_bucket), min(factor - 1, high_bucket) + 1):
            current_price = min_price + accuracy * bucket
            if current_price <= avg:
                addition = scale * turnover_rate if abs(avg - low) < 1e-8 else (
                    (current_price - low) / (avg - low) * scale * turnover_rate
                )
            else:
                addition = scale * turnover_rate if abs(high - avg) < 1e-8 else (
                    (high - current_price) / (high - avg) * scale * turnover_rate
                )
            xdata[bucket] += max(addition, 0.0)

    total_chips = sum(xdata)
    current_close = records[index]["close"]

    def cost_by_chip(chip: float) -> float:
        stacked = 0.0
        for bucket, value in enumerate(xdata):
            if stacked + value > chip:
                return min_price + bucket * accuracy
            stacked += value
        return min_price + (factor - 1) * accuracy

    def percent_chips(percent: float) -> tuple[float, float, float]:
        low = cost_by_chip(total_chips * (1 - percent) / 2)
        high = cost_by_chip(total_chips * (1 + percent) / 2)
        concentration = 0.0 if low + high == 0 else (high - low) / (low + high)
        return low, high, concentration

    winner_ratio = 0.0
    if total_chips > 0:
        below = sum(value for bucket, value in enumerate(xdata) if current_close >= min_price + bucket * accuracy)
        winner_ratio = below / total_chips
    cost_90_low, cost_90_high, concentration_90 = percent_chips(0.9)
    cost_70_low, cost_70_high, concentration_70 = percent_chips(0.7)
    return {
        "winner_ratio": winner_ratio,
        "avg_cost": cost_by_chip(total_chips * 0.5),
        "cost_90_low": cost_90_low,
        "cost_90_high": cost_90_high,
        "concentration_90": concentration_90,
        "cost_70_low": cost_70_low,
        "cost_70_high": cost_70_high,
        "concentration_70": concentration_70,
    }


def _records_from_history(frame: pd.DataFrame) -> list[dict]:
    if frame.empty:
        return []
    required = {"date", "open", "high", "low", "close", "volume", "capital"}
    if not required.issubset(set(frame.columns)):
        return []

    records: list[dict] = []
    for _, row in frame.iterrows():
        close = _finite_float(row.get("close"))
        capital = _finite_float(row.get("capital"))
        volume = _normalized_volume(row, close)
        if close is None or capital is None or capital <= 0 or volume is None:
            continue
        record = {
            "date": _format_date(row.get("date")),
            "open": _finite_float(row.get("open")) or close,
            "high": _finite_float(row.get("high")) or close,
            "low": _finite_float(row.get("low")) or close,
            "close": close,
            "turnover_rate": min(max(volume / capital, 0.0), 1.0),
        }
        records.append(record)
    return records


def _eastmoney_kline_record(item: str) -> dict | None:
    fields = str(item or "").split(",")
    if len(fields) < 11:
        return None
    open_price = _finite_float(fields[1])
    close = _finite_float(fields[2])
    high = _finite_float(fields[3])
    low = _finite_float(fields[4])
    turnover_percent = _finite_float(fields[10])
    if any(value is None for value in (open_price, close, high, low)):
        return None
    return {
        "date": fields[0],
        "open": open_price,
        "close": close,
        "high": high,
        "low": low,
        "turnover_rate": min(max((turnover_percent or 0.0) / 100, 0.0), 1.0),
    }


def _normalized_volume(row: Any, close: float | None) -> float | None:
    volume = _finite_float(row.get("volume"))
    if volume is None:
        return None
    amount = _finite_float(row.get("amount"))
    if close and close > 0 and amount and amount > 0:
        implied_shares = amount / close
        if volume > 0:
            ratio = implied_shares / volume
            if 50 <= ratio <= 150:
                return implied_shares
    return volume


def _finite_float(value: Any) -> float | None:
    try:
        numeric = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(numeric):
        return None
    return numeric


def _format_date(value: Any) -> str:
    parsed = pd.to_datetime(value, errors="coerce")
    if pd.isna(parsed):
        return str(value or "")
    return parsed.strftime("%Y-%m-%d")


def _rounded(value: float | None) -> float | None:
    return round(value, 4) if value is not None and math.isfinite(value) else None


def _percent(value: float | None) -> float | None:
    if value is None or not math.isfinite(value):
        return None
    return round(value * 100, 4)
