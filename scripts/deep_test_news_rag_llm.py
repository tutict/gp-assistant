from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import threading
from contextlib import contextmanager
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from fastapi.testclient import TestClient

from app.api import routes
from app.main import app
from app.providers.base import PROXY_ENV_KEYS, StockProvider
from app.schemas import LlmClientConfig, NewsRagRequest, StockItem, StockRelation
from app.services import news_rag


DISABLE_NETWORK_NEWS = {
    "GP_NEWS_ENABLE_GUBA": "false",
    "GP_NEWS_ENABLE_AKSHARE": "false",
    "GP_NEWS_ENABLE_XUEQIU": "false",
}


class DeepNewsProvider(StockProvider):
    name = "deep-news-fixture"

    def __init__(self) -> None:
        self._stocks = {
            "300750.SZ": StockItem(
                code="300750.SZ",
                name="宁德时代",
                industry="动力电池",
                price=195.0,
                pe=25.5,
                pb=6.5,
                roe=0.18,
                market_cap_billion=900.0,
            ),
            "002594.SZ": StockItem(
                code="002594.SZ",
                name="比亚迪",
                industry="新能源汽车",
                price=246.0,
                pe=22.4,
                pb=4.8,
                roe=0.22,
                market_cap_billion=720.0,
            ),
            "600309.SH": StockItem(
                code="600309.SH",
                name="万华化学",
                industry="化工材料",
                price=78.4,
                pe=15.6,
                pb=2.6,
                roe=0.19,
                market_cap_billion=245.0,
            ),
        }

    def list_stocks(self) -> list[StockItem]:
        return list(self._stocks.values())

    def list_stocks_for_screen(self) -> tuple[list[StockItem], list[str]]:
        return self.list_stocks(), ["deep-test: 使用固定股票池。"]

    def get_stock(self, code: str) -> StockItem:
        return self._stocks[code]

    def get_history(self, code: str, start_date: str, end_date: str):  # pragma: no cover - not used here
        raise NotImplementedError

    def list_relations(self) -> list[StockRelation]:
        return [
            StockRelation(
                source_code="300750.SZ",
                target_code="002594.SZ",
                relation_type="supply_chain",
                weight=0.72,
                description="动力电池与新能源汽车需求链联动。",
            ),
            StockRelation(
                source_code="600309.SH",
                target_code="300750.SZ",
                relation_type="upstream_material",
                weight=0.42,
                description="化工材料与新能源供应链上游相关。",
            ),
        ]


class LlmProbeHandler(BaseHTTPRequestHandler):
    mode = "success"
    requests_seen: list[dict[str, Any]] = []

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0") or "0")
        payload = json.loads(self.rfile.read(length).decode("utf-8") or "{}")
        type(self).requests_seen.append(payload)
        if self.path != "/v1/chat/completions":
            self._write_json(404, {"error": {"message": f"unexpected path {self.path}"}})
            return
        if type(self).mode == "fail_500":
            self._write_json(500, {"error": {"message": "mock llm failure"}})
            return
        user_payload = _read_user_payload(payload)
        target = ((user_payload.get("rule_findings") or [{}])[0].get("target")) or "宁德时代（300750.SZ）"
        response_payload = {
            "findings": [
                {
                    "target": target,
                    "direction": "利好",
                    "confidence": "高",
                    "impact_chain": "模型基于已有供应链关系和缓存新闻判断：订单预期改善可能提振动力电池需求，但仍需核对交付和毛利率。",
                    "pending_checks": ["核对公告订单金额", "跟踪电池装机量和毛利率变化"],
                }
            ],
            "notes": ["mock-llm: 已读取关系边和证据，没有引入外部新闻。"],
        }
        self._write_json(
            200,
            {
                "id": "chatcmpl-news-rag-deep-test",
                "object": "chat.completion",
                "created": 1_786_000_000,
                "model": payload.get("model", "mock-news-rag"),
                "choices": [{"index": 0, "message": {"role": "assistant", "content": json.dumps(response_payload, ensure_ascii=False)}, "finish_reason": "stop"}],
            },
        )

    def log_message(self, _format: str, *args: Any) -> None:
        return

    def _write_json(self, status: int, payload: dict[str, Any]) -> None:
        raw = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json; charset=utf-8")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def _read_user_payload(payload: dict[str, Any]) -> dict[str, Any]:
    messages = payload.get("messages") or []
    for message in messages:
        if message.get("role") != "user":
            continue
        try:
            parsed = json.loads(message.get("content") or "{}")
        except json.JSONDecodeError:
            return {}
        return parsed if isinstance(parsed, dict) else {}
    return {}


@contextmanager
def patched_environment(cache_path: Path, provider: StockProvider):
    old_cache = news_rag.CACHE_PATH
    old_get_provider = routes.get_provider
    old_env = {key: os.environ.get(key) for key in DISABLE_NETWORK_NEWS}
    old_proxy_env = {key: os.environ.get(key) for key in PROXY_ENV_KEYS}
    try:
        news_rag.CACHE_PATH = cache_path
        routes.get_provider = lambda *_args, **_kwargs: provider
        os.environ.update(DISABLE_NETWORK_NEWS)
        for key in PROXY_ENV_KEYS:
            os.environ.pop(key, None)
        os.environ["NO_PROXY"] = "127.0.0.1,localhost"
        os.environ["no_proxy"] = "127.0.0.1,localhost"
        yield
    finally:
        news_rag.CACHE_PATH = old_cache
        routes.get_provider = old_get_provider
        for key, value in old_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
        for key, value in old_proxy_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


@contextmanager
def llm_probe_server():
    LlmProbeHandler.mode = "success"
    LlmProbeHandler.requests_seen = []
    server = ThreadingHTTPServer(("127.0.0.1", 0), LlmProbeHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}/v1", LlmProbeHandler
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=3)


def seed_news_cache() -> None:
    conn = news_rag._connect()
    try:
        news_rag._init_db(conn)
        news_rag._store_news(
            conn,
            [
                news_rag.RawNewsItem(
                    title="宁德时代供应链订单预期改善",
                    summary="公开消息显示动力电池供应链订单预期改善，需结合交付、产能利用率和毛利率继续验证。",
                    source="深测新闻源",
                    url="local://deep-news/300750/supply-chain-order",
                    published_at=datetime.now().isoformat(timespec="seconds"),
                    stock_codes=("300750.SZ", "002594.SZ"),
                    industries=("动力电池", "新能源汽车"),
                    relation_types=("supply_chain",),
                    sentiment="positive",
                ),
                news_rag.RawNewsItem(
                    title="上游材料价格波动影响电池成本",
                    summary="上游材料价格仍有波动，可能影响电池企业短期毛利率，需要继续跟踪采购成本。",
                    source="深测新闻源",
                    url="local://deep-news/300750/upstream-material-risk",
                    published_at=datetime.now().isoformat(timespec="seconds"),
                    stock_codes=("600309.SH", "300750.SZ"),
                    industries=("化工材料", "动力电池"),
                    relation_types=("upstream_material",),
                    sentiment="uncertain",
                ),
            ],
        )
    finally:
        conn.close()


def assert_true(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def request_payload(base_url: str) -> dict[str, Any]:
    return {
        "code": "300750.SZ",
        "seed_codes": ["300750.SZ"],
        "days": 30,
        "max_items": 10,
        "llm": {
            "api_key": "sk-deep-test",
            "base_url": base_url,
            "model": "mock-news-rag",
            "temperature": 0,
            "timeout_seconds": 5,
            "json_mode": True,
        },
    }


def assert_llm_request_shape(payload: dict[str, Any]) -> None:
    assert_true(payload.get("model") == "mock-news-rag", "LLM request did not use configured model.")
    assert_true(payload.get("response_format") == {"type": "json_object"}, "LLM request did not enable JSON response_format.")
    messages = payload.get("messages") or []
    assert_true(len(messages) == 2, "LLM request should contain system and user messages.")
    user_content = messages[1].get("content") or ""
    assert_true("300750.SZ" in user_content, "LLM payload missing target stock code.")
    assert_true("宁德时代供应链订单预期改善" in user_content, "LLM payload missing cached evidence title.")
    assert_true("supply_chain" in user_content, "LLM payload missing relation type.")


def assert_success_result(result: Any, label: str) -> None:
    data = result if isinstance(result, dict) else result.model_dump()
    assert_true(data["message_count"] >= 1, f"{label}: expected cached evidence.")
    assert_true(data["relation_count"] >= 1, f"{label}: expected relation graph edges.")
    assert_true(data["findings"], f"{label}: expected findings.")
    finding = data["findings"][0]
    actual = {
        "direction": finding["direction"],
        "confidence": finding["confidence"],
        "impact_chain": finding["impact_chain"],
        "pending_checks": finding["pending_checks"],
    }
    assert_true(finding["direction"] == "利好", f"{label}: LLM direction was not merged: {actual}")
    assert_true(finding["confidence"] == "高", f"{label}: LLM confidence was not merged: {actual}")
    assert_true("模型基于已有供应链关系" in finding["impact_chain"], f"{label}: LLM impact_chain was not merged: {actual}")
    notes = "\n".join(data["notes"])
    assert_true("已调用模型 mock-news-rag" in notes, f"{label}: result notes did not confirm model usage.")
    assert_true("mock-llm: 已读取关系边和证据" in notes, f"{label}: LLM notes were not propagated.")


def run_deep_test(verbose: bool = False) -> dict[str, Any]:
    provider = DeepNewsProvider()
    with tempfile.TemporaryDirectory(prefix="gp-news-rag-llm-") as tmp, llm_probe_server() as (base_url, handler):
        cache_path = Path(tmp) / "news.sqlite"
        with patched_environment(cache_path, provider):
            seed_news_cache()

            service_result = news_rag.analyze_supply_chain_news(
                provider,
                NewsRagRequest(
                    code="300750.SZ",
                    seed_codes=["300750.SZ"],
                    days=30,
                    max_items=10,
                    llm=LlmClientConfig(
                        api_key="sk-deep-test",
                        base_url=base_url,
                        model="mock-news-rag",
                        temperature=0,
                        timeout_seconds=5,
                        json_mode=True,
                    ),
                ),
            )
            assert_success_result(service_result, "service")
            assert_true(len(handler.requests_seen) == 1, "Service path did not call mock LLM exactly once.")
            assert_llm_request_shape(handler.requests_seen[-1])

            client = TestClient(app)
            api_response = client.post("/api/news-rag", json=request_payload(base_url))
            assert_true(api_response.status_code == 200, f"API returned {api_response.status_code}: {api_response.text}")
            assert_success_result(api_response.json(), "api")
            assert_true(len(handler.requests_seen) == 2, "API path did not call mock LLM exactly once.")
            assert_llm_request_shape(handler.requests_seen[-1])
            successful_call_count = len(handler.requests_seen)

            handler.mode = "fail_500"
            fail_response = client.post("/api/news-rag", json=request_payload(base_url))
            assert_true(fail_response.status_code == 200, f"Failure fallback returned {fail_response.status_code}: {fail_response.text}")
            fail_data = fail_response.json()
            fail_notes = "\n".join(fail_data["notes"])
            assert_true("RAG 模型分析失败" in fail_notes, "Failure fallback did not report model failure.")
            assert_true(fail_data["findings"], "Failure fallback should keep local-rule findings.")

            summary = {
                "status": "PASS",
                "mock_llm_url": base_url,
                "successful_llm_calls": successful_call_count,
                "failure_retry_calls": len(handler.requests_seen) - successful_call_count,
                "total_llm_http_requests": len(handler.requests_seen),
                "service": {
                    "scope_codes": service_result.scope_codes,
                    "relation_count": service_result.relation_count,
                    "message_count": service_result.message_count,
                    "direction": service_result.findings[0].direction,
                    "confidence": service_result.findings[0].confidence,
                },
                "api": {
                    "relation_count": api_response.json()["relation_count"],
                    "message_count": api_response.json()["message_count"],
                    "direction": api_response.json()["findings"][0]["direction"],
                    "confidence": api_response.json()["findings"][0]["confidence"],
                },
                "failure_fallback_note": next(note for note in fail_data["notes"] if "RAG 模型分析失败" in note),
            }
            if verbose:
                summary["last_llm_request"] = handler.requests_seen[-2]
            return summary


def main() -> int:
    parser = argparse.ArgumentParser(description="Deep-test News RAG LLM integration with a local OpenAI-compatible mock server.")
    parser.add_argument("--verbose", action="store_true", help="Print captured LLM request payload.")
    args = parser.parse_args()
    try:
        summary = run_deep_test(verbose=args.verbose)
    except Exception as exc:
        print(json.dumps({"status": "FAIL", "error": str(exc)}, ensure_ascii=False, indent=2), file=sys.stderr)
        return 1
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
