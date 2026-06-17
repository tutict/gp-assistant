from __future__ import annotations

import math
import os
import time
from typing import Dict, List, Optional

import pandas as pd
import requests

from app.providers.akshare import AkShareProvider
from app.providers.base import PROXY_MODE_NONE, StockProvider, normalize_proxy_mode
from app.schemas import FinancialIndicatorSection, MinuteBar, OrderBookSnapshot, StockItem, StockRelation


class EastmoneyProvider(StockProvider):
    name = "eastmoney"
    _SCREEN_REQUIRED_FIELDS = ("f18",)

    def __init__(
        self,
        cache_path: Optional[str] = None,
        refresh: bool = False,
        proxy_mode: Optional[str] = None,
    ):
        self.cache_path = cache_path or os.getenv("EASTMONEY_CACHE", "data/cache/eastmoney_stocks.csv")
        self.refresh = refresh or os.getenv("EASTMONEY_REFRESH", "false").lower() == "true"
        self.max_pages = int(os.getenv("EASTMONEY_MAX_PAGES", "70"))
        self.page_size = int(os.getenv("EASTMONEY_PAGE_SIZE", "100"))
        self.proxy_mode = normalize_proxy_mode(proxy_mode)
        self._session = requests.Session()
        self._session.trust_env = self.proxy_mode != PROXY_MODE_NONE
        self._akshare = AkShareProvider(refresh=False, proxy_mode=self.proxy_mode)
        self._last_load_note: str | None = None

    def list_stocks(self) -> List[StockItem]:
        df = self._load_spot()
        return [stock for _, row in df.iterrows() if (stock := self._row_to_stock(row)) is not None]

    def list_stocks_for_screen(self) -> tuple[List[StockItem], List[str]]:
        try:
            df = self._load_spot(required_fields=self._SCREEN_REQUIRED_FIELDS)
        except Exception as exc:
            return self._fallback_stocks_for_screen(exc)

        items: List[StockItem] = []
        previous_close_count = 0
        fallback_count = 0
        for _, row in df.iterrows():
            stock = self._row_to_stock(row, use_previous_close=True)
            if stock is None:
                continue
            items.append(stock)
            if self._positive_float(row.get("f18")) is not None:
                previous_close_count += 1
            else:
                fallback_count += 1

        notes = []
        if self._last_load_note:
            notes.append(self._last_load_note)
        notes.append(f"筛选价格口径：前一交易日收盘价（东方财富昨收 f18），已应用 {previous_close_count} 只。")
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
        return self._akshare.get_order_book(code)

    def get_financial_indicators(self, stock: StockItem) -> FinancialIndicatorSection | None:
        return self._akshare.get_financial_indicators(stock)

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
        rows: list[dict] = []
        missed_pages = 0
        hosts = [
            "https://push2.eastmoney.com",
            "https://82.push2.eastmoney.com",
            "https://18.push2.eastmoney.com",
            "https://19.push2.eastmoney.com",
        ]

        for page in range(1, self.max_pages + 1):
            page_rows = self._fetch_page(page, hosts)
            if page_rows is None:
                missed_pages += 1
                if missed_pages >= 8 and rows:
                    break
                continue
            missed_pages = 0
            rows.extend(page_rows)
            if len(page_rows) < self.page_size:
                break

        if not rows:
            raise RuntimeError("东方财富 A 股实时行情不可用")
        return pd.DataFrame(rows).drop_duplicates(subset=["f12"])

    def _fetch_page(self, page: int, hosts: list[str]) -> Optional[list[dict]]:
        params = {
            "pn": page,
            "pz": self.page_size,
            "po": 1,
            "np": 1,
            "ut": "bd1d9ddb04089700cf9c27f6f7426281",
            "fltt": 2,
            "invt": 2,
            "fid": "f12",
            "fs": "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048",
            "fields": "f2,f9,f12,f14,f18,f20,f23,f100",
        }
        for host in hosts:
            try:
                response = self._session.get(
                    f"{host}/api/qt/clist/get",
                    params=params,
                    headers={"User-Agent": "Mozilla/5.0", "Referer": "https://quote.eastmoney.com/"},
                    timeout=12,
                )
                response.raise_for_status()
                data = response.json().get("data") or {}
                return data.get("diff") or []
            except Exception:
                time.sleep(0.15)
        return None

    def _load_spot(self, required_fields: tuple[str, ...] = ()) -> pd.DataFrame:
        self._last_load_note = None
        cached_df = self._read_cached_spot()
        if not self.refresh and cached_df is not None:
            missing_fields = [field for field in required_fields if field not in cached_df.columns]
            if not missing_fields:
                return cached_df

        try:
            df = self._fetch_spot()
        except Exception as exc:
            if cached_df is not None:
                missing_fields = [field for field in required_fields if field not in cached_df.columns]
                suffix = f"，缓存缺少 {', '.join(missing_fields)} 字段" if missing_fields else ""
                self._last_load_note = f"东方财富实时连接失败，已使用本地缓存{suffix}：{exc}"
                return cached_df
            return self._fallback_spot_from_akshare(exc)
        os.makedirs(os.path.dirname(self.cache_path), exist_ok=True)
        df.to_csv(self.cache_path, index=False)
        return df

    def _read_cached_spot(self) -> pd.DataFrame | None:
        if not os.path.exists(self.cache_path):
            return None
        try:
            return pd.read_csv(self.cache_path, dtype={"f12": str})
        except Exception:
            return None

    def _fallback_stocks_for_screen(self, exc: Exception) -> tuple[List[StockItem], List[str]]:
        try:
            items = self.list_stocks()
        except Exception as fallback_exc:
            return [], [f"东方财富数据源不可用，备用股票池也不可用：{fallback_exc}"]
        note = self._last_load_note or f"前一交易日收盘价不可用，已回退到股票池价格：{exc}"
        return items, [note]

    def _fallback_spot_from_akshare(self, exc: Exception) -> pd.DataFrame:
        try:
            items = self._akshare.list_stocks()
        except Exception as fallback_exc:
            raise RuntimeError(f"东方财富实时行情不可用，公开行情备用源也不可用：{fallback_exc}") from exc

        rows = [self._stock_to_row(item) for item in items]
        if not rows:
            raise RuntimeError("东方财富实时行情不可用，公开行情备用源没有返回股票") from exc
        self._last_load_note = f"东方财富实时连接失败，已临时使用公开行情备用源：{exc}"
        return pd.DataFrame(rows)

    @staticmethod
    def _stock_to_row(stock: StockItem) -> dict:
        return {
            "f2": stock.price,
            "f9": stock.pe,
            "f12": stock.code.split(".")[0],
            "f14": stock.name,
            "f18": None,
            "f20": stock.market_cap_billion,
            "f23": stock.pb,
            "f100": stock.industry,
            "deducted_net_profit_billion": stock.deducted_net_profit_billion,
            "deducted_net_profit_margin": stock.deducted_net_profit_margin,
        }

    def _row_to_stock(self, row: pd.Series, use_previous_close: bool = False) -> StockItem | None:
        code = str(row.get("f12", "")).strip()
        if not code or not code.startswith(("0", "3", "6")):
            return None

        name = str(row.get("f14", "")).strip() or code
        latest_price = self._positive_float(row.get("f2"))
        previous_close = self._positive_float(row.get("f18")) if use_previous_close else None
        price = previous_close or latest_price
        if price is None or price <= 0:
            return None

        ratio = price / latest_price if latest_price and latest_price > 0 else 1.0
        pe = self._scale_optional(self._non_negative(row.get("f9")), ratio)
        pb = self._scale_optional(self._non_negative(row.get("f23")), ratio)
        market_cap = self._to_float(row.get("f20"))
        market_cap_billion = None
        if market_cap is not None:
            market_cap_billion = market_cap / 1e8 if market_cap > 1e6 else market_cap
            market_cap_billion = self._scale_optional(market_cap_billion, ratio)

        industry = str(row.get("f100", "未知行业")).strip() or "未知行业"
        return StockItem(
            code=self._normalize_code(code),
            name=name,
            industry=industry,
            is_st="ST" in name.upper(),
            price=price,
            pe=pe,
            pb=pb,
            roe=None,
            market_cap_billion=market_cap_billion,
            dividend_yield=None,
            deducted_net_profit_billion=self._deducted_net_profit_billion_from_row(row),
            deducted_net_profit_margin=self._deducted_net_profit_margin_from_row(row),
        )

    @staticmethod
    def _non_negative(value) -> Optional[float]:
        result = EastmoneyProvider._to_float(value)
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
    def _first_float(cls, row, keys: list[str]) -> Optional[float]:
        for key in keys:
            value = cls._to_float(row.get(key))
            if value is not None:
                return value
        return None

    @staticmethod
    def _positive_float(value) -> Optional[float]:
        result = EastmoneyProvider._to_float(value)
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

    @staticmethod
    def _normalize_code(code: str) -> str:
        normalized = str(code or "").strip().upper()
        if "." in normalized:
            return normalized
        if normalized.startswith("6"):
            return f"{normalized}.SH"
        if normalized:
            return f"{normalized}.SZ"
        return normalized
