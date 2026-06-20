import os
import sqlite3
import tempfile
import unittest
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

from mock_provider import MockProvider
from app.schemas import LlmClientConfig, NewsRagRequest, StockItem
from app.services import news_rag
from app.services.agent import run_agent


DISABLE_NETWORK_NEWS = {
    "GP_NEWS_ENABLE_GUBA": "false",
    "GP_NEWS_ENABLE_AKSHARE": "false",
    "GP_NEWS_ENABLE_XUEQIU": "false",
}


def seed_news_cache() -> None:
    conn = news_rag._connect()
    try:
        news_rag._init_db(conn)
        news_rag._store_news(
            conn,
            [
                news_rag.RawNewsItem(
                    title="宁德时代供应链订单预期改善",
                    summary="公开消息显示动力电池供应链订单预期改善，需结合交付和毛利率继续验证。",
                    source="测试新闻源",
                    url="local://test-news/300750/002594",
                    published_at=datetime.now().isoformat(timespec="seconds"),
                    stock_codes=("300750.SZ", "002594.SZ"),
                    industries=("动力电池", "汽车"),
                    relation_types=("supply_chain",),
                    sentiment="positive",
                )
            ],
        )
    finally:
        conn.close()


class NewsRagTests(unittest.TestCase):
    def test_supply_chain_news_uses_existing_relations_and_cache(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            DISABLE_NETWORK_NEWS,
        ):
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            seed_news_cache()
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

    def test_plain_news_groups_without_llm_include_target_and_upstream_messages(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            DISABLE_NETWORK_NEWS,
        ):
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            conn = news_rag._connect()
            try:
                news_rag._init_db(conn)
                news_rag._store_news(
                    conn,
                    [
                        news_rag.RawNewsItem(
                            title="宁德时代供应链订单改善",
                            summary="公开消息显示目标股订单改善。",
                            source="测试新闻源",
                            url="local://positive-target",
                            published_at=datetime.now().isoformat(timespec="seconds"),
                            stock_codes=("300750.SZ",),
                            industries=("动力电池",),
                            relation_types=("supply_chain",),
                            sentiment="positive",
                        ),
                        news_rag.RawNewsItem(
                            title="上游材料成本承压",
                            summary="上游材料涨价可能压缩盈利。",
                            source="测试新闻源",
                            url="local://negative-upstream",
                            published_at=datetime.now().isoformat(timespec="seconds"),
                            stock_codes=("600309.SH",),
                            industries=("化工",),
                            relation_types=("upstream_material",),
                            sentiment="negative",
                        ),
                    ],
                )
            finally:
                conn.close()

            result = news_rag.analyze_supply_chain_news(
                MockProvider(),
                NewsRagRequest(code="300750.SZ", seed_codes=["300750.SZ"], days=30, max_items=10),
            )

        self.assertEqual(result.sentiment_groups.mode, "plain_news")
        self.assertTrue(any(item.title == "宁德时代供应链订单改善" for item in result.sentiment_groups.positive))
        self.assertTrue(any(item.title == "上游材料成本承压" for item in result.sentiment_groups.negative))
        self.assertTrue(any("未接入模型" in note for note in result.notes))

    def test_news_rag_requires_explicit_target_stock(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            DISABLE_NETWORK_NEWS,
        ):
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            with patch.object(news_rag, "_fetch_news_items") as fetch_news:
                result = news_rag.analyze_supply_chain_news(
                    MockProvider(),
                    NewsRagRequest(days=30, max_items=10),
                )

        fetch_news.assert_not_called()
        self.assertEqual(result.scope_codes, [])
        self.assertEqual(result.relation_count, 0)
        self.assertEqual(result.message_count, 0)
        self.assertEqual(result.findings, [])
        self.assertTrue(any("目标股票" in note for note in result.notes))

    def test_default_sources_try_eastmoney_community_and_news(self):
        stock = StockItem(code="300750.SZ", name="宁德时代", industry="动力电池", price=195.0)
        guba_item = news_rag.RawNewsItem(
            title="宁德时代股吧讨论订单改善",
            summary="投资者讨论订单改善，需要公告验证。",
            source="东方财富股吧",
            url="local://guba",
            published_at=datetime.now().isoformat(timespec="seconds"),
            stock_codes=("300750.SZ",),
            industries=("动力电池",),
            relation_types=("supply_chain",),
            sentiment="positive",
            source_tier="community",
        )
        news_item = news_rag.RawNewsItem(
            title="宁德时代获供应链订单",
            summary="公开新闻提到供应链订单。",
            source="AkShare 东方财富个股新闻",
            url="local://news",
            published_at=datetime.now().isoformat(timespec="seconds"),
            stock_codes=("300750.SZ",),
            industries=("动力电池",),
            relation_types=("supply_chain",),
            sentiment="positive",
        )

        class FakeGubaAdapter:
            errors: list[str] = []

            def fetch(self, stocks, relations, days):
                return [guba_item]

        class FakeAkshareAdapter:
            def fetch(self, stocks, relations, days):
                return [news_item]

        with patch.dict(os.environ, {"GP_NEWS_ENABLE_XUEQIU": "false"}), patch.object(
            news_rag,
            "_EastmoneyGubaCommunityAdapter",
            FakeGubaAdapter,
        ), patch.object(news_rag, "_AkshareStockNewsAdapter", FakeAkshareAdapter):
            os.environ.pop("GP_NEWS_ENABLE_GUBA", None)
            os.environ.pop("GP_NEWS_ENABLE_AKSHARE", None)
            items, notes = news_rag._fetch_news_items([stock], [], 30)

        self.assertEqual([item.source_tier for item in items], ["community", "news"])
        self.assertTrue(any("东方财富股吧" in note for note in notes))
        self.assertTrue(any("东方财富个股新闻" in note for note in notes))

    def test_news_rag_uses_llm_for_final_impact_judgment(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            DISABLE_NETWORK_NEWS,
        ):
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            seed_news_cache()
            with patch.object(
                news_rag,
                "_call_news_llm",
                return_value={
                    "findings": [
                        {
                            "target": "宁德时代（300750.SZ）",
                            "direction": "中性",
                            "confidence": "中",
                            "impact_chain": "模型认为订单消息仍需交付和毛利率验证。",
                            "pending_checks": ["核对公告订单金额。"],
                        }
                    ],
                    "notes": ["模型仅引用已有证据。"],
                },
            ) as llm_call:
                result = news_rag.analyze_supply_chain_news(
                    MockProvider(),
                    NewsRagRequest(
                        code="300750.SZ",
                        seed_codes=["300750.SZ"],
                        days=30,
                        max_items=10,
                        llm=LlmClientConfig(api_key="test-key", model="test-model"),
                    ),
                )

        llm_call.assert_called_once()
        self.assertEqual(result.findings[0].direction, "中性")
        self.assertEqual(result.findings[0].confidence, "中")
        self.assertTrue(any("test-model" in note for note in result.notes))
        self.assertTrue(any("模型仅引用已有证据" in note for note in result.notes))

    def test_agent_routes_upstream_news_to_news_rag(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            DISABLE_NETWORK_NEWS,
        ):
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            seed_news_cache()
            result = run_agent(MockProvider(), "分析 300750 上下游利好消息")

        self.assertEqual(result.action, "news_rag")
        self.assertIsNotNone(result.news_rag)
        self.assertIsNotNone(result.data)
        self.assertGreaterEqual(result.data["message_count"], 1)

    def test_cache_migration_defaults_old_rows_to_news_tier(self):
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "news.sqlite"
            conn = sqlite3.connect(db_path)
            conn.row_factory = sqlite3.Row
            conn.execute(
                """
                CREATE TABLE news_items (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    title TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    url TEXT,
                    published_at TEXT,
                    fetched_at TEXT NOT NULL,
                    sentiment TEXT NOT NULL
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE news_entities (
                    news_id TEXT NOT NULL,
                    entity_type TEXT NOT NULL,
                    entity_value TEXT NOT NULL,
                    relation_type TEXT,
                    PRIMARY KEY (news_id, entity_type, entity_value, relation_type)
                )
                """
            )
            now = datetime.now().isoformat(timespec="seconds")
            conn.execute(
                """
                INSERT INTO news_items
                (id, source, title, summary, url, published_at, fetched_at, sentiment)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                ("old-row", "旧新闻源", "旧缓存新闻", "旧缓存摘要", "local://old", now, now, "positive"),
            )
            conn.execute(
                """
                INSERT INTO news_entities
                (news_id, entity_type, entity_value, relation_type)
                VALUES (?, ?, ?, ?)
                """,
                ("old-row", "stock", "300750.SZ", ""),
            )
            conn.commit()

            news_rag._init_db(conn)
            evidence = news_rag._query_evidence(conn, ["300750.SZ"], 30, 5)
            conn.close()

        self.assertEqual(evidence[0].source_tier, "news")

    def test_query_cached_evidence_exposes_public_cache_lookup(self):
        with tempfile.TemporaryDirectory() as tmp:
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            seed_news_cache()
            evidence = news_rag.query_cached_evidence(["300750"], 30, 5)

        self.assertTrue(evidence)
        self.assertIn("300750.SZ", evidence[0].stock_codes)

    def test_guba_article_parser_marks_community_evidence(self):
        html = """
        <script>var post_article={
          "post_id":1628986640,
          "post_guba":{"stockbar_code":"688795","stockbar_name":"摩尔线程-U"},
          "post_title":"摩尔线程由于巨亏和发行价偏高，估计破发，许多人会放弃申购。",
          "post_content":"<div><p>摩尔线程累计亏损，许多人不敢冒险。</p></div>",
          "post_abstract":"摩尔线程累计亏损，估计破发。",
          "post_publish_time":"2025-11-23 11:57:41"
        };</script>
        """
        adapter = news_rag._EastmoneyGubaCommunityAdapter()
        item = adapter._parse_post_article(
            html,
            "https://guba.eastmoney.com/news,688795,1628986640.html",
            {},
            {"688795.SH": {"risk_signal"}},
        )

        self.assertIsNotNone(item)
        self.assertEqual(item.source, "东方财富股吧")
        self.assertEqual(item.source_tier, "community")
        self.assertEqual(item.stock_codes, ("688795.SH",))
        self.assertEqual(item.relation_types, ("risk_signal",))
        self.assertEqual(item.sentiment, "negative")

    def test_guba_list_parser_marks_community_evidence(self):
        html = """
        <script>var article_list={
          "re":[{
            "post_id":1721706782,
            "post_title":"花旗：升宁德时代目标价至888港元",
            "stockbar_code":"300750",
            "user_nickname":"宁德时代资讯",
            "post_click_count":1617,
            "post_comment_count":5,
            "post_publish_time":"2026-06-05 17:05:20"
          }],
          "count":1
        }; var other_list={"re":[]};</script>
        """
        stock = StockItem(code="300750.SZ", name="宁德时代", industry="动力电池", price=195.0)
        adapter = news_rag._EastmoneyGubaCommunityAdapter()
        items = adapter._parse_article_list(html, stock, {"300750.SZ": {"supply_chain"}})

        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].source_tier, "community")
        self.assertEqual(items[0].stock_codes, ("300750.SZ",))
        self.assertIn("评论：5", items[0].summary)

    def test_configured_guba_urls_keep_commas_inside_url(self):
        url = "https://guba.eastmoney.com/news,688795,1628986640.html"
        with patch.dict(os.environ, {"GP_NEWS_GUBA_URLS": url}):
            self.assertEqual(news_rag._configured_urls("GP_NEWS_GUBA_URLS"), [url])

    def test_guba_adapter_uses_safe_defaults_for_bad_env_values(self):
        with patch.dict(
            os.environ,
            {
                "GP_NEWS_GUBA_TIMEOUT": "bad",
                "GP_NEWS_GUBA_MAX_STOCKS": "bad",
                "GP_NEWS_GUBA_MAX_POSTS": "-3",
            },
        ):
            adapter = news_rag._EastmoneyGubaCommunityAdapter()

        self.assertEqual(adapter.timeout, 6.0)
        self.assertEqual(adapter.max_stocks, 6)
        self.assertEqual(adapter.max_posts_per_stock, 0)


if __name__ == "__main__":
    unittest.main()
