import unittest

from app.schemas import StockItem
from app.services.stock_search import search_stock_items


class StockSearchTests(unittest.TestCase):
    def setUp(self):
        self.stocks = [
            StockItem(code="300750.SZ", name="宁德时代", industry="动力电池", price=195.0),
            StockItem(code="300014.SZ", name="亿纬锂能", industry="电池", price=38.0),
            StockItem(code="300124.SZ", name="汇川技术", industry="自动化", price=58.0),
            StockItem(code="600519.SH", name="贵州茅台", industry="白酒", price=1700.0),
        ]

    def test_returns_three_code_prefix_matches(self):
        result = search_stock_items(self.stocks, "300", limit=3)

        self.assertEqual([item.code for item in result], ["300014.SZ", "300124.SZ", "300750.SZ"])

    def test_full_code_match_wins(self):
        result = search_stock_items(self.stocks, "300750", limit=3)

        self.assertEqual(result[0].code, "300750.SZ")

    def test_can_match_name(self):
        result = search_stock_items(self.stocks, "茅台", limit=3)

        self.assertEqual([item.code for item in result], ["600519.SH"])


if __name__ == "__main__":
    unittest.main()
