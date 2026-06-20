"""Shared text / HTML / amount helpers.

Extracted from news_rag, upstream_rag_pack, capital_evidence and
institution_signals to remove duplicated implementations. Behaviour is kept
identical to the previous copies; callers that need a specific number of
decimal places pass ``decimals`` explicitly.
"""

from __future__ import annotations

import math
import re
from html import unescape
from html.parser import HTMLParser
from numbers import Real
from typing import Any, Sequence


class HtmlTextExtractor(HTMLParser):
    """Collect visible text, skipping script/style/noscript content."""

    def __init__(self) -> None:
        super().__init__()
        self.parts: list[str] = []
        self._skip_depth = 0

    def handle_starttag(self, tag: str, attrs) -> None:
        if tag.lower() in {"script", "style", "noscript"}:
            self._skip_depth += 1

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() in {"script", "style", "noscript"} and self._skip_depth:
            self._skip_depth -= 1

    def handle_data(self, data: str) -> None:
        if self._skip_depth:
            return
        text = data.strip()
        if text:
            self.parts.append(text)

    def text(self) -> str:
        return unescape(" ".join(self.parts)).strip()


def strip_html(value: str) -> str:
    parser = HtmlTextExtractor()
    parser.feed(value or "")
    return parser.text()


def coalesce_row(row: dict, keys: Sequence[str]) -> str:
    """Return the first non-empty stringified value among ``keys``."""

    for key in keys:
        value = row.get(key)
        if value is not None and str(value).strip():
            return str(value).strip()
    return ""


def clean_text(value: str) -> str:
    return re.sub(r"\s+", " ", value or "").strip()


def format_amount(number: float, *, decimals: int = 2) -> str:
    """Format a raw CNY amount into a 亿/万 suffixed string.

    ``decimals`` controls the fallback (< 万) precision so callers preserve
    their original output format.
    """

    if abs(number) >= 100_000_000:
        return f"{number / 100_000_000:.2f} 亿"
    if abs(number) >= 10_000:
        return f"{number / 10_000:.2f} 万"
    return f"{number:.{decimals}f}"
