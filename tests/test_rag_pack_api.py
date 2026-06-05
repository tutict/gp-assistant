import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient

from app.main import app


class RagPackApiTests(unittest.TestCase):
    def test_build_and_query_rag_pack_endpoints(self):
        with tempfile.TemporaryDirectory(dir=".") as tmp:
            pack_path = str(Path(tmp) / "rag_pack.sqlite")
            with patch.dict(os.environ, {"GP_RAG_PACK_PATH": pack_path}):
                client = TestClient(app)
                build_response = client.post(
                    "/api/rag-pack/build",
                    json={
                        "pack_version": "api-test",
                        "target_chars": 120,
                        "overlap_chars": 20,
                        "documents": [
                            {
                                "source": "财经新闻",
                                "source_tier": "news",
                                "title": "宁德时代供应链跟踪",
                                "text": "宁德时代上游材料供应稳定，动力电池订单增长。",
                                "stock_codes": ["300750.SZ"],
                                "relation_types": ["supply_chain"],
                                "published_at": "2026-06-01",
                                "sentiment": "positive",
                            }
                        ],
                    },
                )
                query_response = client.post(
                    "/api/rag-pack/query",
                    json={
                        "query": "宁德时代订单增长",
                        "stock_codes": ["300750"],
                        "top_k": 3,
                    },
                )

        self.assertEqual(build_response.status_code, 200)
        self.assertEqual(query_response.status_code, 200)
        self.assertEqual(build_response.json()["chunk_count"], 1)
        self.assertEqual(query_response.json()["hits"][0]["source_tier"], "news")


if __name__ == "__main__":
    unittest.main()
