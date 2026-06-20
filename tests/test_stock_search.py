import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient

from app.main import app
from app.schemas import StockItem
from app.services.stock_search import search_stock_items


class StockSearchTests(unittest.TestCase):
    def setUp(self):
        self.stocks = [
            StockItem(code="300750.SZ", name="宁德时代", industry="动力电池", price=195.0),
            StockItem(code="002594.SZ", name="比亚迪", industry="汽车整车", price=250.0),
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

    def test_can_match_chinese_name(self):
        result = search_stock_items(self.stocks, "茅台", limit=3)

        self.assertEqual([item.code for item in result], ["600519.SH"])

    def test_can_match_chinese_name_prefix(self):
        result = search_stock_items(self.stocks, "比亚", limit=3)

        self.assertEqual([item.code for item in result], ["002594.SZ"])

    def test_can_match_name_by_ordered_shortcut(self):
        result = search_stock_items(self.stocks, "宁时", limit=3)

        self.assertEqual([item.code for item in result], ["300750.SZ"])

    def test_can_match_industry_keyword(self):
        result = search_stock_items(self.stocks, "白酒", limit=3)

        self.assertEqual([item.code for item in result], ["600519.SH"])


class _SearchProvider:
    name = "test"

    def __init__(self, stocks):
        self._stocks = stocks

    def list_stocks(self):
        return self._stocks


class StockSearchApiTests(unittest.TestCase):
    def test_stock_search_endpoint_accepts_chinese_name_query(self):
        stocks = [
            StockItem(code="002594.SZ", name="比亚迪", industry="汽车整车", price=250.0),
            StockItem(code="300750.SZ", name="宁德时代", industry="动力电池", price=195.0),
        ]

        with patch("app.api.routes.get_provider", return_value=_SearchProvider(stocks)):
            response = TestClient(app).get("/api/stock-search", params={"q": "比亚", "limit": 5})

        self.assertEqual(response.status_code, 200)
        payload = response.json()
        self.assertEqual(payload[0]["code"], "002594.SZ")
        self.assertEqual(payload[0]["name"], "比亚迪")

if __name__ == "__main__":
    unittest.main()