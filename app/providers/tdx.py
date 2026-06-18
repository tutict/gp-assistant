from __future__ import annotations

import os
import math
from datetime import date, datetime, time, timedelta, timezone
from typing import List, Optional

import pandas as pd
import requests

from app.providers.akshare import AkShareProvider
from app.providers.base import StockProvider, env_float, env_int, normalize_proxy_mode
from app.providers.tencent import TencentQuoteClient
from app.schemas import (
    FinancialIndicatorItem,
    FinancialIndicatorSection,
    MinuteBar,
    OrderBookLevel,
    OrderBookSnapshot,
    StockItem,
    StockRelation,
)


class TdxProvider(StockProvider):
    name = "tdx"
    _SCREEN_MARKET_CLOSE = time(15, 0)
    _CHINA_TZ = timezone(timedelta(hours=8))

    def __init__(
        self,
        cache_path: Optional[str] = None,
        refresh: bool = False,
        proxy_mode: Optional[str] = None,
    ):
        self.cache_path = cache_path or os.getenv("TDX_CACHE", "data/cache/tdx_stocks.csv")
        self.refresh = refresh or os.getenv("TDX_REFRESH", "false").lower() == "true"
        self.timeout = env_float("TDX_TIMEOUT", 6, minimum=0.1)
        self.page_size = env_int("TDX_PAGE_SIZE", 1000, minimum=100)
        self.quote_batch_size = env_int("TDX_QUOTE_BATCH_SIZE", 80, minimum=1)
        self.tencent_batch_size = env_int("TDX_TENCENT_BATCH_SIZE", 80, minimum=1)
        self.history_batch_size = env_int("TDX_HISTORY_BATCH_SIZE", 800, minimum=1, maximum=800)
        self.history_pages = env_int("TDX_HISTORY_PAGES", 8, minimum=1)
        self.fundamental_page_size = env_int("TDX_FUNDAMENTAL_PAGE_SIZE", 500, minimum=50, maximum=1000)
        self.fundamental_max_pages = env_int("TDX_FUNDAMENTAL_MAX_PAGES", 30, minimum=1)
        self.fundamental_min_rows = env_int("TDX_FUNDAMENTAL_MIN_ROWS", 1000, minimum=1)
        self.proxy_mode = normalize_proxy_mode(proxy_mode)
        self.fundamental_cache_path = self._resolve_fundamental_cache_path()
        self._session = requests.Session()
        self._session.trust_env = False
        self._session.headers.update({"User-Agent": "Mozilla/5.0"})
        self._tencent = TencentQuoteClient(self._session, self.timeout, self.tencent_batch_size)

    def list_stocks(self) -> List[StockItem]:
        cached = self._read_cached_universe()
        if cached is not None and not self.refresh:
            return cached

        items = self._fetch_universe()
        self._write_cached_universe(items)
        return items

    def list_stocks_for_screen(self) -> tuple[List[StockItem], List[str]]:
        try:
            items = self.list_stocks()
        except Exception as exc:
            return [], [f"通达信股票池不可用：{exc}"]

        price_field, price_policy = self._screen_price_policy()
        tencent_quotes, failed_tencent_batches = self._tencent_quotes_batched([item.code for item in items])
        missing_codes = [
            item.code
            for item in items
            if self._screen_quote_price(tencent_quotes.get(self._code_digits(item.code)), price_field) is None
        ]
        tdx_quotes, failed_tdx_batches, tdx_note = self._quotes_batched(missing_codes)
        fundamentals, fundamental_note = self._cached_fundamentals_for_screen()
        updated_items: list[StockItem] = []
        tencent_quoted_count = 0
        tdx_quoted_count = 0
        fallback_count = 0
        fundamental_price_count = 0
        fundamental_count = 0
        estimated_roe_count = 0
        for item in items:
            digits = self._code_digits(item.code)
            quote = tencent_quotes.get(digits)
            updated = self._stock_with_quote(item, quote, price_field)
            quote_source = "tencent" if updated is not None else None
            if updated is None:
                quote = tdx_quotes.get(digits)
                updated = self._stock_with_quote(item, quote, price_field)
                quote_source = "tdx" if updated is not None else None
            if updated is None:
                fallback_count += 1
                updated = item
            elif quote_source == "tencent":
                tencent_quoted_count += 1
            elif quote_source == "tdx":
                tdx_quoted_count += 1

            fundamental = fundamentals.get(updated.code)
            updated, used_fundamental, estimated_roe, used_fundamental_price = self._stock_with_fundamentals(
                updated,
                fundamental,
                prefer_fundamental_price=quote_source is None,
            )
            if used_fundamental:
                fundamental_count += 1
            if estimated_roe:
                estimated_roe_count += 1
            if used_fundamental_price:
                fundamental_price_count += 1
            updated_items.append(updated)

        notes = [
            f"筛选价格口径：{price_policy}（腾讯优先，通达信补充），"
            f"腾讯 {tencent_quoted_count} 只，通达信 {tdx_quoted_count} 只。"
        ]
        if fallback_count:
            notes.append(
                f"{fallback_count} 只股票缺少腾讯/通达信可用价格，其中 {fundamental_price_count} 只已回退到本地基础指标价格，其余回退到股票池缓存价格。"
            )
        if fundamental_count:
            notes.append(f"基础指标补充：腾讯行情估值字段和本地行业/估值缓存，已合并 {fundamental_count} 只。")
        if estimated_roe_count:
            notes.append(f"{estimated_roe_count} 只股票缺少直接 ROE，已用 市净率 / 市盈率 估算 ROE。")
        if fundamental_note:
            notes.append(fundamental_note)
        if failed_tencent_batches:
            notes.append(f"腾讯批量行情失败 {failed_tencent_batches} 批，其余股票已继续处理。")
        if failed_tdx_batches:
            notes.append(f"通达信批量行情失败 {failed_tdx_batches} 批，其余股票已继续处理。")
        if tdx_note:
            notes.append(tdx_note)
        return updated_items, notes

    def get_stock(self, code: str) -> StockItem:
        normalized_code = self._normalize_code(code)
        base = self._find_cached_stock(normalized_code)
        quote = None
        try:
            quote = self._tencent_quote([normalized_code]).get(self._code_digits(normalized_code))
        except Exception:
            quote = None
        if quote is None:
            try:
                quote = self._quotes_batched([normalized_code])[0].get(self._code_digits(normalized_code))
            except Exception:
                quote = None
        updated = self._stock_with_quote(base, quote, "price")
        stock = updated or base
        fundamentals, _ = self._cached_fundamentals_for_screen()
        enriched, _, _, _ = self._stock_with_fundamentals(stock, fundamentals.get(stock.code))
        return enriched

    def get_history(self, code: str, start_date: str, end_date: str):
        try:
            fast_history = self._eastmoney_history(code, start_date, end_date)
            if not fast_history.empty:
                return fast_history
        except Exception:
            pass
        try:
            tencent_history = self._tencent_history(code, start_date, end_date)
            if not tencent_history.empty:
                return tencent_history
        except Exception:
            pass

        market = self._tdx_market_code(code)
        if market is None:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])

        digits = self._code_digits(code)
        start_key = self._date_key(start_date)
        end_key = self._date_key(end_date)
        rows: list[dict] = []
        oldest_key: str | None = None

        def fetch(api):
            nonlocal oldest_key
            for page in range(self.history_pages):
                offset = page * self.history_batch_size
                bars = api.get_security_bars(4, market, digits, offset, self.history_batch_size) or []
                if not bars:
                    break
                for bar in bars:
                    row = self._history_row(bar)
                    date_key = self._date_key(row["date"])
                    oldest_key = date_key if oldest_key is None else min(oldest_key, date_key)
                    if start_key and date_key < start_key:
                        continue
                    if end_key and date_key > end_key:
                        continue
                    rows.append(row)
                if start_key and oldest_key and oldest_key <= start_key:
                    break

        self._with_connected_api(
            fetch,
            host_limit=self._observe_fallback_host_limit(),
            timeout=self._observe_fallback_timeout(),
        )
        if not rows:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])
        return pd.DataFrame(rows).drop_duplicates(subset=["date"]).sort_values("date").reset_index(drop=True)

    def get_chip_distribution(self, code: str):
        return AkShareProvider(proxy_mode=self.proxy_mode).get_chip_distribution(code)

    def get_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> List[MinuteBar]:
        try:
            fast_bars = self._eastmoney_minutes(code, start_datetime, end_datetime, period)
            if fast_bars:
                return fast_bars
        except Exception:
            pass
        try:
            tencent_bars = self._tencent_minutes(code, start_datetime, end_datetime, period)
            if tencent_bars:
                return tencent_bars
        except Exception:
            pass

        market = self._tdx_market_code(code)
        if market is None:
            return []

        category = {"1": 8, "5": 0, "15": 1, "30": 2, "60": 3}.get(str(period or "1"), 8)
        digits = self._code_digits(code)
        start_key = self._datetime_key(start_datetime)
        end_key = self._datetime_key(end_datetime)
        bars: list[MinuteBar] = []

        def fetch(api):
            raw_bars = api.get_security_bars(category, market, digits, 0, 800) or []
            for bar in raw_bars:
                row = self._history_row(bar)
                key = self._datetime_key(row["datetime"])
                if start_key and key < start_key:
                    continue
                if end_key and key > end_key:
                    continue
                bars.append(
                    MinuteBar(
                        datetime=row["datetime"],
                        open=row["open"],
                        high=row["high"],
                        low=row["low"],
                        close=row["close"],
                        volume=row.get("volume"),
                        amount=row.get("amount"),
                    )
                )

        self._with_connected_api(
            fetch,
            host_limit=self._observe_fallback_host_limit(),
            timeout=self._observe_fallback_timeout(),
        )
        return bars

    def _eastmoney_history(self, code: str, start_date: str, end_date: str) -> pd.DataFrame:
        digits = self._code_digits(code)
        market = self._eastmoney_market_code(code)
        if not digits or market is None:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])
        params = {
            "fields1": "f1,f2,f3,f4,f5,f6",
            "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
            "ut": "7eea3edcaed734bea9cbfc24409ed989",
            "klt": "101",
            "fqt": "0",
            "secid": f"{market}.{digits}",
            "beg": self._date_key(start_date) or "0",
            "end": self._date_key(end_date) or "20500000",
        }
        response = self._session.get(
            "https://push2his.eastmoney.com/api/qt/stock/kline/get",
            params=params,
            timeout=self._eastmoney_timeout(),
        )
        response.raise_for_status()
        data = response.json().get("data") or {}
        rows = [self._eastmoney_kline_row(item) for item in data.get("klines") or []]
        rows = [row for row in rows if row is not None]
        if not rows:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])
        return pd.DataFrame(rows).sort_values("date").reset_index(drop=True)

    def _eastmoney_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> list[MinuteBar]:
        period = str(period or "1")
        if period not in {"1", "5", "15", "30", "60"}:
            period = "1"
        digits = self._code_digits(code)
        market = self._eastmoney_market_code(code)
        if not digits or market is None:
            return []
        if period == "1":
            rows = self._eastmoney_minute_trends(market, digits)
        else:
            rows = self._eastmoney_minute_klines(market, digits, period)
        start_key = self._datetime_key(start_datetime)
        end_key = self._datetime_key(end_datetime)
        return [
            row
            for row in rows
            if (not start_key or self._datetime_key(row.datetime) >= start_key)
            and (not end_key or self._datetime_key(row.datetime) <= end_key)
        ]

    def _eastmoney_minute_trends(self, market: int, digits: str) -> list[MinuteBar]:
        response = self._session.get(
            "https://push2his.eastmoney.com/api/qt/stock/trends2/get",
            params={
                "fields1": "f1,f2,f3,f4,f5,f6,f7,f8,f9,f10,f11,f12,f13",
                "fields2": "f51,f52,f53,f54,f55,f56,f57,f58",
                "ut": "7eea3edcaed734bea9cbfc24409ed989",
                "ndays": "5",
                "iscr": "0",
                "secid": f"{market}.{digits}",
            },
            timeout=self._eastmoney_timeout(),
        )
        response.raise_for_status()
        data = response.json().get("data") or {}
        rows = [self._eastmoney_minute_row(item) for item in data.get("trends") or []]
        return [row for row in rows if row is not None]

    def _eastmoney_minute_klines(self, market: int, digits: str, period: str) -> list[MinuteBar]:
        response = self._session.get(
            "https://push2his.eastmoney.com/api/qt/stock/kline/get",
            params={
                "fields1": "f1,f2,f3,f4,f5,f6",
                "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
                "ut": "7eea3edcaed734bea9cbfc24409ed989",
                "klt": period,
                "fqt": "0",
                "secid": f"{market}.{digits}",
                "beg": "0",
                "end": "20500000",
            },
            timeout=self._eastmoney_timeout(),
        )
        response.raise_for_status()
        data = response.json().get("data") or {}
        rows = [self._eastmoney_minute_row(item) for item in data.get("klines") or []]
        return [row for row in rows if row is not None]

    def _tencent_history(self, code: str, start_date: str, end_date: str) -> pd.DataFrame:
        symbol = TencentQuoteClient.tencent_symbol(code)
        if not symbol:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])
        start_key = self._date_key(start_date)
        end_key = self._date_key(end_date)
        start_param = self._tencent_date(start_date)
        end_param = self._tencent_date(end_date)
        response = self._session.get(
            "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get",
            params={"param": f"{symbol},day,{start_param},{end_param},320,"},
            timeout=self._tencent_timeout(),
        )
        response.raise_for_status()
        data = response.json().get("data") or {}
        rows = [
            self._tencent_kline_row(item)
            for item in (data.get(symbol) or {}).get("day")
            or []
        ]
        rows = [
            row
            for row in rows
            if row is not None
            and (not start_key or self._date_key(row["date"]) >= start_key)
            and (not end_key or self._date_key(row["date"]) <= end_key)
        ]
        if not rows:
            return pd.DataFrame(columns=["date", "open", "high", "low", "close", "volume", "amount"])
        return pd.DataFrame(rows).sort_values("date").reset_index(drop=True)

    def _tencent_minutes(
        self,
        code: str,
        start_datetime: str,
        end_datetime: str,
        period: str = "1",
    ) -> list[MinuteBar]:
        period = str(period or "1")
        if period not in {"1", "5", "15", "30", "60"}:
            period = "1"
        symbol = TencentQuoteClient.tencent_symbol(code)
        if not symbol:
            return []
        key = f"m{period}"
        response = self._session.get(
            "https://ifzq.gtimg.cn/appstock/app/kline/mkline",
            params={"param": f"{symbol},{key},,{self._tencent_minute_count()}"},
            timeout=self._tencent_timeout(),
        )
        response.raise_for_status()
        data = response.json().get("data") or {}
        rows = [
            self._tencent_minute_row(item)
            for item in (data.get(symbol) or {}).get(key)
            or []
        ]
        start_key = self._datetime_key(start_datetime)
        end_key = self._datetime_key(end_datetime)
        return [
            row
            for row in rows
            if row is not None
            and (not start_key or self._datetime_key(row.datetime) >= start_key)
            and (not end_key or self._datetime_key(row.datetime) <= end_key)
        ]

    def get_order_book(self, code: str) -> OrderBookSnapshot | None:
        normalized_code = self._normalize_code(code)
        try:
            quote = self._tencent_quote([normalized_code]).get(self._code_digits(normalized_code))
        except Exception:
            quote = None
        if not quote:
            quote = self._quotes_batched([normalized_code])[0].get(self._code_digits(normalized_code))
        if not quote:
            return None

        bids = [level for level in (self._book_level(index, quote, "bid") for index in range(1, 6)) if level]
        asks = [level for level in (self._book_level(index, quote, "ask") for index in range(1, 6)) if level]
        return OrderBookSnapshot(
            code=normalized_code,
            timestamp=quote.get("timestamp") or datetime.now().isoformat(timespec="seconds"),
            bids=bids,
            asks=asks,
            metrics={
                "最新": quote.get("price"),
                "今开": quote.get("open"),
                "最高": quote.get("high"),
                "最低": quote.get("low"),
                "昨收": quote.get("last_close"),
                "成交量": quote.get("volume"),
                "成交额": quote.get("amount"),
            },
        )

    def get_financial_indicators(self, stock: StockItem) -> FinancialIndicatorSection | None:
        items: list[FinancialIndicatorItem] = []
        source_parts = ["\u817e\u8baf/\u901a\u8fbe\u4fe1/\u672c\u5730\u7f13\u5b58"]
        estimated_roe = False
        roe = stock.roe
        if roe is None and stock.pe and stock.pb:
            roe = stock.pb / stock.pe
            estimated_roe = True

        def add(label: str, raw_value, formatter, unit: str | None = None, tone: str = "neutral") -> None:
            value = self._to_float(raw_value)
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

        add("市盈率(TTM)", stock.pe, self._format_indicator_decimal)
        add("市净率(最新)", stock.pb, self._format_indicator_decimal)
        if stock.pe:
            add("每股收益(计算)", stock.price / stock.pe, self._format_indicator_yuan, "元")
        if stock.pb:
            add("每股净资产", stock.price / stock.pb, self._format_indicator_yuan, "元")
        add("净资产收益率", roe, self._format_indicator_ratio_percent, tone="rise" if (roe or 0) >= 0 else "fall")
        add("扣非净利润", stock.deducted_net_profit_billion, self._format_indicator_yi, "亿")
        deducted_margin = self._as_percent(stock.deducted_net_profit_margin)
        add("扣非净利率", deducted_margin, self._format_indicator_percent_points)
        deducted_growth = self._as_percent(stock.deducted_net_profit_growth_rate)
        add("扣非净利同比", deducted_growth, self._format_indicator_percent_points)
        add("市值", stock.market_cap_billion, self._format_indicator_yi, "亿")
        add("股息率", stock.dividend_yield, self._format_indicator_ratio_percent)

        if not items:
            return None

        notes = ["ROE 缺失时按 市净率 / 市盈率 估算。"] if estimated_roe else []
        quarterly_items, quarterly_source, _quarterly_period, quarterly_notes = self._quarterly_eps_from_financial_source(
            stock
        )
        if quarterly_items:
            items.extend(quarterly_items)
            if quarterly_source:
                source_parts.append(quarterly_source)
        notes.extend(quarterly_notes)
        section = FinancialIndicatorSection(
            title="最新指标",
            period="行情估值",
            source="腾讯/通达信/本地缓存",
            items=items,
            notes=notes,
        )
        section.source = " / ".join(dict.fromkeys(source_parts))
        return section

    def _quarterly_eps_from_financial_source(
        self, stock: StockItem
    ) -> tuple[list[FinancialIndicatorItem], str | None, str | None, list[str]]:
        try:
            provider = AkShareProvider(proxy_mode=self.proxy_mode)
            return provider.get_quarterly_eps_indicators(stock.code)
        except Exception as exc:
            return [], None, None, [f"\u5355\u5b63\u5ea6 EPS \u8d22\u62a5\u4fe1\u6e90\u6682\u4e0d\u53ef\u7528\uff1a{exc}"]

    def list_relations(self) -> List[StockRelation]:
        return []

    def _fetch_universe(self) -> list[StockItem]:
        rows: list[StockItem] = []

        def fetch(api):
            for market in (0, 1):
                count = int(api.get_security_count(market) or 0)
                missed_pages = 0
                for start in range(0, count, self.page_size):
                    try:
                        page = api.get_security_list(market, start) or []
                    except Exception:
                        missed_pages += 1
                        if missed_pages >= 3 and rows:
                            break
                        continue
                    missed_pages = 0
                    for item in page:
                        stock = self._stock_from_security_list_item(item, market)
                        if stock is not None:
                            rows.append(stock)

        self._with_connected_api(fetch)
        deduped: dict[str, StockItem] = {}
        for row in rows:
            deduped[row.code] = row
        return sorted(deduped.values(), key=lambda item: item.code)

    def _quotes_batched(self, codes: list[str]) -> tuple[dict[str, dict], int, str | None]:
        normalized_codes = [
            self._normalize_code(code)
            for code in codes
            if code and self._tdx_market_code(code) is not None
        ]
        if not normalized_codes:
            return {}, 0, None

        quotes: dict[str, dict] = {}
        failed_batches = 0

        def fetch(api):
            nonlocal failed_batches
            for index in range(0, len(normalized_codes), self.quote_batch_size):
                batch = normalized_codes[index : index + self.quote_batch_size]
                query = [
                    (market, self._code_digits(item))
                    for item in batch
                    if (market := self._tdx_market_code(item)) is not None
                ]
                if not query:
                    continue
                try:
                    for quote in api.get_security_quotes(query) or []:
                        parsed = self._quote_from_raw(quote)
                        if parsed:
                            quotes[parsed["code"]] = parsed
                except Exception:
                    failed_batches += 1

        try:
            self._with_connected_api(fetch)
        except Exception as exc:
            return {}, max(failed_batches, 1), f"通达信行情不可用：{exc}"
        return quotes, failed_batches, None

    def _tencent_quotes_batched(self, codes: list[str]) -> tuple[dict[str, dict], int]:
        return self._tencent.quotes_batched(codes, self._tencent_quote)

    def _tencent_quote(self, codes: list[str]) -> dict[str, dict]:
        return self._tencent.quote(codes)

    def _screen_price_policy(self) -> tuple[str, str]:
        checked_at = datetime.now(self._CHINA_TZ)
        if checked_at.weekday() < 5 and checked_at.time() < self._SCREEN_MARKET_CLOSE:
            return "last_close", "当天未收盘，使用前一交易日收盘价"
        return "price", "当天已收盘，使用当天收盘价"

    def _screen_quote_price(self, quote: Optional[dict], price_field: str) -> Optional[float]:
        if not quote:
            return None
        preferred = self._positive_float(quote.get(price_field))
        if preferred is not None:
            return preferred
        fallback_field = "last_close" if price_field == "price" else "price"
        return self._positive_float(quote.get(fallback_field))

    def _with_connected_api(self, callback, *, host_limit: int | None = None, timeout: float | None = None):
        from pytdx.hq import TdxHq_API

        last_error: Exception | None = None
        hosts = self._hosts()
        if host_limit is not None:
            hosts = hosts[: max(1, host_limit)]
        connection_timeout = timeout if timeout is not None else self.timeout
        for host, port in hosts:
            api = TdxHq_API(raise_exception=True, auto_retry=False)
            try:
                with api.connect(host, port, time_out=connection_timeout):
                    return callback(api)
            except Exception as exc:
                last_error = exc
                continue
        raise RuntimeError(f"所有通达信服务器连接失败，最近错误：{last_error}")

    def _observe_fallback_host_limit(self) -> int:
        return env_int("TDX_OBSERVE_FALLBACK_HOST_LIMIT", 3, minimum=1, maximum=30)

    def _observe_fallback_timeout(self) -> float:
        return env_float("TDX_OBSERVE_FALLBACK_TIMEOUT", min(self.timeout, 1.5), minimum=0.2, maximum=10.0)

    def _tencent_timeout(self) -> float:
        return env_float("TDX_TENCENT_TIMEOUT", min(self.timeout, 4.0), minimum=0.5, maximum=15.0)

    def _tencent_minute_count(self) -> int:
        return env_int("TDX_TENCENT_MINUTE_COUNT", 800, minimum=100, maximum=800)

    def _read_cached_universe(self) -> list[StockItem] | None:
        if not os.path.exists(self.cache_path):
            return None
        try:
            df = pd.read_csv(self.cache_path)
            return [self._stock_from_cached_row(row) for _, row in df.iterrows()]
        except Exception:
            return None

    def _write_cached_universe(self, items: list[StockItem]) -> None:
        os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
        pd.DataFrame([item.model_dump() for item in items]).to_csv(self.cache_path, index=False)

    def _find_cached_stock(self, normalized_code: str) -> StockItem:
        for item in self.list_stocks():
            if item.code == normalized_code:
                return item
        raise KeyError(f"未找到股票 {normalized_code}")

    def _cached_fundamentals_for_screen(self) -> tuple[dict[str, StockItem], str | None]:
        lookup = self._read_fundamental_cache(self.fundamental_cache_path)
        if lookup is not None and (lookup or os.getenv("TDX_FUNDAMENTAL_CACHE")):
            return lookup, None

        try:
            frame, note = self._fetch_eastmoney_fundamentals_for_screen()
        except Exception as exc:
            legacy = self._read_legacy_fundamental_caches()
            if legacy is not None:
                return legacy, f"\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u8d22\u62a5\u63a5\u53e3\u4e0d\u53ef\u7528\uff0c\u5df2\u56de\u9000\u5230\u65e7\u7f13\u5b58\uff1a{exc}"
            return {}, f"\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u8d22\u62a5\u63a5\u53e3\u4e0d\u53ef\u7528\uff1a{exc}"

        if frame.empty:
            legacy = self._read_legacy_fundamental_caches()
            if legacy is not None:
                return legacy, "\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u672a\u8fd4\u56de\u5168\u5e02\u573a\u8d22\u62a5\u6570\u636e\uff0c\u5df2\u56de\u9000\u5230\u65e7\u7f13\u5b58\u3002"
            return {}, "\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u672a\u8fd4\u56de\u5168\u5e02\u573a\u8d22\u62a5\u6570\u636e\u3002"

        self._write_fundamental_cache(frame)
        return self._fundamental_lookup_from_frame(frame), note

    def _read_fundamental_cache(self, path: str) -> dict[str, StockItem] | None:
        if not os.path.exists(path):
            return None
        try:
            frame = pd.read_csv(
                path,
                dtype={"f12": str, "code": str, "SECUCODE": str, "SECURITY_CODE": str},
            )
        except Exception:
            return None
        return self._fundamental_lookup_from_frame(frame)

    def _read_legacy_fundamental_caches(self) -> dict[str, StockItem] | None:
        for path in self._legacy_fundamental_cache_paths():
            lookup = self._read_fundamental_cache(path)
            if lookup:
                return lookup
        return None

    def _fundamental_lookup_from_frame(self, frame: pd.DataFrame) -> dict[str, StockItem]:
        items = [item for _, row in frame.iterrows() if (item := self._stock_from_fundamental_row(row)) is not None]
        lookup: dict[str, StockItem] = {}
        for item in items:
            normalized = self._normalize_code(item.code)
            if normalized:
                lookup[normalized] = item
        return lookup

    def _write_fundamental_cache(self, frame: pd.DataFrame) -> None:
        directory = os.path.dirname(self.fundamental_cache_path)
        if directory:
            os.makedirs(directory, exist_ok=True)
        frame.to_csv(self.fundamental_cache_path, index=False)

    def _fetch_eastmoney_fundamentals_for_screen(self) -> tuple[pd.DataFrame, str]:
        last_frame = pd.DataFrame()
        last_period: str | None = None
        for report_date in self._fundamental_report_date_candidates():
            frame = self._fetch_eastmoney_fundamentals_for_report_date(report_date)
            if not frame.empty:
                last_frame = frame
                last_period = report_date
            if len(frame) >= self.fundamental_min_rows:
                return frame, (
                    f"\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u8d22\u62a5\u6307\u6807\u5df2\u7f13\u5b58\uff1a"
                    f"{report_date} \u62a5\u544a\u671f\uff0c\u8986\u76d6 {len(frame)} \u53ea A \u80a1\u3002"
                )
        if last_period is not None:
            return last_frame, (
                f"\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u8d22\u62a5\u6307\u6807\u5df2\u7f13\u5b58\uff1a"
                f"{last_period} \u62a5\u544a\u671f\uff0c\u4ec5\u8986\u76d6 {len(last_frame)} \u53ea A \u80a1\u3002"
            )
        return last_frame, "\u4e1c\u8d22\u6570\u636e\u4e2d\u5fc3\u6682\u65e0\u53ef\u7528\u8d22\u62a5\u6307\u6807\u3002"

    def _fetch_eastmoney_fundamentals_for_report_date(self, report_date: str) -> pd.DataFrame:
        rows: list[dict] = []
        page_count = 1
        for page in range(1, self.fundamental_max_pages + 1):
            response = self._session.get(
                "https://datacenter.eastmoney.com/securities/api/data/get",
                params={
                    "type": "RPT_F10_FINANCE_MAINFINADATA",
                    "sty": "APP_F10_MAINFINADATA",
                    "filter": f"(REPORT_DATE='{report_date}')",
                    "p": page,
                    "ps": self.fundamental_page_size,
                    "sr": "-1",
                    "st": "REPORT_DATE",
                    "source": "HSF10",
                    "client": "PC",
                },
                headers={"Referer": "https://emweb.securities.eastmoney.com/"},
                timeout=self._eastmoney_timeout(),
            )
            response.raise_for_status()
            payload = response.json()
            result = (payload.get("result") or {}) if isinstance(payload, dict) else {}
            page_rows = result.get("data") or []
            rows.extend([row for row in page_rows if isinstance(row, dict)])
            try:
                page_count = int(result.get("pages") or page_count or 1)
            except (TypeError, ValueError):
                page_count = page_count or 1
            if page >= page_count or not page_rows:
                break

        if not rows:
            return pd.DataFrame()
        frame = pd.DataFrame(rows)
        code_column = (
            "SECUCODE"
            if "SECUCODE" in frame.columns
            else "SECURITY_CODE"
            if "SECURITY_CODE" in frame.columns
            else None
        )
        if code_column:
            frame = frame[frame[code_column].map(lambda value: self._is_a_share_code(self._code_digits(str(value))))]
            frame = frame.drop_duplicates(subset=[code_column], keep="first")
        return frame.reset_index(drop=True)

    def _fundamental_report_date_candidates(self, today: date | None = None) -> list[str]:
        today = today or datetime.now(self._CHINA_TZ).date()
        report_dates: list[date] = []
        for year in range(today.year, today.year - 3, -1):
            report_dates.extend(
                [
                    date(year, 9, 30),
                    date(year, 6, 30),
                    date(year, 3, 31),
                    date(year - 1, 12, 31),
                ]
            )
        seen: set[str] = set()
        candidates: list[str] = []
        for item in sorted((item for item in report_dates if item <= today), reverse=True):
            key = item.isoformat()
            if key in seen:
                continue
            seen.add(key)
            candidates.append(key)
            if len(candidates) >= 6:
                break
        return candidates

    @classmethod
    def _stock_from_security_list_item(cls, item: dict, market: int) -> StockItem | None:
        code = str(item.get("code") or "").strip()
        if not cls._is_a_share_code(code):
            return None
        name = str(item.get("name") or code).strip() or code
        return StockItem(
            code=cls._normalize_code(code, market=market),
            name=name,
            industry=cls._board_label(code, market),
            is_st=cls._is_risk_labeled_name(name),
            price=cls._positive_float(item.get("pre_close")) or 0.0,
            pe=None,
            pb=None,
            roe=None,
            market_cap_billion=None,
            dividend_yield=None,
        )

    @classmethod
    def _stock_from_cached_row(cls, row) -> StockItem:
        name = str(row.get("name") or row.get("code") or "")
        return StockItem(
            code=str(row.get("code") or ""),
            name=name,
            industry=str(row.get("industry") or "通达信股票池"),
            is_st=cls._bool_value(row.get("is_st")) or cls._is_risk_labeled_name(name),
            price=cls._positive_float(row.get("price")) or 0.0,
            pe=cls._non_negative(row.get("pe")),
            pb=cls._non_negative(row.get("pb")),
            roe=cls._to_float(row.get("roe")),
            market_cap_billion=cls._non_negative(row.get("market_cap_billion")),
            dividend_yield=cls._non_negative(row.get("dividend_yield")),
            deducted_net_profit_billion=cls._deducted_net_profit_billion_from_row(row),
            deducted_net_profit_margin=cls._deducted_net_profit_margin_from_row(row),
            deducted_net_profit_growth_rate=cls._deducted_net_profit_growth_rate_from_row(row),
        )

    @classmethod
    def _stock_from_fundamental_row(cls, row) -> StockItem | None:
        raw_code = str(
            row.get("code")
            or row.get("SECUCODE")
            or row.get("SECURITY_CODE")
            or row.get("f12")
            or ""
        ).strip()
        if not raw_code:
            return None
        code = cls._normalize_code(raw_code)
        name = str(row.get("name") or row.get("SECURITY_NAME_ABBR") or row.get("f14") or raw_code).strip() or raw_code
        latest_price = cls._positive_float(row.get("price")) or cls._positive_float(row.get("f2")) or 0.0
        market_cap = cls._to_float(row.get("market_cap_billion"))
        if market_cap is None:
            raw_market_cap = cls._to_float(row.get("f20"))
            if raw_market_cap is not None:
                market_cap = raw_market_cap / 1e8 if raw_market_cap > 1e6 else raw_market_cap
        return StockItem(
            code=code,
            name=name,
            industry=str(row.get("industry") or row.get("f100") or row.get("INDUSTRY_NAME") or "未知行业").strip()
            or "未知行业",
            is_st=cls._bool_value(row.get("is_st")) or cls._is_risk_labeled_name(name),
            price=latest_price,
            pe=cls._non_negative(row.get("pe")) or cls._non_negative(row.get("f9")),
            pb=cls._non_negative(row.get("pb")) or cls._non_negative(row.get("f23")),
            roe=cls._to_float(row.get("roe")),
            market_cap_billion=cls._non_negative(market_cap),
            dividend_yield=cls._non_negative(row.get("dividend_yield")),
            deducted_net_profit_billion=cls._deducted_net_profit_billion_from_row(row),
            deducted_net_profit_margin=cls._deducted_net_profit_margin_from_row(row),
            deducted_net_profit_growth_rate=cls._deducted_net_profit_growth_rate_from_row(row),
        )

    def _stock_with_quote(self, item: StockItem, quote: Optional[dict], price_field: str) -> StockItem | None:
        if not quote:
            return None
        price = self._screen_quote_price(quote, price_field)
        if price is None:
            return None
        quote_price = self._positive_float(quote.get("price")) or price
        ratio = price / quote_price if quote_price and quote_price > 0 else 1.0
        if not math.isfinite(ratio) or ratio <= 0:
            ratio = 1.0
        pe = self._scale_optional(
            self._non_negative(quote.get("pe_ttm")) or self._non_negative(quote.get("pe_static")),
            ratio,
        )
        pb = self._scale_optional(self._non_negative(quote.get("pb")), ratio)
        market_cap_billion = self._scale_optional(self._non_negative(quote.get("mcap_yi")), ratio)
        return item.model_copy(
            update={
                "name": quote.get("name") or item.name,
                "is_st": item.is_st or self._is_risk_labeled_name(quote.get("name") or item.name),
                "price": price,
                "pe": pe if pe is not None else item.pe,
                "pb": pb if pb is not None else item.pb,
                "market_cap_billion": market_cap_billion if market_cap_billion is not None else item.market_cap_billion,
            }
        )

    def _stock_with_fundamentals(
        self,
        item: StockItem,
        fundamental: StockItem | None,
        prefer_fundamental_price: bool = False,
    ) -> tuple[StockItem, bool, bool, bool]:
        if fundamental is None:
            return item, False, False, False

        target_price = self._positive_float(item.price)
        source_price = self._positive_float(fundamental.price)
        used_fundamental_price = False
        if prefer_fundamental_price and source_price:
            target_price = source_price
            used_fundamental_price = True
        ratio = target_price / source_price if target_price and source_price else 1.0
        if not math.isfinite(ratio) or ratio <= 0:
            ratio = 1.0

        pe = self._scale_optional(fundamental.pe, ratio)
        pb = self._scale_optional(fundamental.pb, ratio)
        final_pe = pe if pe is not None else item.pe
        final_pb = pb if pb is not None else item.pb
        market_cap_billion = self._scale_optional(fundamental.market_cap_billion, ratio)
        roe = fundamental.roe if fundamental.roe is not None else item.roe
        estimated_roe = False
        if roe is None and final_pe and final_pb:
            roe = final_pb / final_pe
            estimated_roe = True

        industry = self._preferred_industry(item.industry, fundamental.industry)
        updates = {
            "price": target_price if used_fundamental_price and target_price else item.price,
            "industry": industry,
            "is_st": item.is_st or self._is_risk_labeled_name(fundamental.name),
            "pe": final_pe,
            "pb": final_pb,
            "roe": roe if roe is not None else item.roe,
            "market_cap_billion": (
                market_cap_billion if market_cap_billion is not None else item.market_cap_billion
            ),
            "dividend_yield": (
                fundamental.dividend_yield if fundamental.dividend_yield is not None else item.dividend_yield
            ),
            "deducted_net_profit_billion": (
                fundamental.deducted_net_profit_billion
                if fundamental.deducted_net_profit_billion is not None
                else item.deducted_net_profit_billion
            ),
            "deducted_net_profit_margin": (
                fundamental.deducted_net_profit_margin
                if fundamental.deducted_net_profit_margin is not None
                else item.deducted_net_profit_margin
            ),
            "deducted_net_profit_growth_rate": (
                fundamental.deducted_net_profit_growth_rate
                if fundamental.deducted_net_profit_growth_rate is not None
                else item.deducted_net_profit_growth_rate
            ),
        }
        used = (
            any(value is not None for key, value in updates.items() if key not in {"industry", "is_st"})
            or industry != item.industry
            or updates["is_st"] != item.is_st
        )
        return item.model_copy(update=updates), used, estimated_roe, used_fundamental_price

    @classmethod
    def _quote_from_raw(cls, quote: dict) -> dict | None:
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
            "volume": cls._to_float(quote.get("vol")),
            "amount": cls._to_float(quote.get("amount")),
            "timestamp": cls._tdx_timestamp(quote),
            "bid1": cls._to_float(quote.get("bid1")),
            "bid1_volume": cls._to_float(quote.get("bid_vol1")),
            "bid2": cls._to_float(quote.get("bid2")),
            "bid2_volume": cls._to_float(quote.get("bid_vol2")),
            "bid3": cls._to_float(quote.get("bid3")),
            "bid3_volume": cls._to_float(quote.get("bid_vol3")),
            "bid4": cls._to_float(quote.get("bid4")),
            "bid4_volume": cls._to_float(quote.get("bid_vol4")),
            "bid5": cls._to_float(quote.get("bid5")),
            "bid5_volume": cls._to_float(quote.get("bid_vol5")),
            "ask1": cls._to_float(quote.get("ask1")),
            "ask1_volume": cls._to_float(quote.get("ask_vol1")),
            "ask2": cls._to_float(quote.get("ask2")),
            "ask2_volume": cls._to_float(quote.get("ask_vol2")),
            "ask3": cls._to_float(quote.get("ask3")),
            "ask3_volume": cls._to_float(quote.get("ask_vol3")),
            "ask4": cls._to_float(quote.get("ask4")),
            "ask4_volume": cls._to_float(quote.get("ask_vol4")),
            "ask5": cls._to_float(quote.get("ask5")),
            "ask5_volume": cls._to_float(quote.get("ask_vol5")),
        }

    @staticmethod
    def _history_row(bar: dict) -> dict:
        date = f"{int(bar.get('year')):04d}-{int(bar.get('month')):02d}-{int(bar.get('day')):02d}"
        hour = int(bar.get("hour") or 15)
        minute = int(bar.get("minute") or 0)
        close = TdxProvider._to_float(bar.get("close")) or 0.0
        return {
            "date": date,
            "datetime": f"{date} {hour:02d}:{minute:02d}:00",
            "open": TdxProvider._to_float(bar.get("open")) or close,
            "high": TdxProvider._to_float(bar.get("high")) or close,
            "low": TdxProvider._to_float(bar.get("low")) or close,
            "close": close,
            "volume": TdxProvider._to_float(bar.get("vol")),
            "amount": TdxProvider._to_float(bar.get("amount")),
        }

    @staticmethod
    def _hosts() -> list[tuple[str, int]]:
        env_hosts = os.getenv("TDX_HOSTS", os.getenv("ASTOCK_TDX_HOSTS", "")).strip()
        hosts: list[tuple[str, int]] = []
        if env_hosts:
            for item in env_hosts.split(","):
                host, _, port = item.strip().partition(":")
                if host:
                    hosts.append((host, int(port or "7709")))
            if hosts:
                return hosts
        preferred = [
            ("218.6.170.47", 7709),
            ("119.147.212.81", 7709),
            ("101.227.73.20", 7709),
        ]
        try:
            from pytdx.config.hosts import hq_hosts

            for _, host, port in hq_hosts:
                preferred.append((host, int(port)))
        except Exception:
            pass

        limit = env_int("TDX_HOST_LIMIT", 30, minimum=1)
        seen: set[tuple[str, int]] = set()
        for host in preferred:
            if host in seen:
                continue
            seen.add(host)
            hosts.append(host)
            if len(hosts) >= limit:
                break
        return hosts

    @staticmethod
    def _is_a_share_code(code: str) -> bool:
        return code.startswith(("000", "001", "002", "003", "300", "301", "600", "601", "603", "605", "688"))

    @staticmethod
    def _board_label(code: str, market: int) -> str:
        if code.startswith("688"):
            return "科创板"
        if code.startswith(("300", "301")):
            return "创业板"
        if market == 1:
            return "沪市A股"
        return "深市A股"

    @staticmethod
    def _tdx_market_code(code: str) -> Optional[int]:
        normalized = TdxProvider._normalize_code(code)
        digits = TdxProvider._code_digits(normalized)
        if normalized.endswith(".SH") or digits.startswith(("6", "9")):
            return 1
        if normalized.endswith(".SZ") or digits.startswith(("0", "2", "3")):
            return 0
        return None

    @staticmethod
    def _eastmoney_market_code(code: str) -> Optional[int]:
        normalized = TdxProvider._normalize_code(code)
        digits = TdxProvider._code_digits(normalized)
        if normalized.endswith(".SH") or digits.startswith(("6", "9")):
            return 1
        if normalized.endswith(".SZ") or digits.startswith(("0", "2", "3")):
            return 0
        if normalized.endswith(".BJ") or digits.startswith(("4", "8")):
            return 0
        return None

    @staticmethod
    def _eastmoney_timeout() -> float:
        return env_float("TDX_EASTMONEY_TIMEOUT", 4.0, minimum=0.5, maximum=15.0)

    @classmethod
    def _eastmoney_kline_row(cls, raw: str) -> dict | None:
        parts = str(raw or "").split(",")
        if len(parts) < 7:
            return None
        close = cls._to_float(parts[2])
        if close is None:
            return None
        return {
            "date": parts[0],
            "open": cls._to_float(parts[1]) or close,
            "close": close,
            "high": cls._to_float(parts[3]) or close,
            "low": cls._to_float(parts[4]) or close,
            "volume": cls._to_float(parts[5]),
            "amount": cls._to_float(parts[6]),
        }

    @classmethod
    def _eastmoney_minute_row(cls, raw: str) -> MinuteBar | None:
        parts = str(raw or "").split(",")
        if len(parts) < 7:
            return None
        close = cls._to_float(parts[2])
        if close is None:
            return None
        return MinuteBar(
            datetime=cls._normalize_eastmoney_datetime(parts[0]),
            open=cls._to_float(parts[1]) or close,
            close=close,
            high=cls._to_float(parts[3]) or close,
            low=cls._to_float(parts[4]) or close,
            volume=cls._to_float(parts[5]),
            amount=cls._to_float(parts[6]),
        )

    @staticmethod
    def _normalize_eastmoney_datetime(value: str) -> str:
        raw = str(value or "").strip()
        if len(raw) == 16 and raw[4] == "-" and raw[13] == ":":
            return f"{raw}:00"
        return raw

    @classmethod
    def _tencent_kline_row(cls, raw: list | tuple) -> dict | None:
        if not isinstance(raw, (list, tuple)) or len(raw) < 6:
            return None
        close = cls._to_float(raw[2])
        if close is None:
            return None
        return {
            "date": str(raw[0]),
            "open": cls._to_float(raw[1]) or close,
            "close": close,
            "high": cls._to_float(raw[3]) or close,
            "low": cls._to_float(raw[4]) or close,
            "volume": cls._to_float(raw[5]),
            "amount": None,
        }

    @classmethod
    def _tencent_minute_row(cls, raw: list | tuple) -> MinuteBar | None:
        if not isinstance(raw, (list, tuple)) or len(raw) < 6:
            return None
        close = cls._to_float(raw[2])
        if close is None:
            return None
        return MinuteBar(
            datetime=cls._normalize_tencent_datetime(str(raw[0])),
            open=cls._to_float(raw[1]) or close,
            close=close,
            high=cls._to_float(raw[3]) or close,
            low=cls._to_float(raw[4]) or close,
            volume=cls._to_float(raw[5]),
            amount=None,
        )

    @staticmethod
    def _normalize_tencent_datetime(value: str) -> str:
        raw = str(value or "").strip()
        if len(raw) >= 12 and raw[:12].isdigit():
            return f"{raw[:4]}-{raw[4:6]}-{raw[6:8]} {raw[8:10]}:{raw[10:12]}:00"
        return raw

    @staticmethod
    def _tencent_date(value: str) -> str:
        raw = TdxProvider._date_key(value)
        if len(raw) == 8 and raw.isdigit():
            return f"{raw[:4]}-{raw[4:6]}-{raw[6:8]}"
        return str(value or "")

    @staticmethod
    def _normalize_code(code: str, market: int | None = None) -> str:
        digits = TdxProvider._code_digits(code)
        if market == 1 or digits.startswith("6"):
            return f"{digits}.SH"
        if market == 0 or digits.startswith(("0", "2", "3")):
            return f"{digits}.SZ"
        if digits.startswith(("4", "8")):
            return f"{digits}.BJ"
        if digits:
            return f"{digits}.SZ"
        return digits

    @staticmethod
    def _code_digits(code: str) -> str:
        normalized = str(code or "").strip().upper()
        if "." in normalized:
            return normalized.split(".")[0]
        if normalized.startswith(("SH", "SZ", "BJ")):
            return normalized[2:]
        return normalized

    @staticmethod
    def _book_level(level: int, quote: dict, side: str) -> OrderBookLevel | None:
        price = quote.get(f"{side}{level}")
        volume = quote.get(f"{side}{level}_volume")
        if price is None and volume is None:
            return None
        return OrderBookLevel(level=level, price=price, volume=volume)

    @staticmethod
    def _date_key(value: str) -> str:
        return str(value or "").replace("-", "")[:8]

    @staticmethod
    def _datetime_key(value: str) -> str:
        return str(value or "").replace("-", "").replace(":", "").replace(" ", "")[:14]

    @staticmethod
    def _tdx_timestamp(quote: dict) -> str | None:
        raw = str(quote.get("servertime") or "").strip()
        if not raw:
            return None
        return f"{datetime.now().date().isoformat()} {raw}"

    @staticmethod
    def _resolve_fundamental_cache_path() -> str:
        explicit = os.getenv("TDX_FUNDAMENTAL_CACHE")
        if explicit:
            return explicit
        return "data/cache/tdx_fundamentals.csv"

    @staticmethod
    def _legacy_fundamental_cache_paths() -> list[str]:
        return [
            path
            for path in [
                os.getenv("ASTOCK_CACHE"),
                "data/cache/astock_stocks.csv",
                os.getenv("EASTMONEY_CACHE"),
                "data/cache/eastmoney_stocks.csv",
            ]
            if path
        ]

    @staticmethod
    def _preferred_industry(current: str, candidate: str) -> str:
        candidate_value = str(candidate or "").strip()
        current_value = str(current or "").strip()
        generic = {"", "通达信股票池", "深市A股", "沪市A股", "科创板", "创业板", "未知行业"}
        if candidate_value and candidate_value not in generic:
            return candidate_value
        return current_value or candidate_value or "通达信股票池"

    @staticmethod
    def _scale_optional(value: Optional[float], ratio: float) -> Optional[float]:
        if value is None:
            return None
        result = value * ratio
        return result if math.isfinite(result) and result >= 0 else None

    @staticmethod
    def _format_indicator_decimal(value: float) -> str:
        return f"{value:.3f}".rstrip("0").rstrip(".")

    @staticmethod
    def _format_indicator_yuan(value: float) -> str:
        return f"{value:.3f}".rstrip("0").rstrip(".") + "元"

    @staticmethod
    def _format_indicator_yi(value: float) -> str:
        return f"{value:.3f}".rstrip("0").rstrip(".") + "亿"

    @staticmethod
    def _format_indicator_ratio_percent(value: float) -> str:
        return f"{value * 100:.2f}".rstrip("0").rstrip(".") + "%"

    @staticmethod
    def _format_indicator_percent_points(value: float) -> str:
        return f"{value:.2f}".rstrip("0").rstrip(".") + "%"

    @staticmethod
    def _as_percent(value: float | None) -> float | None:
        if value is None:
            return None
        return value * 100 if -1 <= value <= 1 else value

    @staticmethod
    def _is_risk_labeled_name(name: object) -> bool:
        value = str(name or "").strip().upper()
        return "ST" in value or "退市" in value

    @staticmethod
    def _bool_value(value) -> bool:
        return str(value).strip().lower() in {"1", "true", "yes", "y"}

    @staticmethod
    def _non_negative(value) -> Optional[float]:
        result = TdxProvider._to_float(value)
        return result if result is not None and result >= 0 else None

    @classmethod
    def _deducted_net_profit_billion_from_row(cls, row) -> Optional[float]:
        value = cls._first_float(
            row,
            [
                "deducted_net_profit_billion",
                "扣非净利润_亿",
                "扣非净利润",
                "DEDU_PARENT_PROFIT",
                "DEDUCT_PARENT_NETPROFIT",
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
                "DEDUCT_PARENT_NETPROFIT_YOY",
                "DJD_DEDUCTDPNP_YOY",
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
    def _positive_float(value) -> Optional[float]:
        result = TdxProvider._to_float(value)
        return result if result is not None and result > 0 else None

    @staticmethod
    def _to_float(value) -> Optional[float]:
        try:
            if value is None or value == "":
                return None
            if isinstance(value, float) and pd.isna(value):
                return None
            return float(value)
        except (TypeError, ValueError):
            return None

