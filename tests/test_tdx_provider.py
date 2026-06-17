import os
import tempfile
import unittest
from unittest.mock import patch

import pandas as pd

from app.providers.akshare import AkShareProvider
from app.providers.base import get_provider
from app.providers.tdx import TdxProvider
from app.schemas import FinancialIndicatorItem, StockItem


class TdxProviderTests(unittest.TestCase):
    def test_get_provider_maps_legacy_data_source_ids_to_tdx(self):
        with patch.dict(os.environ, {"STOCK_PROVIDER": "tdx"}):
            for source in (None, "tdx", "astock", "akshare", "eastmoney"):
                provider = get_provider(source)

                self.assertIsInstance(provider, TdxProvider)
                self.assertEqual(provider.name, "tdx")

    def test_invalid_numeric_env_uses_safe_defaults(self):
        with patch.dict(
            os.environ,
            {
                "TDX_TIMEOUT": "bad",
                "TDX_PAGE_SIZE": "bad",
                "TDX_TENCENT_BATCH_SIZE": "bad",
                "TDX_HISTORY_BATCH_SIZE": "9999",
            },
        ):
            provider = TdxProvider()

        self.assertEqual(provider.timeout, 6)
        self.assertEqual(provider.page_size, 1000)
        self.assertEqual(provider.tencent_batch_size, 80)
        self.assertEqual(provider.history_batch_size, 800)

    def test_security_list_item_normalizes_a_share_codes(self):
        sh_stock = TdxProvider._stock_from_security_list_item(
            {"code": "600000", "name": "浦发银行", "pre_close": 9.8},
            market=1,
        )
        sz_stock = TdxProvider._stock_from_security_list_item(
            {"code": "300750", "name": "宁德时代", "pre_close": 195.0},
            market=0,
        )

        self.assertIsNotNone(sh_stock)
        self.assertEqual(sh_stock.code, "600000.SH")
        self.assertEqual(sh_stock.industry, "沪市A股")
        self.assertEqual(sh_stock.price, 9.8)
        self.assertIsNotNone(sz_stock)
        self.assertEqual(sz_stock.code, "300750.SZ")
        self.assertEqual(sz_stock.industry, "创业板")
        self.assertIsNone(TdxProvider._stock_from_security_list_item({"code": "430000", "name": "北交样本"}, 0))
        delisted = TdxProvider._stock_from_security_list_item({"code": "600001", "name": "退市样本"}, 1)
        self.assertIsNotNone(delisted)
        self.assertTrue(delisted.is_st)

    def test_list_stocks_for_screen_uses_tencent_previous_close_before_market_close(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fundamental_cache_path = os.path.join(temp_dir, "fundamentals.csv")
            pd.DataFrame(columns=["f2", "f9", "f12", "f14", "f20", "f23", "f100"]).to_csv(
                fundamental_cache_path,
                index=False,
            )
            with patch.dict(os.environ, {"TDX_FUNDAMENTAL_CACHE": fundamental_cache_path}):
                provider = TdxProvider()
                provider.list_stocks = lambda: [
                    StockItem(code="600000.SH", name="浦发银行", industry="银行", price=10.0),
                    StockItem(code="300750.SZ", name="宁德时代", industry="动力电池", price=195.0),
                ]
                provider._screen_price_policy = lambda: ("last_close", "当天未收盘，使用前一交易日收盘价")
                provider._tencent_quotes_batched = lambda codes: (
                    {
                        "600000": {
                            "code": "600000",
                            "name": "浦发银行",
                            "price": 10.2,
                            "last_close": 9.8,
                            "pe_ttm": 10.2,
                            "pb": 1.02,
                            "mcap_yi": 102.0,
                        }
                    },
                    0,
                )
                provider._quotes_batched = lambda codes: (
                    {},
                    0,
                    None,
                )

                items, notes = provider.list_stocks_for_screen()

        self.assertEqual([item.code for item in items], ["600000.SH", "300750.SZ"])
        self.assertEqual(items[0].price, 9.8)
        self.assertAlmostEqual(items[0].pe or 0, 9.8)
        self.assertAlmostEqual(items[0].pb or 0, 0.98)
        self.assertAlmostEqual(items[0].market_cap_billion or 0, 98.0)
        self.assertEqual(items[1].price, 195.0)
        self.assertIn("当天未收盘", notes[0])
        self.assertIn("腾讯 1 只", notes[0])
        self.assertTrue(any("股票池缓存价格" in note for note in notes))

    def test_list_stocks_for_screen_uses_tencent_current_price_after_market_close(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fundamental_cache_path = os.path.join(temp_dir, "fundamentals.csv")
            pd.DataFrame(columns=["f2", "f9", "f12", "f14", "f20", "f23", "f100"]).to_csv(
                fundamental_cache_path,
                index=False,
            )
            with patch.dict(os.environ, {"TDX_FUNDAMENTAL_CACHE": fundamental_cache_path}):
                provider = TdxProvider()
                provider.list_stocks = lambda: [
                    StockItem(code="600000.SH", name="浦发银行", industry="银行", price=10.0)
                ]
                provider._screen_price_policy = lambda: ("price", "当天已收盘，使用当天收盘价")
                provider._tencent_quotes_batched = lambda codes: (
                    {
                        "600000": {
                            "code": "600000",
                            "name": "浦发银行",
                            "price": 12.0,
                            "last_close": 10.0,
                            "pe_ttm": 12.0,
                            "pb": 1.2,
                            "mcap_yi": 120.0,
                        }
                    },
                    0,
                )
                provider._quotes_batched = lambda codes: ({}, 0, None)

                items, notes = provider.list_stocks_for_screen()

        self.assertEqual(items[0].price, 12.0)
        self.assertEqual(items[0].pe, 12.0)
        self.assertEqual(items[0].pb, 1.2)
        self.assertEqual(items[0].market_cap_billion, 120.0)
        self.assertIn("当天已收盘", notes[0])

    def test_list_stocks_for_screen_merges_cached_fundamentals(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            fundamental_cache_path = os.path.join(temp_dir, "fundamentals.csv")
            pd.DataFrame(
                [
                    {
                        "f2": 10.0,
                        "f9": 5.0,
                        "f12": "000001",
                        "f14": "平安银行",
                        "f20": 24_000_000_000,
                        "f23": 0.8,
                        "f100": "银行",
                        "DEDU_PARENT_PROFIT": 1_200_000_000,
                        "TOTALOPERATEREVE": 8_000_000_000,
                    }
                ]
            ).to_csv(fundamental_cache_path, index=False)

            with patch.dict(os.environ, {"TDX_FUNDAMENTAL_CACHE": fundamental_cache_path}):
                provider = TdxProvider()
                provider.list_stocks = lambda: [
                    StockItem(code="000001.SZ", name="平安银行", industry="深市A股", price=10.0)
                ]
                provider._screen_price_policy = lambda: ("price", "当天已收盘，使用当天收盘价")
                provider._tencent_quotes_batched = lambda codes: ({}, 0)
                provider._quotes_batched = lambda codes: (
                    {
                        "000001": {
                            "code": "000001",
                            "name": "平安银行",
                            "price": 11.0,
                            "last_close": 11.0,
                        }
                    },
                    0,
                    None,
                )

                items, notes = provider.list_stocks_for_screen()

        self.assertEqual(items[0].industry, "银行")
        self.assertAlmostEqual(items[0].price, 11.0)
        self.assertAlmostEqual(items[0].pe or 0, 5.5)
        self.assertAlmostEqual(items[0].pb or 0, 0.88)
        self.assertAlmostEqual(items[0].market_cap_billion or 0, 264.0)
        self.assertAlmostEqual(items[0].roe or 0, 0.16)
        self.assertAlmostEqual(items[0].deducted_net_profit_billion or 0, 12.0)
        self.assertAlmostEqual(items[0].deducted_net_profit_margin or 0, 15.0)
        self.assertTrue(any("基础指标补充" in note for note in notes))

    def test_get_financial_indicators_uses_enriched_stock_values(self):
        stock = StockItem(
            code="000001.SZ",
            name="平安银行",
            industry="银行",
            price=11.0,
            pe=5.0,
            pb=0.8,
            roe=None,
            market_cap_billion=240.0,
            dividend_yield=0.05,
        )

        section = TdxProvider().get_financial_indicators(stock)

        self.assertIsNotNone(section)
        assert section is not None
        labels = {item.label: item.value for item in section.items}
        self.assertEqual(labels["市盈率(TTM)"], "5")
        self.assertEqual(labels["市净率(最新)"], "0.8")
        self.assertEqual(labels["净资产收益率"], "16%")
        self.assertEqual(labels["市值"], "240亿")
        self.assertTrue(any("ROE 缺失" in note for note in section.notes))

    def test_akshare_quarterly_eps_items_include_period_metadata(self):
        rows = pd.DataFrame(
            [
                {"REPORT_DATE": "2026-03-31", "SEASON_LABEL": "\u4e00\u5b63\u62a5", "EPSJB": 0.88},
                {"REPORT_DATE": "2025-12-31", "SEASON_LABEL": "\u5e74\u62a5", "EPSJB": 2.64},
                {"REPORT_DATE": "2025-03-31", "SEASON_LABEL": "\u4e00\u5b63\u62a5", "EPSJB": 0.72},
            ]
        )
        items = []

        AkShareProvider._append_quarterly_eps(items, rows)

        self.assertEqual(len(items), 3)
        self.assertEqual(items[0].metric_key, "quarterly_eps")
        self.assertEqual(items[0].period, "2026Q1")
        self.assertEqual(items[0].raw_value, 0.88)
        self.assertEqual(items[0].tone, "rise")

    def test_akshare_quarterly_eps_prefers_ths_single_quarter_value(self):
        rows = pd.DataFrame(
            [
                {
                    "report_date": "2026-03-31",
                    "metric_name": "basic_eps",
                    "value": 0.30,
                    "single": 0.12,
                },
                {
                    "report_date": "2025-03-31",
                    "metric_name": "basic_eps",
                    "value": 0.20,
                    "single": 0.08,
                },
                {
                    "report_date": "2026-03-31",
                    "metric_name": "operating_income_total",
                    "value": 100,
                    "single": 100,
                },
            ]
        )
        items = []

        AkShareProvider._append_quarterly_eps(items, rows)

        self.assertEqual(len(items), 2)
        self.assertEqual(items[0].period, "2026Q1")
        self.assertEqual(items[0].raw_value, 0.12)
        self.assertEqual(items[0].tone, "rise")

    def test_tdx_financial_indicators_merge_quarterly_eps_source(self):
        stock = StockItem(
            code="300115.SZ",
            name="\u957f\u76c8\u7cbe\u5bc6",
            industry="\u6d88\u8d39\u7535\u5b50",
            price=29.0,
            pe=20.0,
            pb=3.0,
            market_cap_billion=400.0,
        )
        eps_item = FinancialIndicatorItem(
            label="2026Q1 \u6bcf\u80a1\u6536\u76ca",
            value="0.12\u5143",
            raw_value=0.12,
            unit="\u5143",
            metric_key="quarterly_eps",
            period="2026Q1",
        )

        with patch.object(
            TdxProvider,
            "_quarterly_eps_from_financial_source",
            return_value=([eps_item], "\u540c\u82b1\u987a\u8d22\u62a5(\u5355\u5b63EPS)", "2026Q1", []),
        ):
            section = TdxProvider().get_financial_indicators(stock)

        self.assertIsNotNone(section)
        assert section is not None
        self.assertIn("\u540c\u82b1\u987a\u8d22\u62a5", section.source or "")
        self.assertTrue(any(item.metric_key == "quarterly_eps" for item in section.items))

    def test_list_stocks_reads_tdx_cache_without_network(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            cache_path = os.path.join(temp_dir, "tdx_stocks.csv")
            pd.DataFrame(
                [
                    {
                        "code": "600000.SH",
                        "name": "浦发银行",
                        "industry": "银行",
                        "is_st": False,
                        "price": 9.8,
                        "pe": "",
                        "pb": "",
                        "roe": "",
                        "market_cap_billion": "",
                        "dividend_yield": "",
                    }
                ]
            ).to_csv(cache_path, index=False)

            items = TdxProvider(cache_path=cache_path).list_stocks()

        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].code, "600000.SH")
        self.assertEqual(items[0].price, 9.8)
        self.assertIsNone(items[0].pe)


if __name__ == "__main__":
    unittest.main()
