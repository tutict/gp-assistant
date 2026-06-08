import sqlite3
import tempfile
import unittest
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

from app.providers.base import StockProvider
from app.schemas import StockItem
from app.services.upstream_rag_pack import (
    UpstreamCollectedDocument,
    UpstreamEvidenceChunk,
    UpstreamRelationEdge,
    UpstreamSourceDocument,
    build_upstream_rag_pack,
    build_upstream_pack_version,
    init_upstream_rag_schema,
    validate_upstream_pack,
)


class FakeProvider(StockProvider):
    name = "tdx"

    def get_stock(self, code: str) -> StockItem:
        return StockItem(
            code="300750.SZ",
            name="宁德时代",
            industry="电池",
            price=100,
            pe=20,
            pb=3,
            roe=0.18,
            market_cap_billion=1000,
        )


class UpstreamRagPackTests(unittest.TestCase):
    def test_quality_gate_accepts_supported_single_stock_pack(self):
        report = validate_upstream_pack(
            target_stock_code="300750.SZ",
            target_stock_name="宁德时代",
            documents=[
                UpstreamSourceDocument(
                    document_id="doc_filing",
                    source_tier="filing",
                    title="宁德时代订单公告",
                    source_name="巨潮资讯",
                    source_url="https://example.test/notice",
                ),
                UpstreamSourceDocument(
                    document_id="doc_news",
                    source_tier="news",
                    title="宁德时代供应链新闻",
                    source_name="财经媒体",
                    source_url="https://example.test/news",
                ),
            ],
            evidence_chunks=[
                UpstreamEvidenceChunk(
                    chunk_id="chunk_1",
                    document_id="doc_filing",
                    evidence_text="公告披露新签订单增长。",
                )
            ],
            relation_edges=[
                UpstreamRelationEdge(
                    edge_id="edge_1",
                    source_entity_id="entity_target",
                    target_entity_id="entity_customer",
                    relation_type="customer",
                    status="confirmed",
                    evidence_text="公告披露新签订单增长。",
                    source_ref="chunk_1",
                    confidence=0.9,
                )
            ],
        )

        self.assertTrue(report.valid)
        self.assertEqual(report.source_tier_counts["filing"], 1)
        self.assertEqual(report.fact_evidence_count, 1)

    def test_quality_gate_rejects_community_only_pack(self):
        report = validate_upstream_pack(
            target_stock_code="300750.SZ",
            target_stock_name="宁德时代",
            documents=[
                UpstreamSourceDocument(
                    document_id="doc_community",
                    source_tier="community",
                    title="股吧讨论",
                    source_name="股吧",
                    source_url="https://example.test/community",
                )
            ],
            evidence_chunks=[
                UpstreamEvidenceChunk(
                    chunk_id="chunk_1",
                    document_id="doc_community",
                    evidence_text="社区讨论称订单增长。",
                )
            ],
            relation_edges=[
                UpstreamRelationEdge(
                    edge_id="edge_1",
                    source_entity_id="entity_target",
                    target_entity_id="entity_customer",
                    relation_type="customer",
                    status="rumor",
                    evidence_text="社区讨论称订单增长。",
                    source_ref="chunk_1",
                )
            ],
        )

        self.assertFalse(report.valid)
        self.assertTrue(any("filing 或 news" in error for error in report.errors))
        self.assertTrue(any("confirmed 或 supported" in error for error in report.errors))

    def test_schema_initializer_creates_core_tables(self):
        conn = sqlite3.connect(":memory:")
        init_upstream_rag_schema(conn)
        tables = {
            row[0]
            for row in conn.execute(
                "SELECT name FROM sqlite_master WHERE type = 'table'"
            ).fetchall()
        }
        conn.close()

        self.assertIn("pack_meta", tables)
        self.assertIn("source_documents", tables)
        self.assertIn("relation_edges", tables)
        self.assertIn("light_search_index", tables)

    def test_pack_version_uses_stock_date_time_and_short_hash(self):
        version = build_upstream_pack_version(
            "300750.SZ",
            "2026-06-08",
            built_at=datetime(2026, 6, 8, 15, 30, 12),
            content_sha256="a1b2c3d4e5f6",
        )

        self.assertEqual(version, "300750.SZ_2026-06-08_153012_a1b2c3d4")

    def test_build_upstream_rag_pack_writes_manifest_and_sqlite(self):
        documents = [
            UpstreamCollectedDocument(
                source_tier="filing",
                source_name="巨潮资讯",
                title="宁德时代关于签订下游客户订单的公告",
                text="宁德时代公告披露，公司与下游客户签订动力电池订单。",
                source_url="https://example.test/notice/300750",
                published_at="2026-06-08T10:00:00",
            )
        ]

        with tempfile.TemporaryDirectory() as tmp:
            with patch("app.services.upstream_rag_pack._collect_cninfo_documents", return_value=documents), patch(
                "app.services.upstream_rag_pack._collect_akshare_news_documents", return_value=[]
            ), patch(
                "app.services.upstream_rag_pack._collect_tdx_f10_documents", return_value=[]
            ), patch("app.services.upstream_rag_pack._collect_public_url_documents", return_value=[]):
                result = build_upstream_rag_pack(
                    FakeProvider(),
                    "300750.SZ",
                    data_until="2026-06-08",
                    output_root=Path(tmp),
                )

            self.assertTrue(result.quality.valid)
            self.assertEqual(result.manifest["target_stock_code"], "300750.SZ")
            self.assertEqual(result.manifest["document_count"], 1)
            self.assertGreaterEqual(result.manifest["relation_edge_count"], 1)
            self.assertRegex(result.manifest["sha256"], r"^[0-9a-f]{64}$")
            self.assertTrue(Path(result.pack_path).exists())
            self.assertTrue(Path(result.manifest_path).exists())

            conn = sqlite3.connect(result.pack_path)
            try:
                document_count = conn.execute("SELECT COUNT(*) FROM source_documents").fetchone()[0]
                edge_count = conn.execute("SELECT COUNT(*) FROM relation_edges").fetchone()[0]
            finally:
                conn.close()
            self.assertEqual(document_count, 1)
            self.assertGreaterEqual(edge_count, 1)


if __name__ == "__main__":
    unittest.main()
