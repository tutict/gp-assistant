import os
import math
from datetime import datetime
from typing import Dict, List, Optional

import pandas as pd
import requests

from app.providers.base import StockProvider, env_float, normalize_proxy_mode, proxy_environment
from app.schemas import (
    FinancialIndicatorItem,
    FinancialIndicatorSection,
    MinuteBar,
    OrderBookLevel,
    OrderBookSnapshot,
    StockItem,
    StockRelation,
)
from app.services.chip_distribution import eastmoney_klines_to_chip_frame


class AkShareProvider(StockProvider):
    name = "akshare"

    def __init__(
        self,
        cache_path: Optional[str] = None,
        refresh: bool = False,
        proxy_mode: Optional[str] = None,
    ):
        self.cache_path = cache_path or os.getenv("AKSHARE_CACHE", "data/cache/stocks.csv")
        self.refresh = refresh or os.getenv("AKSHARE_REFRESH", "false").lower() == "true"
        self.proxy_mode = normalize_proxy_mode(proxy_mode)

    def list_stocks(self) -> List[StockItem]:
        df = self._load_spot()
        return [self._row_to_stock(row) for _, row in df.iterrows()]

    def list_stocks_for_screen(self) -> tuple[List[StockItem], List[str]]:
        try:
            df = self._load_spot()
            if not self._has_any_column(df, self._previous_close_columns()):
                df = self._fetch_spot()
                os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
                df.to_csv(self.cache_path, index=False)
        except Exception as exc:
            return self.list_stocks(), [f"前一交易日收盘价不可用，已回退到股票池价格：{exc}"]

        items: List[StockItem] = []
        previous_close_count = 0
        fallback_count = 0
        for _, row in df.iterrows():
            stock = self._row_to_stock(row, use_previous_close=True)
            items.append(stock)
            previous_col = self._first_present_optional(row, self._previous_close_columns())
            if previous_col and self._positive_float(row.get(previous_col)) is not None:
                previous_close_count += 1
            else:
                fallback_count += 1

        notes = [f"筛选价格口径：前一交易日收盘价（AkShare 昨收字段），已应用 {previous_close_count} 只。"]
        if fallback_count:
            notes.append(f"{fallback_count} 只股票缺少昨收价，已回退到股票池价格。")
        return items, notes

    def get_stock(self, code: str) -> StockItem:
        normalized_code = self._normalize_code(code)
        for item in self.list_stocks():
            if item.code == normalized_code:
                return item
        raise KeyError(f"未找到股票 {code}")

    def get_history(self, code: str, start_date: str, end_date: str):
        ak = self._import_akshare()
        history_fn = getattr(ak, "stock_zh_a_hist", None)
        if history_fn is None:
            raise RuntimeError("AkShare 缺少 stock_zh_a_hist 接口")

        symbol = code.split(".")[0]
        with proxy_environment(self.proxy_mode):
            df = history_fn(symbol=symbol, period="daily", start_date=start_date, end_date=end_date, adjust="")
        if df is None or df.empty:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume"])

        columns = {
            "date": self._pick_col(df, ["日期", "date"]),
            "open": self._pick_col(df, ["开盘", "open"], required=False),
            "high": self._pick_col(df, ["最高", "high"], required=False),
            "low": self._pick_col(df, ["最低", "low"], required=False),
            "close": self._pick_col(df, ["收盘", "close"]),
            "volume": self._pick_col(df, ["成交量", "volume", "vol"], required=False),
        }
        selected = [source for source in columns.values() if source]
        renamed = {source: target for target, source in columns.items() if source}
        df = df[selected].rename(columns=renamed)
        df["date"] = df["date"].astype(str)
        return df

    def get_chip_distribution(self, code: str):
        timeout = env_float("GP_CHIP_DISTRIBUTION_TIMEOUT", 6, minimum=1, maximum=20)
        symbol = self._normalize_code(code).split(".")[0]
        market_code = 1 if symbol.startswith("6") else 0
        params = {
            "secid": f"{market_code}.{symbol}",
            "fields1": "f1,f2,f3,f4,f5,f6",
            "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
            "klt": "101",
            "fqt": "0",
            "end": datetime.now().date().strftime("%Y%m%d"),
            "lmt": "210",
        }
        with proxy_environment(self.proxy_mode):
            response = requests.get(
                "https://push2his.eastmoney.com/api/qt/stock/kline/get",
                params=params,
                timeout=timeout,
                headers={"User-Agent": "Mozilla/5.0"},
            )
        response.raise_for_status()
        data = response.json().get("data") or {}
        return eastmoney_klines_to_chip_frame(data.get("klines") or [])

    def get_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> List[MinuteBar]:
        ak = self._import_akshare()
        minute_fn = getattr(ak, "stock_zh_a_hist_min_em", None)
        if minute_fn is None:
            raise RuntimeError("AkShare 缺少 stock_zh_a_hist_min_em 接口")

        period = str(period or "1")
        if period not in {"1", "5", "15", "30", "60"}:
            period = "1"
        symbol = self._normalize_code(code).split(".")[0]
        with proxy_environment(self.proxy_mode):
            df = minute_fn(
                symbol=symbol,
                start_date=start_datetime,
                end_date=end_datetime,
                period=period,
                adjust="",
            )
        if df is None or df.empty:
            return []

        columns = {
            "datetime": self._pick_col(df, ["时间", "datetime", "date"]),
            "open": self._pick_col(df, ["开盘", "open"], required=False),
            "high": self._pick_col(df, ["最高", "high"], required=False),
            "low": self._pick_col(df, ["最低", "low"], required=False),
            "close": self._pick_col(df, ["收盘", "close"]),
            "volume": self._pick_col(df, ["成交量", "volume", "vol"], required=False),
            "amount": self._pick_col(df, ["成交额", "amount"], required=False),
        }

        bars: List[MinuteBar] = []
        for _, row in df.iterrows():
            close = self._to_float(row.get(columns["close"]))
            if close is None:
                continue
            open_price = self._to_float(row.get(columns["open"])) if columns["open"] else close
            high = self._to_float(row.get(columns["high"])) if columns["high"] else close
            low = self._to_float(row.get(columns["low"])) if columns["low"] else close
            bars.append(
                MinuteBar(
                    datetime=str(row.get(columns["datetime"], "")),
                    open=open_price if open_price is not None else close,
                    high=high if high is not None else close,
                    low=low if low is not None else close,
                    close=close,
                    volume=self._to_float(row.get(columns["volume"])) if columns["volume"] else None,
                    amount=self._to_float(row.get(columns["amount"])) if columns["amount"] else None,
                )
            )
        return bars

    def get_order_book(self, code: str) -> OrderBookSnapshot | None:
        ak = self._import_akshare()
        bid_ask_fn = getattr(ak, "stock_bid_ask_em", None)
        if bid_ask_fn is None:
            raise RuntimeError("AkShare 缺少 stock_bid_ask_em 接口")

        normalized_code = self._normalize_code(code)
        symbol = normalized_code.split(".")[0]
        with proxy_environment(self.proxy_mode):
            df = bid_ask_fn(symbol=symbol)
        if df is None or df.empty or "item" not in df or "value" not in df:
            return None

        values = {str(row["item"]): row["value"] for _, row in df.iterrows()}
        bids = [
            OrderBookLevel(
                level=level,
                price=self._to_float(values.get(f"buy_{level}")),
                volume=self._to_float(values.get(f"buy_{level}_vol")),
            )
            for level in range(1, 6)
        ]
        asks = [
            OrderBookLevel(
                level=level,
                price=self._to_float(values.get(f"sell_{level}")),
                volume=self._to_float(values.get(f"sell_{level}_vol")),
            )
            for level in range(1, 6)
        ]
        metric_keys = ["最新", "均价", "涨幅", "涨跌", "总手", "金额", "换手", "量比", "最高", "最低", "今开", "昨收", "涨停", "跌停", "外盘", "内盘"]
        return OrderBookSnapshot(
            code=normalized_code,
            timestamp=datetime.now().isoformat(timespec="seconds"),
            bids=bids,
            asks=asks,
            metrics={key: self._to_float(values.get(key)) for key in metric_keys if key in values},
        )

    def get_financial_indicators(self, stock: StockItem) -> FinancialIndicatorSection | None:
        ak = self._import_akshare()
        normalized_code = self._normalize_code(stock.code)
        row = None
        period = None
        source_parts: list[str] = []
        notes: list[str] = []
        quarterly_eps_items: list[FinancialIndicatorItem] = []

        notes.append("东财 F10 财报指标已禁用，当前仅合并本地估值、同花顺/新浪可用财务摘要。")

        abstract_values: dict[str, float] = {}
        abstract_period = None
        abstract_fn = getattr(ak, "stock_financial_abstract", None)
        if abstract_fn is not None:
            try:
                with proxy_environment(self.proxy_mode):
                    abstract_df = abstract_fn(symbol=normalized_code.split(".")[0])
                abstract_values, abstract_period = self._latest_abstract_values(abstract_df)
                if abstract_values:
                    source_parts.append("\u65b0\u6d6a\u8d22\u62a5")
                    period = period or abstract_period
            except Exception as exc:
                notes.append(f"\u65b0\u6d6a\u8d22\u62a5\u6458\u8981\u6682\u4e0d\u53ef\u7528\uff1a{exc}")

        eps_items, eps_source, eps_period, eps_notes = self.get_quarterly_eps_indicators(normalized_code)
        quarterly_eps_items.extend(eps_items)
        notes.extend(eps_notes)
        if eps_source and eps_items:
            source_parts.append(eps_source)
            period = period or eps_period

        items: list[FinancialIndicatorItem] = []
        if stock.pe is not None or stock.pb is not None or stock.market_cap_billion is not None:
            source_parts.append("\u884c\u60c5\u4f30\u503c")
        self._append_indicator(items, "\u5e02\u76c8\u7387(TTM)", stock.pe, self._format_decimal)
        self._append_indicator(items, "\u5e02\u51c0\u7387(\u6700\u65b0)", stock.pb, self._format_decimal)

        pick = lambda keys, default=None: self._row_value(row, keys, default)
        self._append_indicator(items, "\u6bcf\u80a1\u6536\u76ca(\u8ba1\u7b97)", pick(["EPSJB"]), self._format_yuan)
        self._append_indicator(items, "\u6bcf\u80a1\u51c0\u8d44\u4ea7", pick(["BPS"]), self._format_yuan)
        revenue = pick(["TOTALOPERATEREVE"])
        deducted_profit = pick(["DEDU_PARENT_PROFIT", "KCFJCXSYJLR"])
        if revenue is None and stock.deducted_net_profit_margin is not None and stock.deducted_net_profit_billion:
            margin = stock.deducted_net_profit_margin
            margin_percent = margin * 100 if -1 <= margin <= 1 else margin
            if margin_percent:
                revenue = stock.deducted_net_profit_billion * 1e8 / (margin_percent / 100)
        if deducted_profit is None and stock.deducted_net_profit_billion is not None:
            deducted_profit = stock.deducted_net_profit_billion * 1e8
        deducted_margin = stock.deducted_net_profit_margin
        if deducted_margin is not None and -1 <= deducted_margin <= 1:
            deducted_margin *= 100
        if deducted_margin is None:
            revenue_value = self._to_float(revenue)
            profit_value = self._to_float(deducted_profit)
            if revenue_value and revenue_value > 0 and profit_value is not None:
                deducted_margin = profit_value / revenue_value * 100

        self._append_indicator(items, "\u8425\u4e1a\u603b\u6536\u5165", revenue, self._format_yi_from_yuan)
        self._append_indicator(
            items,
            "\u603b\u8425\u6536\u540c\u6bd4",
            pick(["TOTALOPERATEREVETZ"]),
            self._format_percent,
            tone=self._growth_tone(pick(["TOTALOPERATEREVETZ"])),
        )
        self._append_indicator(items, "\u5f52\u6bcd\u51c0\u5229\u6da6", pick(["PARENTNETPROFIT"]), self._format_yi_from_yuan)
        self._append_indicator(
            items,
            "\u5f52\u6bcd\u51c0\u5229\u540c\u6bd4",
            pick(["PARENTNETPROFITTZ"]),
            self._format_percent,
            tone=self._growth_tone(pick(["PARENTNETPROFITTZ"])),
        )
        self._append_indicator(
            items,
            "\u6263\u975e\u51c0\u5229\u6da6",
            deducted_profit,
            self._format_yi_from_yuan,
        )
        self._append_indicator(items, "\u6263\u975e\u51c0\u5229\u7387", deducted_margin, self._format_percent)
        deducted_growth = pick(["DPNP_YOY_RATIO", "KCFJCXSYJLRTZ"], stock.deducted_net_profit_growth_rate)
        self._append_indicator(
            items,
            "\u6263\u975e\u51c0\u5229\u540c\u6bd4",
            deducted_growth,
            self._format_percent,
            tone=self._growth_tone(deducted_growth),
        )
        self._append_indicator(items, "\u6bdb\u5229\u7387", pick(["GROSS_PROFIT_RATIO", "XSMLL"]), self._format_percent)
        self._append_indicator(items, "\u51c0\u5229\u7387", pick(["NET_PROFIT_RATIO", "XSJLL"]), self._format_percent)
        self._append_indicator(
            items,
            "\u51c0\u8d44\u4ea7\u6536\u76ca\u7387",
            pick(["ROE_DILUTED", "ROEJQ"], (stock.roe * 100 if stock.roe is not None else None)),
            self._format_percent,
        )
        self._append_indicator(
            items,
            "\u8d44\u4ea7\u8d1f\u503a\u7387",
            pick(["ZCFZL"], self._abstract_value(abstract_values, ["\u8d44\u4ea7\u8d1f\u503a\u7387"])),
            self._format_percent,
        )
        self._append_indicator(items, "\u5e02\u503c", stock.market_cap_billion, self._format_yi)
        self._append_indicator(
            items,
            "\u80a1\u606f\u7387",
            stock.dividend_yield * 100 if stock.dividend_yield is not None else None,
            self._format_percent,
        )
        items.extend(quarterly_eps_items)

        if not items:
            return None
        return FinancialIndicatorSection(
            period=period,
            source=" / ".join(dict.fromkeys(source_parts)) or None,
            items=items,
            notes=notes,
        )

    def get_quarterly_eps_indicators(
        self, code: str
    ) -> tuple[list[FinancialIndicatorItem], str | None, str | None, list[str]]:
        ak = self._import_akshare()
        normalized_code = self._normalize_code(code)
        df, source, period, notes = self._fetch_quarterly_eps_frame(ak, normalized_code)
        items: list[FinancialIndicatorItem] = []
        self._append_quarterly_eps(items, df)
        if not items and not notes:
            notes.append("\u5355\u5b63\u5ea6 EPS \u6570\u636e\u672a\u8fd4\u56de\u53ef\u8bc6\u522b\u5b57\u6bb5\u3002")
        return items, source, period, notes

    def _fetch_quarterly_eps_frame(self, ak, normalized_code: str):
        notes: list[str] = []
        digits = normalized_code.split(".")[0]

        ths_new_fn = getattr(ak, "stock_financial_abstract_new_ths", None)
        if ths_new_fn is not None:
            try:
                with proxy_environment(self.proxy_mode):
                    df = ths_new_fn(symbol=digits, indicator="\u6309\u62a5\u544a\u671f")
                points = self._quarterly_eps_points(df)
                if points:
                    return df, "\u540c\u82b1\u987a\u8d22\u62a5(\u5355\u5b63EPS)", points[0][0], notes
            except Exception as exc:
                notes.append(f"\u540c\u82b1\u987a\u5355\u5b63 EPS \u6682\u4e0d\u53ef\u7528\uff1a{exc}")

        notes.append("东财财报源已禁用。")

        if not notes:
            notes.append("单季度 EPS 信源暂无可用明细；东财财报源已禁用。")
        return None, None, None, notes

    @classmethod
    def _append_quarterly_eps(cls, items: list[FinancialIndicatorItem], df: pd.DataFrame | None) -> None:
        quarterly_points = cls._quarterly_eps_points(df)

        if not quarterly_points:
            return

        values_by_period = {period_key: eps_value for period_key, eps_value in quarterly_points}
        for period_key, eps_value in quarterly_points:
            previous_key = cls._previous_year_period(period_key)
            previous_value = values_by_period.get(previous_key)
            tone = "neutral"
            if previous_value is not None:
                tone = cls._growth_tone(eps_value - previous_value)
            items.append(
                FinancialIndicatorItem(
                    label=f"{period_key} \u6bcf\u80a1\u6536\u76ca",
                    value=cls._format_yuan(eps_value),
                    raw_value=eps_value,
                    unit="\u5143",
                    tone=tone,
                    metric_key="quarterly_eps",
                    period=period_key,
                )
            )

    @classmethod
    def _quarterly_eps_points(cls, df: pd.DataFrame | None) -> list[tuple[str, float]]:
        if df is None or df.empty:
            return []

        quarterly_points: list[tuple[str, float]] = []
        seen_periods: set[str] = set()

        for _, row in df.iterrows():
            eps_value = cls._quarterly_eps_value(row)
            period_key = cls._financial_period_key(row)
            if period_key is None or eps_value is None or period_key in seen_periods:
                continue
            seen_periods.add(period_key)
            quarterly_points.append((period_key, eps_value))
            if len(quarterly_points) >= 12:
                break
        return quarterly_points

    @classmethod
    def _quarterly_eps_value(cls, row) -> Optional[float]:
        metric_name = str(row.get("metric_name") or "").strip().lower()
        if metric_name and metric_name not in {"basic_eps", "epsjb", "eps", "basic_earnings_per_share"}:
            return None
        single_value = cls._to_float(row.get("single"))
        if single_value is not None:
            return single_value
        return cls._to_float(
            cls._row_value(
                row,
                [
                    "EPSJB",
                    "EPS",
                    "BASIC_EPS",
                    "\u57fa\u672c\u6bcf\u80a1\u6536\u76ca",
                    "\u644a\u8584\u6bcf\u80a1\u6536\u76ca(\u5143)",
                    "\u52a0\u6743\u6bcf\u80a1\u6536\u76ca(\u5143)",
                    "value",
                ],
            )
        )

    @staticmethod
    def _previous_year_period(period_key: str) -> str | None:
        if len(period_key) != 6:
            return None
        year_part = period_key[:4]
        if not (year_part.isdigit() and period_key[4] == "Q" and period_key[5].isdigit()):
            return None
        return f"{int(year_part) - 1:04d}Q{period_key[5]}"

    @classmethod
    def _financial_period_key(cls, row) -> str | None:
        report_date = (
            row.get("REPORT_DATE")
            or row.get("report_date")
            or row.get("\u65e5\u671f")
            or row.get("date")
        )
        parsed_date = pd.to_datetime(report_date, errors="coerce")
        if pd.notna(parsed_date):
            month = int(parsed_date.month)
            if month in {3, 6, 9, 12}:
                quarter = (month - 1) // 3 + 1
                return f"{int(parsed_date.year):04d}Q{quarter}"

        season_label = str(
            row.get("SEASON_LABEL")
            or row.get("REPORT_PERIOD")
            or row.get("REPORT_DATE_NAME")
            or row.get("report_name")
            or row.get("quarter_name")
            or row.get("report_period")
            or ""
        ).strip().upper()
        if not season_label:
            return None
        year_text = "".join(char for char in str(report_date or "") if char.isdigit())[:4]
        if len(year_text) != 4:
            year_text = ""
        quarter = None
        if "Q1" in season_label or "1季" in season_label or "一季" in season_label:
            quarter = 1
        elif "Q2" in season_label or "2季" in season_label or "中报" in season_label:
            quarter = 2
        elif "Q3" in season_label or "3季" in season_label or "三季" in season_label:
            quarter = 3
        elif "Q4" in season_label or "4季" in season_label or "年报" in season_label or "四季" in season_label:
            quarter = 4
        if quarter is None or not year_text:
            return None
        return f"{year_text}Q{quarter}"

    def list_relations(self) -> List[StockRelation]:
        items = self.list_stocks()
        by_industry: Dict[str, List[StockItem]] = {}
        for item in items:
            if item.industry and item.industry != "未知行业":
                by_industry.setdefault(item.industry, []).append(item)

        relations: List[StockRelation] = []
        for industry, members in by_industry.items():
            top_members = sorted(
                members,
                key=lambda stock: stock.market_cap_billion or 0,
                reverse=True,
            )[:20]
            for index, source in enumerate(top_members):
                for target in top_members[index + 1 : index + 4]:
                    relations.append(
                        StockRelation(
                            source_code=source.code,
                            target_code=target.code,
                            relation_type="industry_peer",
                            weight=0.5,
                            description=f"同行业：{industry}",
                        )
                    )
        return relations

    def _fetch_spot(self) -> pd.DataFrame:
        ak = self._import_akshare()
        candidates = ["stock_zh_a_spot_em", "stock_zh_a_spot"]
        for name in candidates:
            fn = getattr(ak, name, None)
            if fn:
                with proxy_environment(self.proxy_mode):
                    df = fn()
                if df is not None and not df.empty:
                    return df
        raise RuntimeError("AkShare A 股实时行情不可用")

    def _load_spot(self) -> pd.DataFrame:
        if not self.refresh and os.path.exists(self.cache_path):
            return pd.read_csv(self.cache_path)

        df = self._fetch_spot()
        os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
        df.to_csv(self.cache_path, index=False)
        return df

    @staticmethod
    def _pick_col(df: pd.DataFrame, options: List[str], required: bool = True) -> Optional[str]:
        for col in options:
            if col in df.columns:
                return col
        if not required:
            return None
        raise KeyError(f"AkShare 数据缺少字段：{options}")

    def _row_to_stock(self, row: pd.Series, use_previous_close: bool = False) -> StockItem:
        code_col = self._first_present(row, ["代码", "code", "symbol", "股票代码"])
        name_col = self._first_present(row, ["名称", "name", "股票简称"])
        price_col = self._first_present(row, ["最新价", "price", "现价"])
        pe_col = self._first_present(row, ["市盈率-动态", "市盈率", "pe", "PE"])
        pb_col = self._first_present(row, ["市净率", "pb", "PB"])
        mcap_col = self._first_present(row, ["总市值", "总市值-元", "market_cap"])
        industry_col = self._first_present(row, ["行业", "industry", "板块"])
        previous_col = self._first_present_optional(row, self._previous_close_columns())

        code = self._normalize_code(str(row.get(code_col, "")).strip())
        name = str(row.get(name_col, "")).strip()
        latest_price = self._positive_float(row.get(price_col))
        previous_close = self._positive_float(row.get(previous_col)) if use_previous_close and previous_col else None
        price = previous_close or latest_price or 0.0
        ratio = price / latest_price if latest_price and latest_price > 0 else 1.0
        pe = self._scale_optional(self._to_float(row.get(pe_col)), ratio)
        pb = self._scale_optional(self._to_float(row.get(pb_col)), ratio)
        mcap = self._to_float(row.get(mcap_col))
        industry = str(row.get(industry_col, "未知行业")).strip() or "未知行业"

        market_cap_billion = None
        if mcap is not None:
            market_cap_billion = mcap / 1e8 if mcap > 1e6 else mcap
            market_cap_billion = self._scale_optional(market_cap_billion, ratio)

        is_st = "ST" in name.upper()
        return StockItem(
            code=code,
            name=name or code,
            industry=industry,
            is_st=is_st,
            price=price or 0.0,
            pe=pe,
            pb=pb,
            roe=None,
            market_cap_billion=market_cap_billion,
            dividend_yield=None,
            deducted_net_profit_billion=self._deducted_net_profit_billion_from_row(row),
            deducted_net_profit_margin=self._deducted_net_profit_margin_from_row(row),
            deducted_net_profit_growth_rate=self._deducted_net_profit_growth_rate_from_row(row),
        )

    @staticmethod
    def _to_float(value) -> Optional[float]:
        try:
            if value is None:
                return None
            if isinstance(value, str) and value.strip() in {"", "-", "None"}:
                return None
            result = float(value)
            return result if math.isfinite(result) else None
        except (TypeError, ValueError):
            return None

    @staticmethod
    def _positive_float(value) -> Optional[float]:
        result = AkShareProvider._to_float(value)
        return result if result is not None and result > 0 else None

    @staticmethod
    def _scale_optional(value: Optional[float], ratio: float) -> Optional[float]:
        if value is None:
            return None
        result = value * ratio
        return result if math.isfinite(result) and result >= 0 else None

    @classmethod
    def _deducted_net_profit_billion_from_row(cls, row) -> Optional[float]:
        value = cls._first_float(
            row,
            [
                "deducted_net_profit_billion",
                "扣非净利润_亿",
                "扣非净利润",
                "DEDU_PARENT_PROFIT",
                "KCFJCXSYJLR",
            ],
        )
        if value is None:
            return None
        return value / 1e8 if abs(value) > 1e6 else value

    @classmethod
    def _deducted_net_profit_margin_from_row(cls, row) -> Optional[float]:
        value = cls._first_float(row, ["deducted_net_profit_margin", "扣非净利润率", "扣非净利率"])
        if value is not None:
            return value
        profit = cls._deducted_net_profit_billion_from_row(row)
        revenue = cls._first_float(row, ["revenue_billion", "营业总收入_亿", "营业总收入", "TOTALOPERATEREVE"])
        if profit is None or revenue is None:
            return None
        revenue_billion = revenue / 1e8 if abs(revenue) > 1e6 else revenue
        if revenue_billion <= 0:
            return None
        return profit / revenue_billion * 100

    @classmethod
    def _deducted_net_profit_growth_rate_from_row(cls, row) -> Optional[float]:
        return cls._first_float(
            row,
            [
                "deducted_net_profit_growth_rate",
                "扣非净利润增长率",
                "扣非净利润同比增长率",
                "扣非净利润同比增长",
                "扣非净利润同比",
                "扣非净利润增速",
                "DPNP_YOY_RATIO",
                "KCFJCXSYJLRTZ",
            ],
        )

    @classmethod
    def _first_float(cls, row, keys: list[str]) -> Optional[float]:
        for key in keys:
            value = cls._to_float(row.get(key))
            if value is not None:
                return value
        return None

    @staticmethod
    def _first_present(row: pd.Series, options: List[str]) -> str:
        for col in options:
            if col in row:
                return col
        return options[0]

    @staticmethod
    def _first_present_optional(row: pd.Series, options: List[str]) -> Optional[str]:
        for col in options:
            if col in row:
                return col
        return None

    @staticmethod
    def _has_any_column(df: pd.DataFrame, options: List[str]) -> bool:
        return any(option in df.columns for option in options)

    @staticmethod
    def _previous_close_columns() -> List[str]:
        return ["昨收", "昨收价", "前收盘价", "previous_close", "prev_close", "yesterday_close", "f18"]

    @classmethod
    def _append_indicator(
        cls,
        items: list[FinancialIndicatorItem],
        label: str,
        raw_value,
        formatter,
        unit: str | None = None,
        tone: str = "neutral",
    ) -> None:
        value = cls._to_float(raw_value)
        if value is None:
            return
        items.append(
            FinancialIndicatorItem(
                label=label,
                value=formatter(value),
                raw_value=value,
                unit=unit,
                tone=tone,
            )
        )

    @classmethod
    def _latest_abstract_values(cls, df: pd.DataFrame | None) -> tuple[dict[str, float], str | None]:
        if df is None or df.empty or "\u6307\u6807" not in df.columns:
            return {}, None
        period_columns = [str(column) for column in df.columns if str(column).isdigit()]
        if not period_columns:
            return {}, None
        latest_period = period_columns[0]
        values: dict[str, float] = {}
        for _, row in df.iterrows():
            label = str(row.get("\u6307\u6807") or "").strip()
            value = cls._to_float(row.get(latest_period))
            if label and value is not None and label not in values:
                values[label] = value
        return values, cls._format_period_key(latest_period)

    @staticmethod
    def _abstract_value(values: dict[str, float], labels: list[str]) -> Optional[float]:
        for label in labels:
            if label in values:
                return values[label]
        return None

    @classmethod
    def _row_value(cls, row, keys: list[str], default=None):
        if row is None:
            return default
        for key in keys:
            value = row.get(key)
            if cls._to_float(value) is not None:
                return value
        return default

    @classmethod
    def _growth_tone(cls, value) -> str:
        number = cls._to_float(value)
        if number is None:
            return "neutral"
        if number > 0:
            return "rise"
        if number < 0:
            return "fall"
        return "neutral"

    @classmethod
    def _format_financial_period(cls, report_date, season_label) -> str | None:
        raw = str(report_date or "").strip()
        if not raw:
            return None
        date_part = raw.split(" ")[0]
        season = str(season_label or "").strip()
        return f"{date_part} {season}".strip()

    @staticmethod
    def _format_period_key(value: str) -> str | None:
        raw = str(value or "")
        if len(raw) == 8:
            return f"{raw[:4]}-{raw[4:6]}-{raw[6:8]}"
        return raw or None

    @classmethod
    def _format_decimal(cls, value: float) -> str:
        return cls._trim_number(value, 3)

    @classmethod
    def _format_yuan(cls, value: float) -> str:
        return f"{cls._trim_number(value, 4)}\u5143"

    @classmethod
    def _format_percent(cls, value: float) -> str:
        return f"{cls._trim_number(value, 2)}%"

    @classmethod
    def _format_yi(cls, value: float) -> str:
        return f"{cls._trim_number(value, 2)}\u4ebf"

    @classmethod
    def _format_yi_from_yuan(cls, value: float) -> str:
        return cls._format_yi(value / 100_000_000)

    @staticmethod
    def _trim_number(value: float, digits: int) -> str:
        text = f"{value:.{digits}f}"
        return text.rstrip("0").rstrip(".")

    @staticmethod
    def _normalize_code(code: str) -> str:
        normalized = str(code or "").strip().upper()
        if "." in normalized:
            return normalized
        if normalized.startswith("6"):
            return f"{normalized}.SH"
        if normalized.startswith(("4", "8")):
            return f"{normalized}.BJ"
        if normalized:
            return f"{normalized}.SZ"
        return normalized

    @staticmethod
    def _import_akshare():
        try:
            import akshare as ak
        except ImportError as exc:
            raise RuntimeError("未安装 AkShare，请先执行：pip install akshare") from exc
        return ak
