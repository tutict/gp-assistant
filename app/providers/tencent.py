from __future__ import annotations

import math
from datetime import datetime
from typing import Callable, Optional

import requests


class TencentQuoteClient:
    endpoint = "https://qt.gtimg.cn/q="

    def __init__(self, session: requests.Session, timeout: float, batch_size: int | str = 80):
        self.session = session
        self.timeout = timeout
        self.batch_size = self._positive_int(batch_size, 80)

    def quotes_batched(
        self,
        codes: list[str],
        quote_func: Callable[[list[str]], dict[str, dict]] | None = None,
    ) -> tuple[dict[str, dict], int]:
        normalized_codes = self._dedupe([self.normalize_code(code) for code in codes if code])
        if not normalized_codes:
            return {}, 0

        fetch_quote = quote_func or self.quote
        quotes: dict[str, dict] = {}
        failed_batches = 0
        for index in range(0, len(normalized_codes), self.batch_size):
            batch = normalized_codes[index : index + self.batch_size]
            try:
                quotes.update(fetch_quote(batch))
            except Exception:
                failed_batches += 1
        return quotes, failed_batches

    def quote(self, codes: list[str]) -> dict[str, dict]:
        symbols = self._dedupe([symbol for code in codes if (symbol := self.tencent_symbol(code))])
        if not symbols:
            return {}
        response = self.session.get(
            self.endpoint + ",".join(symbols),
            timeout=self.timeout,
        )
        response.raise_for_status()
        response.encoding = "gbk"
        return self.parse_response(response.text)

    @classmethod
    def parse_response(cls, text: str) -> dict[str, dict]:
        result: dict[str, dict] = {}
        for raw_line in str(text or "").strip().split(";"):
            line = raw_line.strip()
            if not line or "=" not in line or '"' not in line:
                continue
            left, _, right = line.partition("=")
            key = left.split("_")[-1]
            quoted = right.split('"')
            if len(quoted) < 2:
                continue
            values = quoted[1].split("~")
            if len(values) < 53:
                continue
            code = cls.code_digits(key)
            if not code:
                continue
            result[code] = {
                "code": code,
                "name": values[1],
                "price": cls.to_float(values[3]),
                "last_close": cls.to_float(values[4]),
                "open": cls.to_float(values[5]),
                "bid1": cls.to_float(values[9]),
                "bid1_volume": cls.to_float(values[10]),
                "bid2": cls.to_float(values[11]),
                "bid2_volume": cls.to_float(values[12]),
                "bid3": cls.to_float(values[13]),
                "bid3_volume": cls.to_float(values[14]),
                "bid4": cls.to_float(values[15]),
                "bid4_volume": cls.to_float(values[16]),
                "bid5": cls.to_float(values[17]),
                "bid5_volume": cls.to_float(values[18]),
                "ask1": cls.to_float(values[19]),
                "ask1_volume": cls.to_float(values[20]),
                "ask2": cls.to_float(values[21]),
                "ask2_volume": cls.to_float(values[22]),
                "ask3": cls.to_float(values[23]),
                "ask3_volume": cls.to_float(values[24]),
                "ask4": cls.to_float(values[25]),
                "ask4_volume": cls.to_float(values[26]),
                "ask5": cls.to_float(values[27]),
                "ask5_volume": cls.to_float(values[28]),
                "timestamp": cls.format_timestamp(values[30]),
                "change_amt": cls.to_float(values[31]),
                "change_pct": cls.to_float(values[32]),
                "high": cls.to_float(values[33]),
                "low": cls.to_float(values[34]),
                "amount_wan": cls.to_float(values[37]),
                "turnover_pct": cls.to_float(values[38]),
                "pe_ttm": cls.to_float(values[39]),
                "amplitude_pct": cls.to_float(values[43]),
                "mcap_yi": cls.to_float(values[44]),
                "float_mcap_yi": cls.to_float(values[45]),
                "pb": cls.to_float(values[46]),
                "limit_up": cls.to_float(values[47]),
                "limit_down": cls.to_float(values[48]),
                "vol_ratio": cls.to_float(values[49]),
                "pe_static": cls.to_float(values[52]),
            }
        return result

    @staticmethod
    def tencent_symbol(code: str) -> str:
        normalized = TencentQuoteClient.normalize_code(code)
        digits = TencentQuoteClient.code_digits(normalized)
        if not digits:
            return ""
        if normalized.endswith(".BJ") or digits.startswith(("4", "8", "920")):
            return f"bj{digits}"
        if normalized.endswith(".SH") or digits.startswith(("6", "9")):
            return f"sh{digits}"
        return f"sz{digits}"

    @staticmethod
    def normalize_code(code: str) -> str:
        raw = str(code or "").strip().upper()
        digits = TencentQuoteClient.code_digits(code)
        if not digits:
            return ""
        if raw.startswith("BJ") or raw.endswith(".BJ") or digits.startswith(("4", "8", "920")):
            return f"{digits}.BJ"
        if digits.startswith(("6", "9")):
            return f"{digits}.SH"
        return f"{digits}.SZ"

    @staticmethod
    def code_digits(code: str) -> str:
        normalized = str(code or "").strip().upper()
        if "." in normalized:
            digits = normalized.split(".", 1)[0]
            return digits if digits.isdigit() and len(digits) == 6 else ""
        if normalized.startswith(("SH", "SZ", "BJ")):
            digits = normalized[2:]
            return digits if digits.isdigit() and len(digits) == 6 else ""
        return normalized if normalized.isdigit() and len(normalized) == 6 else ""

    @staticmethod
    def format_timestamp(value: str) -> str | None:
        raw = str(value or "").strip()
        if len(raw) < 14:
            return raw or None
        try:
            return datetime.strptime(raw[:14], "%Y%m%d%H%M%S").isoformat(timespec="seconds")
        except ValueError:
            return raw

    @staticmethod
    def to_float(value) -> Optional[float]:
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
    def _dedupe(codes: list[str]) -> list[str]:
        seen: set[str] = set()
        result: list[str] = []
        for code in codes:
            if not code or code in seen:
                continue
            seen.add(code)
            result.append(code)
        return result

    @staticmethod
    def _positive_int(value, default: int) -> int:
        try:
            parsed = int(value)
        except (TypeError, ValueError):
            parsed = default
        return max(1, parsed)
