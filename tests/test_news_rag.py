import os
import sqlite3
import tempfile
import unittest
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

from app.providers.mock import MockProvider
from app.schemas import NewsRagRequest, StockItem
from app.services import news_rag
from app.services.agent import run_agent


class NewsRagTests(unittest.TestCase):
    def test_supply_chain_news_uses_existing_relations_and_cache(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, {"GP_NEWS_ENABLE_GUBA": "false"}):
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
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, {"GP_NEWS_ENABLE_GUBA": "false"}):
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
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
