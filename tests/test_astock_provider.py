import unittest
from unittest.mock import patch

from app.providers.astock import AStockDataProvider
from app.schemas import StockItem


class AStockDataProviderScreenTests(unittest.TestCase):
    def test_invalid_numeric_env_uses_safe_defaults(self):
        with patch.dict(
            "os.environ",
            {
                "ASTOCK_TIMEOUT": "bad",
                "ASTOCK_TENCENT_BATCH_SIZE": "bad",
            },
        ):
            provider = AStockDataProvider()

        self.assertEqual(provider.timeout, 10)
        self.assertEqual(provider._tencent.batch_size, 80)

    def test_list_stocks_for_screen_uses_tencent_previous_close(self):
        provider = AStockDataProvider()
        provider.list_stocks = lambda: [
            StockItem(
                code="600000.SH",
                name="浦发银行",
                industry="银行",
                price=10.0,
                pe=20.0,
                pb=1.0,
                market_cap_billion=100.0,
            )
        ]
        provider._tencent_quote = lambda codes: {
            "600000": {
                "name": "浦发银行",
                "price": 12.0,
                "last_close": 11.0,
                "pe_ttm": 24.0,
                "pb": 1.2,
                "mcap_yi": 120.0,
            }
        }
        provider._tdx_quotes_batched = lambda codes: ({}, 0, None)

        items, notes = provider.list_stocks_for_screen()

        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].price, 11.0)
        self.assertAlmostEqual(items[0].pe or 0, 22.0)
        self.assertAlmostEqual(items[0].pb or 0, 1.1)
        self.assertAlmostEqual(items[0].market_cap_billion or 0, 110.0)
        self.assertIn("腾讯昨收", notes[0])

    def test_list_stocks_for_screen_uses_tdx_when_tencent_is_missing(self):
        provider = AStockDataProvider()
        provider.list_stocks = lambda: [
            StockItem(
                code="600000.SH",
                name="浦发银行",
                industry="银行",
                price=10.0,
                pe=20.0,
                pb=1.0,
                market_cap_billion=100.0,
            )
        ]
        provider._tencent_quote = lambda codes: {}
        provider._tdx_quotes_batched = lambda codes: (
            {
                "600000": {
                    "name": "浦发银行",
                    "price": 12.0,
                    "last_close": 11.0,
                }
            },
            0,
            None,
        )

        items, notes = provider.list_stocks_for_screen()

        self.assertEqual(len(items), 1)
        self.assertEqual(items[0].price, 11.0)
        self.assertAlmostEqual(items[0].pe or 0, 22.0)
        self.assertAlmostEqual(items[0].pb or 0, 1.1)
        self.assertAlmostEqual(items[0].market_cap_billion or 0, 110.0)
        self.assertIn("通达信 1 只", notes[0])

    def test_list_stocks_for_screen_falls_back_when_market_quotes_are_unavailable(self):
        original = StockItem(
            code="600000.SH",
            name="浦发银行",
            industry="银行",
            price=10.0,
            pe=20.0,
            pb=1.0,
            market_cap_billion=100.0,
        )
        provider = AStockDataProvider()
        provider.list_stocks = lambda: [original]

        def raise_quote(_codes):
            raise RuntimeError("offline")

        provider._tencent_quote = raise_quote
        provider._tdx_quotes_batched = lambda codes: ({}, 0, "通达信补充行情未启用：请安装 pytdx。")

        items, notes = provider.list_stocks_for_screen()

        self.assertEqual(items, [original])
        self.assertTrue(any("腾讯与通达信行情不可用" in note for note in notes))


if __name__ == "__main__":
    unittest.main()
