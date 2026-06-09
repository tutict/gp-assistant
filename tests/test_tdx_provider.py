import os
import tempfile
import unittest
from unittest.mock import patch

import pandas as pd

from app.providers.base import get_provider
from app.providers.tdx import TdxProvider
from app.schemas import StockItem


class TdxProviderTests(unittest.TestCase):
    def test_get_provider_maps_legacy_data_source_ids_to_tdx(self):
        with patch.dict(os.environ, {"STOCK_PROVIDER": "tdx"}):
            for source in (None, "tdx", "astock", "akshare", "eastmoney"):
                provider = get_provider(source)

                self.assertIsInstance(provider, TdxProvider)
                self.assertEqual(provider.name, "tdx")

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

    def test_list_stocks_for_screen_uses_tdx_last_close_then_cache_fallback(self):
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
                provider._quotes_batched = lambda codes: (
                    {
                        "600000": {
                            "code": "600000",
                            "name": "浦发银行",
                            "price": 10.2,
                            "last_close": 9.8,
                        }
                    },
                    0,
                    None,
                )

                items, notes = provider.list_stocks_for_screen()

        self.assertEqual([item.code for item in items], ["600000.SH", "300750.SZ"])
        self.assertEqual(items[0].price, 9.8)
        self.assertEqual(items[1].price, 195.0)
        self.assertIn("通达信前一交易日收盘价", notes[0])
        self.assertTrue(any("股票池缓存价格" in note for note in notes))

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
                    }
                ]
            ).to_csv(fundamental_cache_path, index=False)

            with patch.dict(os.environ, {"TDX_FUNDAMENTAL_CACHE": fundamental_cache_path}):
                provider = TdxProvider()
                provider.list_stocks = lambda: [
                    StockItem(code="000001.SZ", name="平安银行", industry="深市A股", price=10.0)
                ]
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
