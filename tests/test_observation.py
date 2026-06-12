import os
import tempfile
import unittest
from datetime import datetime
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import pandas as pd

from mock_provider import MockProvider
from app.schemas import LlmClientConfig, StockObserveRequest
from app.services import capital_evidence, news_rag
from app.services.agent import run_agent
from app.services.observation import observe_stock


DISABLE_NETWORK_NEWS = {
    "GP_NEWS_ENABLE_GUBA": "false",
    "GP_NEWS_ENABLE_AKSHARE": "false",
    "GP_NEWS_ENABLE_XUEQIU": "false",
}


class ObservationCapitalBehaviorTests(unittest.TestCase):
    def test_observation_includes_capital_behavior_and_rule_evidence_score(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "false",
                **DISABLE_NETWORK_NEWS,
            },
        ):
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = observe_stock(
                MockProvider(),
                StockObserveRequest(
                    code="300750.SZ",
                    start_date="20200101",
                    end_date="20200630",
                    series_limit=80,
                    minute_limit=20,
                    include_order_book=False,
                ),
            )

        self.assertIsNotNone(result.trend)
        self.assertGreater(len(result.trend.series), 20)
        latest = result.trend.series[-1]
        self.assertIsNotNone(latest.accumulation_index)
        self.assertIsNotNone(latest.accumulation_strength)
        self.assertIsNotNone(latest.swing_opportunity)
        self.assertIsNotNone(latest.rebound_signal)
        self.assertIsNotNone(latest.trend_heat)
        self.assertIsNotNone(result.trend.signal.pattern_score)
        self.assertTrue(any("吸筹分析基于日线量价" in note for note in result.trend.signal.notes))
        self.assertIsNotNone(result.capital_evidence)
        self.assertIsNotNone(result.capital_evidence.composite_score)
        self.assertFalse(result.capital_evidence.model_used)
        self.assertIn("技术推断", result.capital_evidence.contributions)
        self.assertTrue(any(item.category == "technical_behavior" for item in result.capital_evidence.items))
        self.assertTrue(any(item.category == "external_status" for item in result.capital_evidence.items))

    def test_agent_routes_capital_behavior_question_to_observation(self):
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "false",
                **DISABLE_NETWORK_NEWS,
            },
        ):
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = run_agent(MockProvider(), "看一下 300750 的四维擒龙和吸筹指标")

        self.assertEqual(result.action, "observe_stock")
        self.assertIsNotNone(result.observe)
        self.assertIsNotNone(result.data)
        self.assertIn("trend", result.data)

    def test_capital_evidence_parses_fund_flow_and_lhb_rows(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")

        class FakeAk:
            @staticmethod
            def stock_individual_fund_flow(stock, market):
                return pd.DataFrame(
                    [
                        {
                            "日期": "2026-06-12",
                            "主力净流入-净额": 120000000,
                            "超大单净流入-净额": 45000000,
                        }
                    ]
                )

            @staticmethod
            def stock_lhb_jgmmtj_em(start_date, end_date):
                return pd.DataFrame(
                    [
                        {
                            "代码": "300750",
                            "名称": "宁德时代",
                            "日期": "2026-06-12",
                            "机构买入额": 230000000,
                            "机构卖出额": 80000000,
                            "机构净买额": 150000000,
                        }
                    ]
                )

        fund_item = capital_evidence._fetch_individual_fund_flow(FakeAk, stock, "20260101", "20260612")
        lhb_item = capital_evidence._fetch_institution_lhb(FakeAk, stock, "20260101", "20260612")

        self.assertEqual(fund_item.metrics["主力净流入"], "1.20 亿")
        self.assertEqual(lhb_item.metrics["机构净买额"], "1.50 亿")
        self.assertGreater(fund_item.score, 50)
        self.assertGreater(lhb_item.score, 50)

    def test_capital_evidence_uses_fresh_cache_before_external_fetch(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")
        calls = {"fund": 0}

        fake_ak = SimpleNamespace(
            stock_individual_fund_flow=lambda stock, market: self._fake_fund_frame(calls),
            stock_lhb_jgmmtj_em=lambda start_date, end_date: self._fake_lhb_frame(),
        )

        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "true",
                **DISABLE_NETWORK_NEWS,
            },
        ), patch.dict("sys.modules", {"akshare": fake_ak}):
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            first = capital_evidence.fetch_capital_evidence(stock, "20260101", "20260612")
            second = capital_evidence.fetch_capital_evidence(stock, "20260101", "20260612")

        self.assertEqual(calls["fund"], 1)
        self.assertEqual(first.freshness, "refreshed")
        self.assertEqual(second.freshness, "fresh-cache")
        self.assertEqual(second.composite_score, first.composite_score)

    def test_capital_evidence_merges_news_cache_as_auxiliary_evidence(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "false",
                **DISABLE_NETWORK_NEWS,
            },
        ):
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            conn = news_rag._connect()
            try:
                news_rag._init_db(conn)
                news_rag._store_news(
                    conn,
                    [
                        news_rag.RawNewsItem(
                            title="宁德时代资金关注度提升",
                            summary="公开新闻提到订单和资金关注度，仍需公告核验。",
                            source="测试新闻源",
                            url="local://news",
                            published_at=datetime.now().isoformat(timespec="seconds"),
                            stock_codes=("300750.SZ",),
                            industries=("动力电池",),
                            relation_types=("supply_chain",),
                            sentiment="positive",
                        )
                    ],
                )
            finally:
                conn.close()

            result = capital_evidence.fetch_capital_evidence(stock, "20260101", "20260612")

        self.assertTrue(any(item.category == "news_rag" for item in result.items))
        self.assertIn("消息情绪", result.contributions)
        self.assertTrue(result.contributions["消息情绪"]["available"])

    def test_capital_evidence_llm_enhancement_is_optional(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "false",
                **DISABLE_NETWORK_NEWS,
            },
        ), patch.object(
            capital_evidence,
            "_call_capital_llm",
            return_value={
                "summary": "模型认为资金证据整体偏积极，但仍需核验。",
                "composite_score": 68,
                "confidence": "中",
                "notes": ["模型只引用已提供证据。"],
            },
        ) as llm_call:
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = capital_evidence.fetch_capital_evidence(
                stock,
                "20260101",
                "20260612",
                llm=LlmClientConfig(api_key="test-key", model="test-model"),
            )

        llm_call.assert_called_once()
        self.assertTrue(result.model_used)
        self.assertEqual(result.confidence, "中")
        self.assertIn("模型认为", result.summary)

    def test_effective_trade_date_uses_previous_day_before_close(self):
        self.assertEqual(
            capital_evidence.effective_trade_date("20260612", now=datetime(2026, 6, 12, 10, 0)),
            "2026-06-11",
        )
        self.assertEqual(
            capital_evidence.effective_trade_date("20260612", now=datetime(2026, 6, 12, 16, 0)),
            "2026-06-12",
        )

    @staticmethod
    def _fake_fund_frame(calls):
        calls["fund"] += 1
        return pd.DataFrame(
            [
                {
                    "日期": "2026-06-12",
                    "主力净流入-净额": 100000000,
                    "超大单净流入-净额": 30000000,
                }
            ]
        )

    @staticmethod
    def _fake_lhb_frame():
        return pd.DataFrame(
            [
                {
                    "代码": "300750",
                    "名称": "宁德时代",
                    "日期": "2026-06-12",
                    "机构买入额": 160000000,
                    "机构卖出额": 60000000,
                    "机构净买额": 100000000,
                }
            ]
        )


if __name__ == "__main__":
    unittest.main()
