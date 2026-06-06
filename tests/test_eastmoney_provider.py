import os
import tempfile
import unittest

import pandas as pd

from app.providers.eastmoney import EastmoneyProvider
from app.schemas import StockItem


class EastmoneyProviderFallbackTests(unittest.TestCase):
    def test_load_spot_uses_cache_when_live_fetch_fails(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            cache_path = os.path.join(temp_dir, "eastmoney.csv")
            pd.DataFrame(
                [
                    {
                        "f2": 10.0,
                        "f9": 20.0,
                        "f12": "600000",
                        "f14": "浦发银行",
                        "f18": 9.8,
                        "f20": 100.0,
                        "f23": 1.2,
                        "f100": "银行",
                    }
                ]
            ).to_csv(cache_path, index=False)

            provider = EastmoneyProvider(cache_path=cache_path, refresh=True)
            provider._fetch_spot = lambda: (_ for _ in ()).throw(RuntimeError("offline"))

            items, notes = provider.list_stocks_for_screen()

        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].code, "600000.SH")
        self.assertEqual(items[0].price, 9.8)
        self.assertTrue(any("已使用本地缓存" in note for note in notes))

    def test_load_spot_uses_akshare_fallback_when_no_cache(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            provider = EastmoneyProvider(cache_path=os.path.join(temp_dir, "eastmoney.csv"), refresh=True)
            provider._fetch_spot = lambda: (_ for _ in ()).throw(RuntimeError("offline"))
            provider._akshare.list_stocks = lambda: [
                StockItem(
                    code="300750.SZ",
                    name="宁德时代",
                    industry="动力电池",
                    price=195.0,
                    pe=22.0,
                    pb=4.1,
                    market_cap_billion=8500.0,
                )
            ]

            items, notes = provider.list_stocks_for_screen()

        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].code, "300750.SZ")
        self.assertEqual(items[0].price, 195.0)
        self.assertTrue(any("公开行情备用源" in note for note in notes))


if __name__ == "__main__":
    unittest.main()
