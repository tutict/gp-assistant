from __future__ import annotations

import math
import os
from datetime import datetime
from typing import List, Optional

import pandas as pd
import requests

from app.providers.akshare import AkShareProvider
from app.providers.base import PROXY_MODE_NONE, StockProvider, env_float, env_int, normalize_proxy_mode
from app.providers.eastmoney import EastmoneyProvider
from app.providers.tencent import TencentQuoteClient
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
        self.timeout = env_float("ASTOCK_TIMEOUT", 10, minimum=0.1)
        self._session = requests.Session()
        self._session.trust_env = self.proxy_mode != PROXY_MODE_NONE
        self._session.headers.update({"User-Agent": "Mozilla/5.0"})
        self._tencent = TencentQuoteClient(
            self._session,
            self.timeout,
            env_int("ASTOCK_TENCENT_BATCH_SIZE", 80, minimum=1),
        )
        self._eastmoney = EastmoneyProvider(
            cache_path=self.cache_path,
            refresh=self.refresh,
            proxy_mode=self.proxy_mode,
        )
        self._akshare = AkShareProvider(refresh=False, proxy_mode=self.proxy_mode)

    def list_stocks(self) -> List[StockItem]:
        return self._eastmoney.list_stocks()

    def list_stocks_for_screen(self) -> tuple[List[StockItem], List[str]]:
        try:
            items = self.list_stocks()
        except Exception as exc:
            return [], [f"股票池不可用，腾讯行情无法枚举全市场代码：{exc}"]

        codes = [item.code for item in items]
        quotes, failed_batches = self._tencent_quotes_batched(codes)
        missing_codes = [
            item.code
            for item in items
            if self._positive_float((quotes.get(self._code_digits(item.code)) or {}).get("last_close")) is None
        ]
        tdx_quotes, failed_tdx_batches, tdx_note = self._tdx_quotes_batched(missing_codes)
        if not quotes and not tdx_quotes:
            notes = ["前一交易日收盘价不可用，已回退到股票池价格：A股全栈/腾讯与通达信行情不可用"]
            if failed_batches:
                notes.append(f"腾讯批量行情失败 {failed_batches} 批。")
            if failed_tdx_batches:
                notes.append(f"通达信批量行情失败 {failed_tdx_batches} 批。")
            if tdx_note:
                notes.append(tdx_note)
            return items, notes

        updated_items: List[StockItem] = []
        tencent_previous_close_count = 0
        tdx_previous_close_count = 0
        fallback_count = 0
        for item in items:
            quote = quotes.get(self._code_digits(item.code))
            updated = self._stock_with_previous_close_quote(item, quote) if quote else None
            if updated is not None:
                tencent_previous_close_count += 1
                updated_items.append(updated)
                continue

            tdx_quote = tdx_quotes.get(self._code_digits(item.code))
            updated = self._stock_with_previous_close_quote(item, tdx_quote) if tdx_quote else None
            if updated is None:
                fallback_count += 1
                updated_items.append(item)
                continue
            tdx_previous_close_count += 1
            updated_items.append(updated)

        notes = [
            "筛选价格口径：前一交易日收盘价（腾讯昨收优先，通达信补充），"
            f"腾讯 {tencent_previous_close_count} 只，通达信 {tdx_previous_close_count} 只。"
        ]
        if fallback_count:
            notes.append(f"{fallback_count} 只股票缺少腾讯/通达信昨收价，已回退到股票池价格。")
        if failed_batches:
            notes.append(f"腾讯批量行情失败 {failed_batches} 批，其余股票已继续处理。")
        if failed_tdx_batches:
            notes.append(f"通达信批量行情失败 {failed_tdx_batches} 批，其余股票已继续处理。")
        if tdx_note:
            notes.append(tdx_note)
        return updated_items, notes

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

    def _tencent_quotes_batched(self, codes: list[str]) -> tuple[dict[str, dict], int]:
        return self._tencent.quotes_batched(codes, self._tencent_quote)

    def _tdx_quotes_batched(self, codes: list[str]) -> tuple[dict[str, dict], int, str | None]:
        enabled = os.getenv("ASTOCK_TDX_ENABLED", "true").strip().lower() not in {"0", "false", "no", "off"}
        if not enabled or not codes:
            return {}, 0, None

        try:
            from pytdx.hq import TdxHq_API
        except Exception:
            return {}, 0, "通达信补充行情未启用：请安装 pytdx。"

        batch_size = env_int("ASTOCK_TDX_BATCH_SIZE", 80, minimum=1)
        normalized_codes = [self._normalize_code(code) for code in codes if self._tdx_market_code(code) is not None]
        if not normalized_codes:
            return {}, 0, None

        last_error: str | None = None
        timeout = env_float("ASTOCK_TDX_TIMEOUT", 3, minimum=0.1)
        for host, port in self._tdx_hosts():
            api = TdxHq_API(raise_exception=True, auto_retry=False)
            try:
                with api.connect(host, port, time_out=timeout):
                    quotes: dict[str, dict] = {}
                    failed_batches = 0
                    for index in range(0, len(normalized_codes), batch_size):
                        batch = normalized_codes[index : index + batch_size]
                        query = [
                            (market, self._code_digits(code))
                            for code in batch
                            if (market := self._tdx_market_code(code)) is not None
                        ]
                        if not query:
                            continue
                        try:
                            for quote in api.get_security_quotes(query) or []:
                                parsed = self._tdx_quote_from_raw(quote)
                                if parsed:
                                    quotes[parsed["code"]] = parsed
                        except Exception:
                            failed_batches += 1
                    return quotes, failed_batches, None
            except Exception as exc:
                last_error = str(exc)
                continue
        note = "通达信补充行情不可用。"
        if last_error:
            note = f"{note} 最近错误：{last_error}"
        return {}, 1, note

    def _stock_with_previous_close_quote(self, item: StockItem, quote: Optional[dict]) -> StockItem | None:
        if not quote:
            return None
        previous_close = self._positive_float(quote.get("last_close"))
        if previous_close is None:
            return None

        latest_price = self._positive_float(quote.get("price")) or self._positive_float(item.price)
        ratio = previous_close / latest_price if latest_price and latest_price > 0 else 1.0
        fallback_ratio = previous_close / item.price if item.price > 0 else 1.0
        pe = self._scale_optional(self._non_negative(quote.get("pe_ttm")), ratio)
        pb = self._scale_optional(self._non_negative(quote.get("pb")), ratio)
        market_cap_billion = self._scale_optional(self._non_negative(quote.get("mcap_yi")), ratio)

        if pe is None:
            pe = self._scale_optional(item.pe, fallback_ratio)
        if pb is None:
            pb = self._scale_optional(item.pb, fallback_ratio)
        if market_cap_billion is None:
            market_cap_billion = self._scale_optional(item.market_cap_billion, fallback_ratio)

        return item.model_copy(
            update={
                "name": quote.get("name") or item.name,
                "is_st": "ST" in str(quote.get("name") or item.name).upper(),
                "price": previous_close,
                "pe": pe,
                "pb": pb,
                "market_cap_billion": market_cap_billion,
            }
        )

    @staticmethod
    def _tdx_hosts() -> list[tuple[str, int]]:
        env_hosts = os.getenv("ASTOCK_TDX_HOSTS", "").strip()
        if env_hosts:
            hosts: list[tuple[str, int]] = []
            for item in env_hosts.split(","):
                host, _, port = item.strip().partition(":")
                if not host:
                    continue
                hosts.append((host, int(port or "7709")))
            if hosts:
                return hosts
        return [
            ("119.147.212.81", 7709),
            ("101.227.73.20", 7709),
            ("218.6.170.47", 7709),
        ]

    @staticmethod
    def _tdx_market_code(code: str) -> Optional[int]:
        normalized = AStockDataProvider._normalize_code(code)
        digits = AStockDataProvider._code_digits(normalized)
        if normalized.endswith(".SH") or digits.startswith(("5", "6", "9")):
            return 1
        if normalized.endswith(".SZ") or digits.startswith(("0", "2", "3")):
            return 0
        return None

    @classmethod
    def _tdx_quote_from_raw(cls, quote: dict) -> dict | None:
        code = str(quote.get("code") or "").strip()
        if not code:
            return None
        return {
            "code": code,
            "name": quote.get("name"),
            "price": cls._to_float(quote.get("price")),
            "last_close": cls._to_float(quote.get("last_close") or quote.get("pre_close")),
            "open": cls._to_float(quote.get("open")),
            "high": cls._to_float(quote.get("high")),
            "low": cls._to_float(quote.get("low")),
        }

    def _tencent_quote(self, codes: list[str]) -> dict[str, dict]:
        return self._tencent.quote(codes)

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
    def _positive_float(value) -> Optional[float]:
        result = AStockDataProvider._to_float(value)
        return result if result is not None and result > 0 else None

    @staticmethod
    def _scale_optional(value: Optional[float], ratio: float) -> Optional[float]:
        if value is None:
            return None
        result = value * ratio
        return result if math.isfinite(result) and result >= 0 else None

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
