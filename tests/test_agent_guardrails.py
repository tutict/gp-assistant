import os
import unittest
from unittest.mock import patch

from mock_provider import MockProvider
from app.services.agent import _system_prompt, run_agent


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
