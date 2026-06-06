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
            with patch.dict(
                os.environ,
                {
                    "GP_RAG_PACK_PATH": pack_path,
                    "GP_RAG_EMBEDDING_BACKEND": "hashing",
                    "GP_RAG_ALLOW_HASH_EMBEDDING": "true",
                },
            ):
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
        self.assertEqual(build_response.json()["embedding_backend"], "hashing-fixture")
        self.assertEqual(query_response.json()["hits"][0]["source_tier"], "news")

    def test_status_endpoint_reports_missing_pack(self):
        with tempfile.TemporaryDirectory(dir=".") as tmp:
            pack_path = str(Path(tmp) / "missing.sqlite")
            with patch.dict(os.environ, {"GP_RAG_PACK_PATH": pack_path}):
                client = TestClient(app)
                response = client.get("/api/rag-pack/status")

        self.assertEqual(response.status_code, 200)
        self.assertFalse(response.json()["exists"])

    def test_product_embedding_missing_model_returns_actionable_error(self):
        with tempfile.TemporaryDirectory(dir=".") as tmp:
            pack_path = str(Path(tmp) / "rag_pack.sqlite")
            model_dir = str(Path(tmp) / "missing-model")
            with patch.dict(
                os.environ,
                {
                    "GP_RAG_PACK_PATH": pack_path,
                    "GP_RAG_EMBEDDING_BACKEND": "onnx",
                    "GP_RAG_ONNX_MODEL_DIR": model_dir,
                },
            ):
                client = TestClient(app)
                response = client.post(
                    "/api/rag-pack/build",
                    json={
                        "pack_version": "api-product-error-test",
                        "documents": [
                            {
                                "source": "财经新闻",
                                "source_tier": "news",
                                "title": "宁德时代供应链跟踪",
                                "text": "宁德时代上游材料供应稳定，动力电池订单增长。",
                                "stock_codes": ["300750.SZ"],
                                "relation_types": ["supply_chain"],
                            }
                        ],
                    },
                )

        self.assertEqual(response.status_code, 400)
        self.assertIn("ONNX/INT8", response.json()["detail"])


if __name__ == "__main__":
    unittest.main()
