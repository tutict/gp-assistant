import sqlite3
import tempfile
import unittest
from pathlib import Path

from app.schemas import RagPackBuildRequest, RagPackDocument, RagPackQueryRequest
from app.services.rag_pack import build_rag_pack, chunk_document, query_rag_pack


class RagPackTests(unittest.TestCase):
    def test_chunk_document_uses_overlap_and_metadata(self):
        document = RagPackDocument(
            source="公告",
            source_tier="filing",
            title="宁德时代订单公告",
            text="宁德时代动力电池订单增长，供应链需求改善。" * 20,
            stock_codes=["300750"],
            relation_types=["supply_chain"],
            published_at="2026-06-01",
        )

        chunks = chunk_document(document, target_chars=80, overlap_chars=20)

        self.assertGreater(len(chunks), 1)
        self.assertEqual(chunks[0].source_tier, "filing")
        self.assertEqual(chunks[0].stock_codes, ("300750.SZ",))
        self.assertEqual(chunks[0].relation_types, ("supply_chain",))
        self.assertTrue(chunks[0].text[-20:] in chunks[1].text)

    def test_build_pack_writes_manifest_documents_chunks_and_embeddings(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rag_pack.sqlite"
            result = build_rag_pack(_build_request(), path=path)
            conn = sqlite3.connect(path)
            conn.row_factory = sqlite3.Row
            manifest = conn.execute("SELECT * FROM rag_manifest WHERE id = 1").fetchone()
            chunk_count = conn.execute("SELECT COUNT(*) AS count FROM chunks").fetchone()["count"]
            embedding_count = conn.execute("SELECT COUNT(*) AS count FROM chunk_embeddings").fetchone()["count"]
            conn.close()

        self.assertEqual(result.document_count, 3)
        self.assertGreaterEqual(result.chunk_count, 3)
        self.assertEqual(manifest["schema_version"], "rag-pack-v1")
        self.assertEqual(chunk_count, embedding_count)

    def test_query_pack_filters_by_stock_relation_and_source_tier(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rag_pack.sqlite"
            build_rag_pack(_build_request(), path=path)

            result = query_rag_pack(
                RagPackQueryRequest(
                    query="宁德时代 供应链 订单 增长",
                    stock_codes=["300750"],
                    relation_types=["supply_chain"],
                    source_tiers=["news", "filing"],
                    top_k=5,
                ),
                path=path,
            )

        self.assertTrue(result.hits)
        self.assertTrue(all("300750.SZ" in hit.stock_codes for hit in result.hits))
        self.assertTrue(all("supply_chain" in hit.relation_types for hit in result.hits))
        self.assertTrue(all(hit.source_tier in {"news", "filing"} for hit in result.hits))

    def test_missing_pack_returns_note_instead_of_error(self):
        result = query_rag_pack(
            RagPackQueryRequest(query="宁德时代"),
            path=Path("C:/tmp/does-not-exist-rag-pack.sqlite"),
        )

        self.assertEqual(result.hits, [])
        self.assertTrue(result.notes)


def _build_request() -> RagPackBuildRequest:
    return RagPackBuildRequest(
        pack_version="test-pack",
        target_chars=120,
        overlap_chars=20,
        documents=[
            RagPackDocument(
                source="交易所公告",
                source_tier="filing",
                title="宁德时代订单公告",
                text="宁德时代披露动力电池订单增长，供应链需求改善，产能利用率提升。",
                stock_codes=["300750.SZ"],
                relation_types=["supply_chain"],
                published_at="2026-06-01",
                url="https://example.test/filing",
                sentiment="positive",
            ),
            RagPackDocument(
                source="财经新闻",
                source_tier="news",
                title="宁德时代供应链跟踪",
                text="市场报道显示宁德时代上游材料供应稳定，动力电池交付节奏改善。",
                stock_codes=["300750.SZ"],
                relation_types=["supply_chain"],
                published_at="2026-06-02",
                url="https://example.test/news",
                sentiment="positive",
            ),
            RagPackDocument(
                source="东方财富股吧",
                source_tier="community",
                title="宁德时代讨论",
                text="社区讨论认为宁德时代短期波动较大，需要等待公告验证。",
                stock_codes=["300750.SZ"],
                relation_types=["supply_chain"],
                published_at="2026-06-03",
                url="https://example.test/community",
                sentiment="mixed",
            ),
        ],
    )


if __name__ == "__main__":
    unittest.main()
