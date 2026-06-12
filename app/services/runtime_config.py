from __future__ import annotations

import os
import re
from typing import Any


TRUE_VALUES = {"1", "true", "yes", "y", "on"}
FALSE_VALUES = {"0", "false", "no", "n", "off"}


def env_bool(name: str, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None or value == "":
        return default
    lowered = value.strip().lower()
    if lowered in TRUE_VALUES:
        return True
    if lowered in FALSE_VALUES:
        return False
    return default


def env_int(name: str, default: int, *, minimum: int | None = None, maximum: int | None = None) -> int:
    try:
        value = int(os.getenv(name, str(default)))
    except (TypeError, ValueError):
        value = default
    if minimum is not None:
        value = max(minimum, value)
    if maximum is not None:
        value = min(maximum, value)
    return value


def env_float(name: str, default: float, *, minimum: float | None = None, maximum: float | None = None) -> float:
    try:
        value = float(os.getenv(name, str(default)))
    except (TypeError, ValueError):
        value = default
    if minimum is not None:
        value = max(minimum, value)
    if maximum is not None:
        value = min(maximum, value)
    return value


def coalesce_str(*values: object) -> str | None:
    for value in values:
        if value is None:
            continue
        text = str(value).strip()
        if text:
            return text
    return None


def coalesce_float(*values: object) -> float:
    fallback = float(values[-1])
    for value in values[:-1]:
        if value is None or value == "":
            continue
        try:
            return float(value)
        except (TypeError, ValueError):
            continue
    return fallback


def coalesce_bool(*values: object) -> bool:
    fallback = bool(values[-1])
    for value in values[:-1]:
        if value is None or value == "":
            continue
        if isinstance(value, bool):
            return value
        lowered = str(value).strip().lower()
        if lowered in TRUE_VALUES:
            return True
        if lowered in FALSE_VALUES:
            return False
    return fallback


def truncate_chars(value: str, max_chars: int) -> str:
    if len(value) <= max_chars:
        return value
    return f"{value[:max_chars]}..."


def safe_string(value: object, max_chars: int) -> str:
    if not isinstance(value, str):
        return ""
    text = value.strip()
    return truncate_chars(text, max_chars) if text else ""


def safe_string_list(value: object, limit: int, max_chars: int = 160) -> list[str]:
    if not isinstance(value, list):
        return []
    result: list[str] = []
    for item in value:
        text = safe_string(item, max_chars)
        if text:
            result.append(text)
        if len(result) >= limit:
            break
    return result


def redact_error(error: Exception | object, *, max_chars: int = 180, secrets: list[str] | None = None) -> str:
    text = str(error)
    for secret in secrets or [os.getenv("OPENAI_API_KEY", "")]:
        if secret:
            text = text.replace(secret, "[redacted]")
    text = re.sub(r"sk-[A-Za-z0-9_-]{8,}", "sk-[redacted]", text)
    return truncate_chars(text, max_chars)


def safe_dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}
