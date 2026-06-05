import tempfile
import unittest
from pathlib import Path

from app.providers.mock import MockProvider
from app.schemas import NewsRagRequest
from app.services import news_rag
from app.services.agent import run_agent


class NewsRagTests(unittest.TestCase):
    def test_supply_chain_news_uses_existing_relations_and_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = news_rag.analyze_supply_chain_news(
                MockProvider(),
                NewsRagRequest(code="300750.SZ", seed_codes=["300750.SZ"], days=30, max_items=10),
            )

        self.assertEqual(result.scope_codes, ["300750.SZ"])
        self.assertGreaterEqual(result.relation_count, 1)
        self.assertGreaterEqual(result.message_count, 1)
        self.assertTrue(result.findings)
        self.assertTrue(result.findings[0].evidence)
        self.assertTrue(any("已有股票关系图" in note for note in result.notes))

    def test_agent_routes_upstream_news_to_news_rag(self):
        with tempfile.TemporaryDirectory() as tmp:
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = run_agent(MockProvider(), "分析 300750 上下游利好消息")

        self.assertEqual(result.action, "news_rag")
        self.assertIsNotNone(result.news_rag)
        self.assertIsNotNone(result.data)
        self.assertGreaterEqual(result.data["message_count"], 1)


if __name__ == "__main__":
    unittest.main()
