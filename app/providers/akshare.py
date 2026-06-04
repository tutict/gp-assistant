import os
import math
from datetime import datetime
from typing import Dict, List, Optional

import pandas as pd

from app.providers.base import StockProvider, normalize_proxy_mode, proxy_environment
from app.schemas import (
    FinancialIndicatorItem,
    FinancialIndicatorSection,
    MinuteBar,
    OrderBookLevel,
    OrderBookSnapshot,
    StockItem,
    StockRelation,
)


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
        if not self.refresh and os.path.exists(self.cache_path):
            df = pd.read_csv(self.cache_path)
            return [self._row_to_stock(row) for _, row in df.iterrows()]

        df = self._fetch_spot()
        os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
        df.to_csv(self.cache_path, index=False)
        return [self._row_to_stock(row) for _, row in df.iterrows()]

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

        indicator_fn = getattr(ak, "stock_financial_analysis_indicator_em", None)
        if indicator_fn is not None:
            try:
                with proxy_environment(self.proxy_mode):
                    df = indicator_fn(symbol=normalized_code, indicator="\u6309\u62a5\u544a\u671f")
                if df is not None and not df.empty:
                    row = df.iloc[0]
                    period = self._format_financial_period(row.get("REPORT_DATE"), row.get("SEASON_LABEL"))
                    source_parts.append("\u4e1c\u8d22F10")
            except Exception as exc:
                notes.append(f"\u4e1c\u8d22F10\u6307\u6807\u6682\u4e0d\u53ef\u7528\uff1a{exc}")

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

        items: list[FinancialIndicatorItem] = []
        if stock.pe is not None or stock.pb is not None or stock.market_cap_billion is not None:
            source_parts.append("\u884c\u60c5\u4f30\u503c")
        self._append_indicator(items, "\u5e02\u76c8\u7387(TTM)", stock.pe, self._format_decimal)
        self._append_indicator(items, "\u5e02\u51c0\u7387(\u6700\u65b0)", stock.pb, self._format_decimal)

        pick = lambda keys, default=None: self._row_value(row, keys, default)
        self._append_indicator(items, "\u6bcf\u80a1\u6536\u76ca(\u8ba1\u7b97)", pick(["EPSJB"]), self._format_yuan)
        self._append_indicator(items, "\u6bcf\u80a1\u51c0\u8d44\u4ea7", pick(["BPS"]), self._format_yuan)
        self._append_indicator(items, "\u8425\u4e1a\u603b\u6536\u5165", pick(["TOTALOPERATEREVE"]), self._format_yi_from_yuan)
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
            pick(["DEDU_PARENT_PROFIT", "KCFJCXSYJLR"]),
            self._format_yi_from_yuan,
        )
        self._append_indicator(
            items,
            "\u6263\u975e\u51c0\u5229\u540c\u6bd4",
            pick(["DPNP_YOY_RATIO", "KCFJCXSYJLRTZ"]),
            self._format_percent,
            tone=self._growth_tone(pick(["DPNP_YOY_RATIO", "KCFJCXSYJLRTZ"])),
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

        if not items:
            return None
        return FinancialIndicatorSection(
            period=period,
            source=" / ".join(dict.fromkeys(source_parts)) or None,
            items=items,
            notes=notes,
        )

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

    @staticmethod
    def _pick_col(df: pd.DataFrame, options: List[str], required: bool = True) -> Optional[str]:
        for col in options:
            if col in df.columns:
                return col
        if not required:
            return None
        raise KeyError(f"AkShare 数据缺少字段：{options}")

    def _row_to_stock(self, row: pd.Series) -> StockItem:
        code_col = self._first_present(row, ["代码", "code", "symbol", "股票代码"])
        name_col = self._first_present(row, ["名称", "name", "股票简称"])
        price_col = self._first_present(row, ["最新价", "price", "现价"])
        pe_col = self._first_present(row, ["市盈率-动态", "市盈率", "pe", "PE"])
        pb_col = self._first_present(row, ["市净率", "pb", "PB"])
        mcap_col = self._first_present(row, ["总市值", "总市值-元", "market_cap"])
        industry_col = self._first_present(row, ["行业", "industry", "板块"])

        code = self._normalize_code(str(row.get(code_col, "")).strip())
        name = str(row.get(name_col, "")).strip()
        price = self._to_float(row.get(price_col))
        pe = self._to_float(row.get(pe_col))
        pb = self._to_float(row.get(pb_col))
        mcap = self._to_float(row.get(mcap_col))
        industry = str(row.get(industry_col, "未知行业")).strip() or "未知行业"

        market_cap_billion = None
        if mcap is not None:
            market_cap_billion = mcap / 1e8 if mcap > 1e6 else mcap

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
    def _first_present(row: pd.Series, options: List[str]) -> str:
        for col in options:
            if col in row:
                return col
        return options[0]

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
