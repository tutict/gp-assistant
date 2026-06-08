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

    def test_list_stocks_for_screen_uses_tdx_last_close_then_cache_fallback(self):
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
        self.assertTrue(any("已回退到股票池缓存价格" in note for note in notes))

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
