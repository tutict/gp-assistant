from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from typing import Any
from urllib.parse import urlparse

from app.schemas import LlmClientConfig
from app.services.runtime_config import coalesce_bool, coalesce_float, coalesce_str, redact_error


@dataclass(frozen=True)
class ResolvedLlmConfig:
    api_key: str | None
    base_url: str | None
    model: str
    temperature: float
    timeout_seconds: float
    json_mode: bool
    organization: str | None
    project: str | None


def resolve_llm_config(
    override: LlmClientConfig | None,
    *,
    default_model: str = "gpt-4o-mini",
    default_temperature: float = 0.2,
    default_timeout_seconds: float = 30.0,
) -> ResolvedLlmConfig:
    api_key = coalesce_str(override.api_key if override else None, os.getenv("OPENAI_API_KEY"))
    model = coalesce_str(override.model if override else None, os.getenv("OPENAI_MODEL"), default_model)
    temperature = coalesce_float(
        override.temperature if override else None,
        os.getenv("OPENAI_TEMPERATURE"),
        default_temperature,
    )
    timeout_seconds = coalesce_float(
        override.timeout_seconds if override else None,
        os.getenv("OPENAI_TIMEOUT_SECONDS"),
        default_timeout_seconds,
    )
    return ResolvedLlmConfig(
        api_key=api_key,
        base_url=_normalize_base_url(coalesce_str(override.base_url if override else None, os.getenv("OPENAI_BASE_URL"))),
        model=model or default_model,
        temperature=min(max(temperature, 0.0), 2.0),
        timeout_seconds=min(max(timeout_seconds, 1.0), 180.0),
        json_mode=coalesce_bool(override.json_mode if override else None, os.getenv("OPENAI_JSON_MODE"), True),
        organization=coalesce_str(override.organization if override else None, os.getenv("OPENAI_ORG_ID")),
        project=coalesce_str(override.project if override else None, os.getenv("OPENAI_PROJECT_ID")),
    )


def create_openai_client(config: ResolvedLlmConfig) -> Any:
    from openai import DefaultHttpxClient, OpenAI

    client_kwargs: dict[str, Any] = {
        "api_key": config.api_key,
        "timeout": config.timeout_seconds,
    }
    if config.base_url:
        client_kwargs["base_url"] = config.base_url
        if _is_loopback_url(config.base_url):
            client_kwargs["http_client"] = DefaultHttpxClient(trust_env=False)
    if config.organization:
        client_kwargs["organization"] = config.organization
    if config.project:
        client_kwargs["project"] = config.project
    return OpenAI(**client_kwargs)


def create_chat_completion(client: Any, request: dict[str, Any]) -> Any:
    try:
        return client.chat.completions.create(**request)
    except Exception:
        fallback = dict(request)
        if "response_format" not in fallback:
            raise
        fallback.pop("response_format", None)
        return client.chat.completions.create(**fallback)


def parse_json_response(content: str) -> Any:
    try:
        return json.loads(content)
    except json.JSONDecodeError:
        match = re.search(r"\{.*\}", content, flags=re.DOTALL)
        if not match:
            raise
        return json.loads(match.group(0))


def safe_llm_error(exc: Exception) -> str:
    return redact_error(exc)


def _normalize_base_url(value: str | None) -> str | None:
    if not value:
        return None
    return value.rstrip("/")


def _is_loopback_url(value: str) -> bool:
    try:
        parsed = urlparse(value)
    except ValueError:
        return False
    host = (parsed.hostname or "").lower()
    return host in {"localhost", "127.0.0.1", "::1"} or host.startswith("127.")
