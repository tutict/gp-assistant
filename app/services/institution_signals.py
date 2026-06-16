from __future__ import annotations

from dataclasses import dataclass
from datetime import date, timedelta
import importlib
import re
from typing import Any

from app.schemas import ScreenCriteria, ScreenedStock
from app.services.runtime_config import env_int, redact_error
from app.services.stock_code import stock_digits


INSTITUTION_BUY_COLUMNS = (
    "机构买入总额",
    "机构席位买入额",
    "机构买入额",
    "累计买入额",
    "买入额",
)
INSTITUTION_SELL_COLUMNS = (
    "机构卖出总额",
    "机构席位卖出额",
    "机构卖出额",
    "累计卖出额",
    "卖出额",
)
INSTITUTION_BUY_RATIO_COLUMNS = (
    "机构买入占比",
    "机构席位买入占比",
    "买入占比",
)
INSTITUTION_SELL_RATIO_COLUMNS = (
    "机构卖出占比",
    "机构席位卖出占比",
    "卖出占比",
)
INSTITUTION_TOTAL_COLUMNS = (
    "成交额",
    "总成交额",
    "龙虎榜成交额",
    "市场总成交额",
)
LHB_CODE_COLUMNS = ("代码", "股票代码", "证券代码", "code")


@dataclass
class InstitutionBuySignal:
    buy: float = 0.0
    sell: float = 0.0
    total_amount: float = 0.0
    buy_ratio_sum: float = 0.0
    sell_ratio_sum: float = 0.0
    ratio_count: int = 0
    latest_date: str = ""

    def add(
        self,
        *,
        buy: float | None,
        sell: float | None,
        total_amount: float | None,
        buy_ratio: float | None,
        sell_ratio: float | None,
        trade_date: str,
    ) -> None:
        if buy is not None:
            self.buy += buy
        if sell is not None:
            self.sell += sell
        if total_amount is not None:
            self.total_amount += total_amount
        if buy_ratio is not None and sell_ratio is not None:
            self.buy_ratio_sum += buy_ratio
            self.sell_ratio_sum += sell_ratio
            self.ratio_count += 1
        if trade_date and trade_date > self.latest_date:
            self.latest_date = trade_date

    @property
    def buy_ratio(self) -> float | None:
        if self.total_amount > 0:
            return self.buy / self.total_amount * 100
        if self.ratio_count:
            return self.buy_ratio_sum / self.ratio_count
        return None

    @property
    def sell_ratio(self) -> float | None:
        if self.total_amount > 0:
            return self.sell / self.total_amount * 100
        if self.ratio_count:
            return self.sell_ratio_sum / self.ratio_count
        return None

    def passes(self) -> bool:
        buy_ratio = self.buy_ratio
        sell_ratio = self.sell_ratio
        if buy_ratio is not None and sell_ratio is not None:
            return buy_ratio > sell_ratio
        return self.buy > self.sell

    def reason(self) -> str:
        buy_ratio = self.buy_ratio
        sell_ratio = self.sell_ratio
        if buy_ratio is not None and sell_ratio is not None:
            return f"机构买入占比 {buy_ratio:.1f}% > 卖出占比 {sell_ratio:.1f}%"
        return f"机构买入额 {_format_amount(self.buy)} > 卖出额 {_format_amount(self.sell)}"


def apply_institution_buy_ratio_filter(
    screened: list[ScreenedStock],
    criteria: ScreenCriteria,
    notes: list[str],
) -> list[ScreenedStock]:
    if not criteria.require_institution_buy_ratio_gt_sell_ratio:
        return screened
    candidate_codes = {stock_digits(item.stock.code) for item in screened}
    candidate_codes.discard("")
    signals, signal_notes = load_institution_buy_ratio_signals(candidate_codes)
    notes.extend(signal_notes)
    if not signals:
        notes.append("机构买入占比规则已启用，但没有取得可匹配的龙虎榜机构买卖数据，候选股未放行。")
        return []

    matched: list[ScreenedStock] = []
    for item in screened:
        signal = signals.get(stock_digits(item.stock.code))
        if signal is None or not signal.passes():
            continue
        bonus = _institution_signal_bonus(signal)
        factor_scores = dict(item.factor_scores or {})
        factor_scores["institution"] = round(min(1.0, bonus / 1.5), 4)
        matched.append(
            item.model_copy(
                update={
                    "score": round(item.score + bonus, 6),
                    "reasons": [*item.reasons, signal.reason()],
                    "factor_scores": factor_scores,
                }
            )
        )
    notes.append(
        f"机构买入占比规则：近 {_institution_lhb_lookback_days()} 天龙虎榜机构买入占比需高于卖出占比，命中 {len(matched)} 只。"
    )
    return matched


def load_institution_buy_ratio_signals(candidate_codes: set[str]) -> tuple[dict[str, InstitutionBuySignal], list[str]]:
    notes: list[str] = []
    if not candidate_codes:
        return {}, notes
    start_date, end_date = _institution_lhb_date_window()
    try:
        ak = importlib.import_module("akshare")
    except Exception as exc:
        return {}, [f"机构买入占比规则无法加载 AkShare：{redact_error(exc)}"]

    frame = None
    source_name = "东方财富龙虎榜机构统计"
    try:
        frame = ak.stock_lhb_jgmmtj_em(start_date=start_date, end_date=end_date)
    except Exception as exc:
        notes.append(f"东方财富龙虎榜机构统计不可用：{redact_error(exc)}")
        source_name = "新浪龙虎榜机构席位备用源"
        frame = _safe_sina_lhb_frame(ak, notes)

    signals = _institution_signals_from_frame(frame, candidate_codes)
    if signals:
        notes.append(f"机构买入占比数据源：{source_name}，窗口 {start_date}-{end_date}。")
        return signals, notes
    if frame is not None:
        fallback = _safe_sina_lhb_frame(ak, notes) if source_name != "新浪龙虎榜机构席位备用源" else None
        fallback_signals = _institution_signals_from_frame(fallback, candidate_codes)
        if fallback_signals:
            notes.append(f"机构买入占比数据源：新浪龙虎榜机构席位备用源，窗口 {start_date}-{end_date}。")
            return fallback_signals, notes
        notes.append("龙虎榜机构统计没有命中当前候选池。")
    return {}, notes


def _safe_sina_lhb_frame(ak: Any, notes: list[str]) -> Any:
    try:
        notes.append("已切换新浪龙虎榜机构席位备用源。")
        return ak.stock_lhb_jgmx_sina()
    except Exception as exc:
        notes.append(f"新浪龙虎榜机构席位不可用：{redact_error(exc)}")
        return None


def _institution_signals_from_frame(frame: Any, candidate_codes: set[str]) -> dict[str, InstitutionBuySignal]:
    signals: dict[str, InstitutionBuySignal] = {}
    for row in _iter_rows(frame):
        code = _row_code_digits(row)
        if not code or code not in candidate_codes:
            continue
        buy = _row_number(row, INSTITUTION_BUY_COLUMNS)
        sell = _row_number(row, INSTITUTION_SELL_COLUMNS)
        buy_ratio = _row_percent(row, INSTITUTION_BUY_RATIO_COLUMNS)
        sell_ratio = _row_percent(row, INSTITUTION_SELL_RATIO_COLUMNS)
        total_amount = _row_number(row, INSTITUTION_TOTAL_COLUMNS)
        if buy is None and sell is None and (buy_ratio is None or sell_ratio is None):
            continue
        signals.setdefault(code, InstitutionBuySignal()).add(
            buy=buy,
            sell=sell,
            total_amount=total_amount,
            buy_ratio=buy_ratio,
            sell_ratio=sell_ratio,
            trade_date=_row_trade_date(row),
        )
    return signals


def _institution_lhb_date_window() -> tuple[str, str]:
    end = date.today()
    start = end - timedelta(days=_institution_lhb_lookback_days())
    return start.strftime("%Y%m%d"), end.strftime("%Y%m%d")


def _institution_lhb_lookback_days() -> int:
    return env_int("GP_SCREEN_LHB_LOOKBACK_DAYS", 30, minimum=1, maximum=180)


def _institution_signal_bonus(signal: InstitutionBuySignal) -> float:
    if signal.sell > 0:
        return min(1.5, max(0.0, (signal.buy - signal.sell) / signal.sell))
    if signal.buy > 0:
        return 1.0
    buy_ratio = signal.buy_ratio
    sell_ratio = signal.sell_ratio
    if buy_ratio is not None and sell_ratio is not None:
        return min(1.5, max(0.0, (buy_ratio - sell_ratio) / 20))
    return 0.0


def _iter_rows(frame: Any):
    if frame is None or getattr(frame, "empty", False):
        return []
    if hasattr(frame, "iterrows"):
        return (row for _, row in frame.iterrows())
    if isinstance(frame, list):
        return frame
    return []


def _row_code_digits(row: Any) -> str:
    for column in LHB_CODE_COLUMNS:
        digits = stock_digits(_row_get(row, column))
        if digits:
            return digits
    return ""


def _row_trade_date(row: Any) -> str:
    value = _row_get_any(row, ("日期", "交易日期", "上榜日期", "trade_date", "date"))
    return "".join(char for char in str(value or "") if char.isdigit())[:8]


def _row_number(row: Any, columns: tuple[str, ...]) -> float | None:
    for column in columns:
        value = _to_number(_row_get(row, column))
        if value is not None:
            return value
    return None


def _row_percent(row: Any, columns: tuple[str, ...]) -> float | None:
    for column in columns:
        value = _to_percent(_row_get(row, column))
        if value is not None:
            return value
    return None


def _row_get_any(row: Any, columns: tuple[str, ...]) -> Any:
    for column in columns:
        value = _row_get(row, column)
        if value is not None and str(value).strip() not in {"", "-", "--", "nan", "None"}:
            return value
    return None


def _row_get(row: Any, column: str) -> Any:
    if hasattr(row, "get"):
        return row.get(column)
    if isinstance(row, dict):
        return row.get(column)
    return None


def _to_number(value: Any) -> float | None:
    if value is None:
        return None
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return float(value)
    text = str(value).strip().replace(",", "")
    if not text or text in {"-", "--", "nan", "None"}:
        return None
    multiplier = 1.0
    if "亿" in text:
        multiplier = 100_000_000.0
    elif "万" in text:
        multiplier = 10_000.0
    match = re.search(r"-?\d+(?:\.\d+)?", text)
    if not match:
        return None
    return float(match.group(0)) * multiplier


def _to_percent(value: Any) -> float | None:
    if value is None:
        return None
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        number = float(value)
        return number * 100 if -1 <= number <= 1 else number
    text = str(value).strip()
    if not text or text in {"-", "--", "nan", "None"}:
        return None
    number = _to_number(text)
    if number is None:
        return None
    return number if "%" in text or abs(number) > 1 else number * 100


def _format_amount(value: float) -> str:
    if abs(value) >= 100_000_000:
        return f"{value / 100_000_000:.2f} 亿"
    if abs(value) >= 10_000:
        return f"{value / 10_000:.2f} 万"
    return f"{value:.0f}"
