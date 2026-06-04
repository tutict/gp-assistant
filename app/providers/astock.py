from __future__ import annotations

import math
import os
from datetime import datetime
from typing import List, Optional

import pandas as pd
import requests

from app.providers.akshare import AkShareProvider
from app.providers.base import PROXY_MODE_NONE, StockProvider, normalize_proxy_mode
from app.providers.eastmoney import EastmoneyProvider
from app.schemas import FinancialIndicatorSection, MinuteBar, OrderBookSnapshot, StockItem, StockRelation


class AStockDataProvider(StockProvider):
    name = "astock"

    def __init__(
        self,
        cache_path: Optional[str] = None,
        refresh: bool = False,
        proxy_mode: Optional[str] = None,
    ):
        self.proxy_mode = normalize_proxy_mode(proxy_mode)
        self.refresh = refresh or os.getenv("ASTOCK_REFRESH", "false").lower() == "true"
        self.cache_path = cache_path or os.getenv("ASTOCK_CACHE", "data/cache/astock_stocks.csv")
        self.timeout = float(os.getenv("ASTOCK_TIMEOUT", "10"))
        self._session = requests.Session()
        self._session.trust_env = self.proxy_mode != PROXY_MODE_NONE
        self._session.headers.update({"User-Agent": "Mozilla/5.0"})
        self._eastmoney = EastmoneyProvider(
            cache_path=self.cache_path,
            refresh=self.refresh,
            proxy_mode=self.proxy_mode,
        )
        self._akshare = AkShareProvider(refresh=False, proxy_mode=self.proxy_mode)

    def list_stocks(self) -> List[StockItem]:
        return self._eastmoney.list_stocks()

    def get_stock(self, code: str) -> StockItem:
        normalized_code = self._normalize_code(code)
        try:
            quote = self._tencent_quote([normalized_code]).get(self._code_digits(normalized_code))
        except Exception:
            quote = None
        if quote:
            return StockItem(
                code=normalized_code,
                name=quote.get("name") or normalized_code,
                industry=self._lookup_industry(normalized_code),
                is_st="ST" in str(quote.get("name") or "").upper(),
                price=quote.get("price") or 0.0,
                pe=self._non_negative(quote.get("pe_ttm")),
                pb=self._non_negative(quote.get("pb")),
                roe=None,
                market_cap_billion=self._non_negative(quote.get("mcap_yi")),
                dividend_yield=None,
            )
        return self._eastmoney.get_stock(normalized_code)

    def get_history(self, code: str, start_date: str, end_date: str):
        try:
            df = self._baidu_daily_bars(code, start_date, end_date)
        except Exception:
            df = pd.DataFrame()
        if not df.empty:
            return df
        return self._akshare.get_history(code, start_date, end_date)

    def get_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> List[MinuteBar]:
        return self._akshare.get_minutes(code, start_datetime, end_datetime, period)

    def get_order_book(self, code: str) -> OrderBookSnapshot | None:
        normalized_code = self._normalize_code(code)
        try:
            quote = self._tencent_quote([normalized_code]).get(self._code_digits(normalized_code))
        except Exception:
            quote = None
        if not quote:
            return self._akshare.get_order_book(normalized_code)

        price = quote.get("price")
        bids = [
            level
            for level in (
                self._book_level(index, quote, "bid")
                for index in range(1, 6)
            )
            if level is not None
        ]
        asks = [
            level
            for level in (
                self._book_level(index, quote, "ask")
                for index in range(1, 6)
            )
            if level is not None
        ]
        return OrderBookSnapshot(
            code=normalized_code,
            timestamp=quote.get("timestamp") or datetime.now().isoformat(timespec="seconds"),
            bids=bids,
            asks=asks,
            metrics={
                "最新": price,
                "今开": quote.get("open"),
                "最高": quote.get("high"),
                "最低": quote.get("low"),
                "昨收": quote.get("last_close"),
                "涨跌": quote.get("change_amt"),
                "涨幅": quote.get("change_pct"),
                "换手": quote.get("turnover_pct"),
                "量比": quote.get("vol_ratio"),
            },
        )

    def get_financial_indicators(self, stock: StockItem) -> FinancialIndicatorSection | None:
        return self._akshare.get_financial_indicators(stock)

    def list_relations(self) -> List[StockRelation]:
        return self._eastmoney.list_relations()

    def _tencent_quote(self, codes: list[str]) -> dict[str, dict]:
        prefixed = [self._tencent_symbol(code) for code in codes]
        response = self._session.get(
            "https://qt.gtimg.cn/q=" + ",".join(prefixed),
            timeout=self.timeout,
        )
        response.raise_for_status()
        response.encoding = "gbk"
        result: dict[str, dict] = {}
        for line in response.text.strip().split(";"):
            if not line.strip() or "=" not in line or '"' not in line:
                continue
            key = line.split("=")[0].split("_")[-1]
            values = line.split('"')[1].split("~")
            if len(values) < 53:
                continue
            code = key[2:]
            result[code] = {
                "name": values[1],
                "price": self._to_float(values[3]),
                "last_close": self._to_float(values[4]),
                "open": self._to_float(values[5]),
                "bid1": self._to_float(values[9]),
                "bid1_volume": self._to_float(values[10]),
                "bid2": self._to_float(values[11]),
                "bid2_volume": self._to_float(values[12]),
                "bid3": self._to_float(values[13]),
                "bid3_volume": self._to_float(values[14]),
                "bid4": self._to_float(values[15]),
                "bid4_volume": self._to_float(values[16]),
                "bid5": self._to_float(values[17]),
                "bid5_volume": self._to_float(values[18]),
                "ask1": self._to_float(values[19]),
                "ask1_volume": self._to_float(values[20]),
                "ask2": self._to_float(values[21]),
                "ask2_volume": self._to_float(values[22]),
                "ask3": self._to_float(values[23]),
                "ask3_volume": self._to_float(values[24]),
                "ask4": self._to_float(values[25]),
                "ask4_volume": self._to_float(values[26]),
                "ask5": self._to_float(values[27]),
                "ask5_volume": self._to_float(values[28]),
                "timestamp": self._format_tencent_timestamp(values[30]),
                "change_amt": self._to_float(values[31]),
                "change_pct": self._to_float(values[32]),
                "high": self._to_float(values[33]),
                "low": self._to_float(values[34]),
                "amount_wan": self._to_float(values[37]),
                "turnover_pct": self._to_float(values[38]),
                "pe_ttm": self._to_float(values[39]),
                "amplitude_pct": self._to_float(values[43]),
                "mcap_yi": self._to_float(values[44]),
                "float_mcap_yi": self._to_float(values[45]),
                "pb": self._to_float(values[46]),
                "limit_up": self._to_float(values[47]),
                "limit_down": self._to_float(values[48]),
                "vol_ratio": self._to_float(values[49]),
                "pe_static": self._to_float(values[52]),
            }
        return result

    def _baidu_daily_bars(self, code: str, start_date: str, end_date: str) -> pd.DataFrame:
        response = self._session.get(
            "https://finance.pae.baidu.com/selfselect/getstockquotation",
            params={
                "all": "1",
                "isIndex": "false",
                "isBk": "false",
                "isBlock": "false",
                "isFutures": "false",
                "isStock": "true",
                "newFormat": "1",
                "group": "quotation_kline_ab",
                "finClientType": "pc",
                "code": self._code_digits(code),
                "ktype": "1",
            },
            headers={
                "Accept": "application/vnd.finance-web.v1+json",
                "Origin": "https://gushitong.baidu.com",
                "Referer": "https://gushitong.baidu.com/",
                "User-Agent": "Mozilla/5.0",
            },
            timeout=self.timeout,
        )
        response.raise_for_status()
        market_data = ((response.json().get("Result") or {}).get("newMarketData") or {})
        keys = market_data.get("keys") or []
        rows = [row for row in str(market_data.get("marketData") or "").split(";") if row]
        if not keys or not rows:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])

        start = self._date_key(start_date)
        end = self._date_key(end_date)
        records: list[dict] = []
        for row in rows:
            values = row.split(",")
            item = dict(zip(keys, values))
            date_value = str(item.get("time") or "")
            date_key = date_value.replace("-", "")
            if start and date_key < start:
                continue
            if end and date_key > end:
                continue
            close = self._to_float(item.get("close"))
            if close is None:
                continue
            records.append(
                {
                    "date": date_value,
                    "open": self._to_float(item.get("open")) or close,
                    "high": self._to_float(item.get("high")) or close,
                    "low": self._to_float(item.get("low")) or close,
                    "close": close,
                    "volume": self._to_float(item.get("volume")),
                    "amount": self._to_float(item.get("amount")),
                    "ma5": self._to_float(item.get("ma5avgprice")),
                    "ma10": self._to_float(item.get("ma10avgprice")),
                    "ma20": self._to_float(item.get("ma20avgprice")),
                }
            )
        return pd.DataFrame(records)

    def _lookup_industry(self, normalized_code: str) -> str:
        try:
            return self._eastmoney.get_stock(normalized_code).industry
        except Exception:
            return "未知行业"

    @staticmethod
    def _book_level(level: int, quote: dict, side: str):
        from app.schemas import OrderBookLevel

        price = quote.get(f"{side}{level}")
        volume = quote.get(f"{side}{level}_volume")
        if price is None and volume is None:
            return None
        return OrderBookLevel(level=level, price=price, volume=volume)

    @staticmethod
    def _date_key(value: str) -> str:
        return str(value or "").replace("-", "")[:8]

    @staticmethod
    def _format_tencent_timestamp(value: str) -> Optional[str]:
        raw = str(value or "").strip()
        if len(raw) < 14:
            return raw or None
        try:
            return datetime.strptime(raw[:14], "%Y%m%d%H%M%S").isoformat(timespec="seconds")
        except ValueError:
            return raw

    @staticmethod
    def _tencent_symbol(code: str) -> str:
        normalized = AStockDataProvider._normalize_code(code)
        digits = AStockDataProvider._code_digits(normalized)
        if normalized.endswith(".SH") or digits.startswith(("6", "9")):
            return f"sh{digits}"
        if normalized.endswith(".BJ") or digits.startswith("8"):
            return f"bj{digits}"
        return f"sz{digits}"

    @staticmethod
    def _code_digits(code: str) -> str:
        normalized = str(code or "").strip().upper()
        if "." in normalized:
            return normalized.split(".")[0]
        if normalized.startswith(("SH", "SZ", "BJ")):
            return normalized[2:]
        return normalized

    @staticmethod
    def _normalize_code(code: str) -> str:
        digits = AStockDataProvider._code_digits(code)
        if digits.startswith("6"):
            return f"{digits}.SH"
        if digits.startswith(("4", "8")):
            return f"{digits}.BJ"
        if digits:
            return f"{digits}.SZ"
        return digits

    @staticmethod
    def _non_negative(value) -> Optional[float]:
        result = AStockDataProvider._to_float(value)
        return result if result is not None and result >= 0 else None

    @staticmethod
    def _to_float(value) -> Optional[float]:
        try:
            if value is None:
                return None
            if isinstance(value, str) and value.strip() in {"", "-", "None", "nan"}:
                return None
            result = float(value)
            return result if math.isfinite(result) else None
        except (TypeError, ValueError):
            return None
