import unittest

from fastapi.testclient import TestClient

from app.main import app


class DataSourcesApiTests(unittest.TestCase):
    def test_data_sources_exposes_only_tdx(self):
        client = TestClient(app)

        response = client.get("/api/data-sources")

        self.assertEqual(response.status_code, 200)
        body = response.json()
        self.assertEqual(body["current"], "tdx")
        self.assertEqual([item["id"] for item in body["available"]], ["tdx"])

    def test_data_sources_rejects_legacy_provider_ids(self):
        client = TestClient(app)

        response = client.get("/api/data-sources", headers={"X-Stock-Provider": "eastmoney"})

        self.assertEqual(response.status_code, 400)
        self.assertIn("不支持的数据源", response.text)


if __name__ == "__main__":
    unittest.main()
