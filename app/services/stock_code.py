from __future__ import annotations


def stock_digits(code: str | None) -> str:
    normalized = str(code or "").strip().upper()
    if "." in normalized:
        digits = normalized.split(".", 1)[0]
        return digits if digits.isdigit() and len(digits) == 6 else ""
    if normalized.startswith(("SH", "SZ", "BJ")):
        digits = normalized[2:]
        return digits if digits.isdigit() and len(digits) == 6 else ""
    return normalized if normalized.isdigit() and len(normalized) == 6 else ""


def normalize_stock_code(code: str | None, market: str | None = None) -> str:
    digits = stock_digits(code)
    if not digits:
        return str(code or "").strip().upper()
    explicit_market = (market or "").strip().upper()
    normalized = str(code or "").strip().upper()
    if explicit_market in {"SH", "SZ", "BJ"}:
        suffix = explicit_market
    elif normalized.endswith((".SH", ".SZ", ".BJ")):
        suffix = normalized.rsplit(".", 1)[1]
    elif normalized.startswith(("SH", "SZ", "BJ")):
        suffix = normalized[:2]
    elif digits.startswith(("6", "9")):
        suffix = "SH"
    elif digits.startswith(("4", "8")):
        suffix = "BJ"
    else:
        suffix = "SZ"
    return f"{digits}.{suffix}"


def market_prefix(code: str | None) -> str:
    normalized = normalize_stock_code(code)
    if normalized.endswith(".SH"):
        return "sh"
    if normalized.endswith(".BJ"):
        return "bj"
    return "sz"


def compact_date(value: str | None) -> str:
    return "".join(char for char in str(value or "") if char.isdigit())[:8]
