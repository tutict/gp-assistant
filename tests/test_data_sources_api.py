import unittest

from fastapi.testclient import TestClient

from app.main import app


class DataSourcesApiTests(unittest.TestCase):
    def test_data_sources_exposes_only_tdx_and_accepts_legacy_header(self):
        client = TestClient(app)

        response = client.get("/api/data-sources", headers={"X-Stock-Provider": "eastmoney"})

        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["current"], "tdx")
        self.assertEqual(body["available"], [
            {
                "id": "tdx",
                "name": "通达信",
                "description": "通过通达信获取 A 股股票池、日线和分钟线；筛选与盘口行情优先使用腾讯股票。",
            }
        ])


if __name__ == "__main__":
    unittest.main()
