from __future__ import annotations

from dataclasses import dataclass
from datetime import date, timedelta
import importlib
import re
from typing import Any, Dict, List, Optional

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


HOT_SECTOR_KEYWORDS: tuple[tuple[str, float], ...] = (
    ("氟化工", 0.58),
    ("氟材料", 0.58),
    ("锂电材料", 0.56),
    ("电解液", 0.54),
    ("六氟磷酸锂", 0.54),
    ("新能材", 0.52),
    ("新材料", 0.48),
    ("固态电池", 0.48),
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
    ("游戏", 0.42),
    ("手游", 0.4),
    ("电竞", 0.36),
    ("云游戏", 0.36),
    ("互动娱乐", 0.34),
)

HOT_SECTOR_CATEGORIES: tuple[tuple[str, tuple[str, ...]], ...] = (
    (
        "materials",
        ("氟化工", "氟材料", "锂电材料", "电解液", "六氟磷酸锂", "新能材", "新材料", "固态电池"),
    ),
    (
        "tech",
        ("半导体", "芯片", "算力", "人工智能", "ai", "机器人", "软件", "通信", "科技", "电子"),
    ),
    ("energy", ("新能源", "电池", "储能", "光伏", "电力", "能源", "油气", "煤炭")),
    ("game", ("游戏", "网络游戏", "手游", "电竞", "云游戏", "互动娱乐", "文化传媒")),
)

HOT_SECTOR_PROMOTION_ORDER = ("materials", "tech", "energy", "game")
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
    score += _hot_sector_bonus(f"{stock.name} {stock.industry}")
    score += _hot_theme_bonus(stock)
    score -= _cold_sector_penalty(stock.industry)
    return score


def _hot_sector_bonus(text: str) -> float:
    normalized = (text or "").strip().lower()
    if not normalized:
        return 0.0
    return max((weight for keyword, weight in HOT_SECTOR_KEYWORDS if keyword in normalized), default=0.0)


def _hot_theme_bonus(stock: StockItem) -> float:
    category = _hot_sector_category(f"{stock.name} {stock.industry}")
    return 0.75 if category == "materials" else 0.0


def _cold_sector_penalty(industry: str) -> float:
    return 1.1 if _is_cold_sector(industry) else 0.0


def _is_cold_sector(industry: str) -> bool:
    normalized = (industry or "").strip().lower()
    return bool(normalized) and any(keyword in normalized for keyword in COLD_SECTOR_KEYWORDS)


def _hot_pick_category(stock: StockItem) -> Optional[str]:
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

    for category in HOT_SECTOR_PROMOTION_ORDER:
        for item in screened:
            if item.stock.code in used_codes or _hot_pick_category(item.stock) != category:
                continue
            promoted.append(item)
            used_codes.add(item.stock.code)
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


def _apply_institution_buy_ratio_filter(
    screened: List[ScreenedStock],
    criteria: ScreenCriteria,
    notes: list[str],
) -> List[ScreenedStock]:
    if not criteria.require_institution_buy_ratio_gt_sell_ratio:
        return screened
    candidate_codes = {stock_digits(item.stock.code) for item in screened}
    candidate_codes.discard("")
    signals, signal_notes = _load_institution_buy_ratio_signals(candidate_codes)
    notes.extend(signal_notes)
    if not signals:
        notes.append("机构买入占比规则已启用，但没有取得可匹配的龙虎榜机构买卖数据，候选股未放行。")
        return []

    matched: List[ScreenedStock] = []
    for item in screened:
        signal = signals.get(stock_digits(item.stock.code))
        if signal is None or not signal.passes():
            continue
        reason = signal.reason()
        matched.append(
            ScreenedStock(
                stock=item.stock,
                score=item.score + _institution_signal_bonus(signal),
                reasons=[*item.reasons, reason],
            )
        )
    notes.append(
        f"机构买入占比规则：近 {_institution_lhb_lookback_days()} 天龙虎榜机构买入占比需高于卖出占比，命中 {len(matched)} 只。"
    )
    return matched


def _load_institution_buy_ratio_signals(candidate_codes: set[str]) -> tuple[dict[str, InstitutionBuySignal], list[str]]:
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


def _screened_stocks(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: Optional[list[str]] = None,
) -> List[ScreenedStock]:
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

    return _apply_institution_buy_ratio_filter(screened, criteria, notes if notes is not None else [])


def _optional_sort_key(value: Optional[float], descending: bool) -> tuple[int, float]:
    if value is None:
        return (1, 0.0)
    return (0, -value if descending else value)


def screen_stocks(
    universe: List[StockItem],
    criteria: ScreenCriteria,
    notes: Optional[List[str]] = None,
) -> ScreenResult:
    result_notes = [*(notes or [])]
    screened = _screened_stocks(universe, criteria, result_notes)
    items, promoted = _promote_hot_sector_items(screened, criteria)
    if promoted:
        result_notes.append("已优先推送新能材、科技、能源、游戏等热门主题候选，并降低银行/基建优先级。")
    return ScreenResult(total=len(screened), returned=len(items), items=items, notes=result_notes)


def screen_stocks_by_sector(
    universe: List[StockItem],
    request: SectorScreenRequest,
    notes: Optional[List[str]] = None,
) -> SectorScreenResult:
    result_notes = [*(notes or []), "按股票行业字段作为板块分组。"]
    screened = _screened_stocks(universe, request.criteria, result_notes)
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
    if request.criteria.industry:
        result_notes.append(f"已限制行业：{request.criteria.industry}")

    return SectorScreenResult(
        total=len(screened),
        returned=returned,
        sector_count=len(groups),
        groups=groups,
        notes=result_notes,
    )
