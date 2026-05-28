import os
import math
from datetime import datetime
from typing import Dict, List, Optional

import pandas as pd

from app.providers.base import StockProvider
from app.schemas import MinuteBar, OrderBookLevel, OrderBookSnapshot, StockItem, StockRelation


class AkShareProvider(StockProvider):
    name = "akshare"

    def __init__(self, cache_path: Optional[str] = None, refresh: bool = False):
        self.cache_path = cache_path or os.getenv("AKSHARE_CACHE", "data/cache/stocks.csv")
        self.refresh = refresh or os.getenv("AKSHARE_REFRESH", "false").lower() == "true"

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
        raise KeyError(f"Stock {code} not found")

    def get_history(self, code: str, start_date: str, end_date: str):
        ak = self._import_akshare()
        history_fn = getattr(ak, "stock_zh_a_hist", None)
        if history_fn is None:
            raise RuntimeError("AkShare missing stock_zh_a_hist")

        symbol = code.split(".")[0]
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
            raise RuntimeError("AkShare missing stock_zh_a_hist_min_em")

        period = str(period or "1")
        if period not in {"1", "5", "15", "30", "60"}:
            period = "1"
        symbol = self._normalize_code(code).split(".")[0]
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
            raise RuntimeError("AkShare missing stock_bid_ask_em")

        normalized_code = self._normalize_code(code)
        symbol = normalized_code.split(".")[0]
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

    def list_relations(self) -> List[StockRelation]:
        items = self.list_stocks()
        by_industry: Dict[str, List[StockItem]] = {}
        for item in items:
            if item.industry and item.industry != "Unknown":
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
                            description=f"Same industry: {industry}",
                        )
                    )
        return relations

    def _fetch_spot(self) -> pd.DataFrame:
        ak = self._import_akshare()
        candidates = ["stock_zh_a_spot_em", "stock_zh_a_spot"]
        for name in candidates:
            fn = getattr(ak, name, None)
            if fn:
                df = fn()
                if df is not None and not df.empty:
                    return df
        raise RuntimeError("AkShare spot data unavailable")

    @staticmethod
    def _pick_col(df: pd.DataFrame, options: List[str], required: bool = True) -> Optional[str]:
        for col in options:
            if col in df.columns:
                return col
        if not required:
            return None
        raise KeyError(f"Column not found in AkShare data: {options}")

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
        industry = str(row.get(industry_col, "Unknown")).strip() or "Unknown"

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
            raise RuntimeError("AkShare not installed. Run: pip install akshare") from exc
        return ak
