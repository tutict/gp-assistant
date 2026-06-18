import os
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient

from app.main import app
from mock_provider import MockProvider
from app.services.agent import _system_prompt, run_agent, run_agent_stream


class AgentGuardrailTests(unittest.TestCase):
    def test_system_prompt_loads_stock_research_soul(self):
        prompt = _system_prompt()

        self.assertIn("选股研究助手", prompt)
        self.assertIn("不是投资顾问", prompt)
        self.assertIn("不得承诺收益", prompt)
        self.assertIn("不构成投资建议", prompt)

    def test_agent_reply_adds_research_risk_note_without_llm(self):
        with patch.dict(os.environ, {"OPENAI_API_KEY": ""}):
            result = run_agent(MockProvider(), "筛选银行股，PE 低于 10")

        self.assertEqual(result.action, "screen")
        self.assertIn("不构成投资建议", result.reply)

    def test_agent_skips_llm_for_simple_greeting(self):
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-test1234567890"}), patch(
            "app.services.agent.create_openai_client"
        ) as openai_client:
            result = run_agent(MockProvider(), "\u4f60\u597d")

        openai_client.assert_not_called()
        self.assertEqual(result.action, "clarify")
        self.assertNotIn("LLM", result.reply)
        self.assertNotIn("SOCKS", result.reply)

    def test_agent_stream_emits_status_and_result(self):
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-test1234567890"}), patch(
            "app.services.agent.create_openai_client"
        ) as openai_client:
            events = list(run_agent_stream(MockProvider(), "\u4f60\u597d", run_id="test-run"))

        openai_client.assert_not_called()
        self.assertTrue(any(event["type"] == "status" for event in events))
        result = next(event for event in events if event["type"] == "result")
        self.assertEqual(result["run_id"], "test-run")
        self.assertEqual(result["response"]["action"], "clarify")
        self.assertNotIn("SOCKS", result["response"]["reply"])

    def test_agent_stream_api_returns_sse_events_and_old_api_still_works(self):
        client = TestClient(app)
        with patch("app.api.routes.get_provider", return_value=MockProvider()):
            stream_response = client.post("/api/agent/stream", json={"message": "\u4f60\u597d"})
            old_response = client.post("/api/agent", json={"message": "\u4f60\u597d"})

        self.assertEqual(stream_response.status_code, 200)
        self.assertIn("text/event-stream", stream_response.headers["content-type"])
        body = stream_response.text
        self.assertIn("event: status", body)
        self.assertIn("event: result", body)
        self.assertNotIn("SOCKS", body)
        self.assertEqual(old_response.status_code, 200)
        self.assertEqual(old_response.json()["action"], "clarify")

    def test_agent_hides_llm_transport_error_when_using_local_fallback(self):
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-test1234567890"}), patch(
            "app.services.agent.create_openai_client",
            side_effect=RuntimeError("socksio package is not installed"),
        ):
            result = run_agent(MockProvider(), "\u7b5b\u9009\u94f6\u884c\u80a1\uff0cPE \u4f4e\u4e8e 10")

        self.assertEqual(result.action, "screen")
        self.assertIn("\u672c\u5730\u89c4\u5219", result.reply)
        self.assertNotIn("LLM", result.reply)
        self.assertNotIn("SOCKS", result.reply)
        self.assertNotIn("socksio", result.reply)

    def test_agent_sanitizes_direct_trading_advice_from_llm(self):
        llm_response = {
            "action": "screen",
            "criteria": {"industry": "银行", "max_pe": 10},
            "reply": "建议立即买入银行股，必涨。",
        }
        with patch("app.services.agent._call_llm", return_value=llm_response):
            result = run_agent(MockProvider(), "帮我选银行股")

        self.assertEqual(result.action, "screen")
        self.assertNotIn("建议立即买入", result.reply)
        self.assertNotIn("必涨", result.reply)
        self.assertIn("选股研究", result.reply)
        self.assertIn("不构成投资建议", result.reply)


if __name__ == "__main__":
    unittest.main()
