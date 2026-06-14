import os
import tempfile
import unittest
from datetime import datetime
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import pandas as pd

from mock_provider import MockProvider
from app.schemas import LlmClientConfig, NewsEvidence, StockItem, StockObserveRequest
from app.services import capital_evidence, news_rag
from app.services.agent import run_agent
from app.services.observation import _default_dates, observe_stock


DISABLE_NETWORK_NEWS = {
    "GP_NEWS_ENABLE_GUBA": "false",
    "GP_NEWS_ENABLE_AKSHARE": "false",
    "GP_NEWS_ENABLE_XUEQIU": "false",
}


class ObservationCapitalBehaviorTests(unittest.TestCase):
    def test_observation_default_dates_cover_ten_trading_days(self):
        self.assertEqual(_default_dates(None, "20260614"), ("20260601", "20260614"))
        self.assertEqual(_default_dates(None, "20260615"), ("20260602", "20260615"))

    def test_observation_passes_proxy_mode_to_capital_evidence(self):
        provider = MockProvider()
        provider.proxy_mode = "none"

        with patch("app.services.observation.fetch_capital_evidence", return_value=None) as fetch:
            observe_stock(
                provider,
                StockObserveRequest(
                    code="300750.SZ",
                    start_date="20260601",
                    end_date="20260612",
                    series_limit=20,
                    minute_limit=5,
                    include_order_book=False,
                ),
            )

        self.assertEqual(fetch.call_args.kwargs["proxy_mode"], "none")

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

    def test_capital_evidence_parses_ths_fund_flow_with_short_numeric_code(self):
        stock = StockItem(code="002407.SZ", name="多氟多", industry="化学制品", price=39.67)
        fake_ak = SimpleNamespace(
            stock_fund_flow_individual=lambda symbol: pd.DataFrame(
                [
                    {
                        "股票代码": 2407,
                        "股票简称": "多氟多",
                        "最新价": 39.67,
                        "涨跌幅": "3.88%",
                        "换手率": "25.11%",
                        "流入资金": "54.35亿",
                        "流出资金": "49.96亿",
                        "净额": "4.40亿",
                        "成交额": "104.30亿",
                    }
                ]
            )
        )

        item = capital_evidence._fetch_ths_individual_fund_flow(fake_ak, stock, "20260601", "20260612")

        self.assertIsNotNone(item)
        self.assertEqual(item.source, "AkShare/同花顺个股资金流")
        self.assertEqual(item.metrics["净额"], "4.40亿")
        self.assertGreater(item.score, 60)

    def test_capital_evidence_external_path_uses_ths_fund_flow_and_lhb(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")
        calls = {"fund": 0, "lhb": 0}

        fake_ak = SimpleNamespace(
            stock_fund_flow_individual=lambda symbol: self._fake_ths_fund_frame(calls),
            stock_lhb_jgmmtj_em=lambda start_date, end_date: self._fake_lhb_frame(calls),
            stock_lhb_jgmx_sina=lambda: pd.DataFrame(),
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
            result = capital_evidence.fetch_capital_evidence(stock, "20260101", "20260612")

        self.assertEqual(calls["fund"], 1)
        self.assertEqual(calls["lhb"], 1)
        self.assertTrue(any(item.category == "fund_flow" for item in result.items))
        self.assertTrue(any(item.category == "institution_lhb" for item in result.items))

    def test_capital_evidence_falls_back_to_sina_lhb_when_eastmoney_misses(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")

        fake_ak = SimpleNamespace(
            stock_lhb_jgmmtj_em=lambda start_date, end_date: pd.DataFrame(),
            stock_lhb_jgmx_sina=lambda: pd.DataFrame(
                [
                    {
                        "\u80a1\u7968\u4ee3\u7801": "300750",
                        "\u80a1\u7968\u540d\u79f0": "\u5b81\u5fb7\u65f6\u4ee3",
                        "\u4ea4\u6613\u65e5\u671f": "2026-06-12",
                        "\u673a\u6784\u5e2d\u4f4d\u4e70\u5165\u989d": 230000000,
                        "\u673a\u6784\u5e2d\u4f4d\u5356\u51fa\u989d": 80000000,
                        "\u7c7b\u578b": "\u673a\u6784\u51c0\u4e70",
                    }
                ]
            ),
        )

        item = capital_evidence._fetch_institution_lhb(fake_ak, stock, "20260101", "20260612")

        self.assertIsNotNone(item)
        self.assertEqual(item.source, "AkShare/\u65b0\u6d6a\u9f99\u864e\u699c\u673a\u6784\u5e2d\u4f4d")
        self.assertEqual(item.metrics["\u673a\u6784\u51c0\u4e70\u989d"], "1.50 \u4ebf")
        self.assertGreater(item.score, 50)

    def test_capital_evidence_uses_fresh_cache_before_external_fetch(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")
        calls = {"lhb": 0}

        fake_ak = SimpleNamespace(
            stock_fund_flow_individual=lambda symbol: self._fake_ths_fund_frame(calls),
            stock_lhb_jgmmtj_em=lambda start_date, end_date: self._fake_lhb_frame(calls),
            stock_lhb_jgmx_sina=lambda: pd.DataFrame(),
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

        self.assertEqual(calls["lhb"], 1)
        self.assertEqual(first.freshness, "refreshed")
        self.assertEqual(second.freshness, "fresh-cache")
        self.assertEqual(second.composite_score, first.composite_score)

    def test_capital_evidence_sanitizes_external_interface_errors(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")

        class FakeAk:
            @staticmethod
            def stock_individual_fund_flow(stock, market):
                raise RuntimeError(
                    "HTTPSConnectionPool(host='push2his.eastmoney.com', port=443): "
                    "Max retries exceeded with url: /api/qt/stock/fflow/daykline/get?secid=0.002448&fields1=f1 "
                    "(Caused by ProxyError('Unable to connect to proxy', "
                    "RemoteDisconnected('Remote end closed connection without response')))"
                )

            @staticmethod
            def stock_lhb_jgmmtj_em(start_date, end_date):
                raise TimeoutError("超过 5.0s")

        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "true",
                **DISABLE_NETWORK_NEWS,
            },
        ), patch.dict("sys.modules", {"akshare": FakeAk}):
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = capital_evidence.fetch_capital_evidence(stock, "20260101", "20260612")

        status_parts = list(result.notes)
        for item in result.items:
            if item.category == "external_status":
                status_parts.extend([item.source, item.title, str(item.metrics), str(item.note)])
        status_text = "\n".join(status_parts)
        self.assertIn("外部资金接口", status_text)
        self.assertNotIn("HTTPConnectionPool", status_text)
        self.assertNotIn("push2his", status_text)
        self.assertNotIn("/api/qt/stock", status_text)
        self.assertNotIn("ProxyError", status_text)
        self.assertNotIn("RemoteDisconnected", status_text)

    def test_capital_fetch_timeout_runner_honors_direct_proxy_mode(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")

        def fetcher(_ak, _stock, _start_date, _end_date):
            return os.environ.get("HTTP_PROXY")

        with patch.dict(os.environ, {"HTTP_PROXY": "http://127.0.0.1:9999"}, clear=False):
            observed_proxy = capital_evidence._call_fetcher_with_timeout(
                fetcher,
                object(),
                stock,
                "20260101",
                "20260612",
                proxy_mode="none",
            )
            restored_proxy = os.environ.get("HTTP_PROXY")

        self.assertIsNone(observed_proxy)
        self.assertEqual(restored_proxy, "http://127.0.0.1:9999")

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
        self.assertTrue(any(section.key == "message_sentiment" for section in result.sections))
        self.assertTrue(any(section.items for section in result.sections if section.key == "message_sentiment"))

    def test_capital_evidence_fetches_news_on_cache_miss(self):
        provider = MockProvider()
        stock = provider.get_stock("300750.SZ")
        evidence = NewsEvidence(
            title="宁德时代股吧讨论升温",
            summary="东方财富股吧和个股新闻出现订单与资金关注线索。",
            source="东方财富股吧",
            source_tier="community",
            published_at=datetime.now().isoformat(timespec="seconds"),
            url="local://guba",
            stock_codes=["300750.SZ"],
            sentiment="positive",
        )
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            os.environ,
            {
                "GP_CAPITAL_ENABLE_EXTERNAL": "false",
                "GP_CAPITAL_NEWS_ON_DEMAND": "true",
            },
        ), patch.object(
            news_rag,
            "fetch_and_cache_stock_evidence",
            return_value=([evidence], ["按需抓取东方财富新闻/股吧 1 条。"]),
        ) as fetch_news:
            capital_evidence.CACHE_PATH = Path(tmp) / "capital.sqlite"
            news_rag.CACHE_PATH = Path(tmp) / "news.sqlite"
            result = capital_evidence.fetch_capital_evidence(stock, "20260101", "20260612")

        fetch_news.assert_called_once()
        self.assertTrue(any(item.category == "community_sentiment" for item in result.items))
        self.assertTrue(any(section.key == "message_sentiment" and section.items for section in result.sections))

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
    def _fake_ths_fund_frame(calls=None):
        if calls is not None:
            calls["fund"] = calls.get("fund", 0) + 1
        return pd.DataFrame(
            [
                {
                    "\u80a1\u7968\u4ee3\u7801": 300750,
                    "\u80a1\u7968\u7b80\u79f0": "\u5b81\u5fb7\u65f6\u4ee3",
                    "\u6700\u65b0\u4ef7": 394.85,
                    "\u6da8\u8dcc\u5e45": "3.31%",
                    "\u6362\u624b\u7387": "0.97%",
                    "\u6d41\u5165\u8d44\u91d1": "87.07\u4ebf",
                    "\u6d41\u51fa\u8d44\u91d1": "75.72\u4ebf",
                    "\u51c0\u989d": "11.35\u4ebf",
                    "\u6210\u4ea4\u989d": "162.79\u4ebf",
                }
            ]
        )

    @staticmethod
    def _fake_lhb_frame(calls=None):
        if calls is not None:
            calls["lhb"] += 1
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
