import json
import unittest
from collections import Counter
from pathlib import Path

FIXTURE = Path(__file__).resolve().parents[2] / "app" / "prompts" / "research_retrieval_eval_cases.json"
EXPECTED = {"fact_query", "multi_evidence", "temporal_validity", "source_tier_conflict", "community_gate", "no_evidence_refusal", "private_evidence", "agent_trace"}

class FrozenGoldenFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = json.loads(FIXTURE.read_text(encoding="utf-8-sig"))
        cls.cases = cls.fixture["cases"]

    def test_has_40_cases_and_balanced_categories(self):
        self.assertEqual(self.fixture["version"], "research-retrieval-eval-v5")
        self.assertEqual(len(self.cases), 40)
        self.assertEqual(self.fixture["dataset"]["corpus_version"], "research-corpus-v3")
        self.assertEqual(set(case["category"] for case in self.cases), EXPECTED)
        self.assertEqual(Counter(case["category"] for case in self.cases), Counter({category: 5 for category in EXPECTED}))

    def test_every_case_has_traceable_evidence_and_construct_contracts(self):
        for case in self.cases:
            with self.subTest(case=case["id"]):
                documents = {item["document_id"]: item for item in case.get("documents", [])}
                generated = case.get("generated_documents") or {}
                count = int(generated.get("count", 0))
                prefix = generated.get("document_id_prefix", "generated-")
                for index in range(count):
                    documents[f"{prefix}{index}"] = {
                        "content": generated.get("content", "")
                    }
                self.assertTrue(case["evidence_spans"])
                self.assertTrue(case["expect"]["required_document_ids"])
                self.assertTrue(case["expect"]["required_evidence_sentences"])
                self.assertTrue(case["expect"]["answer_contract"]["acceptable_answers"])
                self.assertEqual(
                    set(case["expect"]["answer_contract"].get("required_facts", [])),
                    {fact["claim"] for fact in case["expect"].get("answer_facts", [])},
                )
                self.assertIn("forbidden_claims", case["expect"]["answer_contract"])
                answer_facts = case["expect"].get("answer_facts")
                self.assertIsInstance(answer_facts, list)
                evidence_by_document = {}
                for span in case["evidence_spans"]:
                    evidence_by_document.setdefault(span["document_id"], set()).add(span["sentence"])
                for fact in answer_facts:
                    self.assertTrue(fact["fact_id"])
                    self.assertTrue(fact["claim"])
                    self.assertTrue(fact["required_evidence"])
                    for evidence in fact["required_evidence"]:
                        self.assertIn(evidence["document_id"], evidence_by_document)
                        self.assertIn(evidence["sentence"], evidence_by_document[evidence["document_id"]])
                for citation in case["expect"].get("required_citations", []):
                    self.assertIn(citation["fact_id"], {fact["fact_id"] for fact in answer_facts})
                self.assertTrue(case["expect"]["agent_contract"]["required_tools"])
                self.assertIn("forbidden_tools", case["expect"]["agent_contract"])
                for span in case["evidence_spans"]:
                    self.assertIn(span["document_id"], documents)
                    self.assertIn(span["sentence"], documents[span["document_id"]]["content"])

    def test_evidence_sources_have_explicit_synthetic_provenance_and_hashes(self):
        import hashlib

        for case in self.cases:
            with self.subTest(case=case["id"]):
                documents = {item["document_id"]: item["content"] for item in case.get("documents", [])}
                generated = case.get("generated_documents") or {}
                prefix = generated.get("document_id_prefix", "generated-")
                for index in range(int(generated.get("count", 0))):
                    documents[f"{prefix}{index}"] = generated.get("content", "")
                lineage = {item["document_id"]: item for item in case["provenance"].get("source_documents", [])}
                self.assertEqual(set(lineage), set(documents))
                for document_id, content in documents.items():
                    source = lineage[document_id]
                    self.assertEqual(source["source_type"], "synthetic_fixture")
                    self.assertIs(source["real_world_source"], False)
                    self.assertEqual(source["source_reference"], f"fixture:{case['id']}:{document_id}")
                    self.assertEqual(source["fixture_text_sha256"], hashlib.sha256(content.encode("utf-8")).hexdigest())

    def test_source_tier_conflict_cases_trace_both_sides_to_evidence(self):
        for case in self.cases:
            if case["category"] != "source_tier_conflict":
                continue
            with self.subTest(case=case["id"]):
                document_ids = {item["document_id"] for item in case.get("documents", [])}
                required_ids = set(case["expect"].get("required_document_ids", []))
                evidence_ids = {item["document_id"] for item in case.get("evidence_spans", [])}
                self.assertTrue(document_ids.issubset(required_ids), "conflicting sources must both be retrieved")
                self.assertTrue(document_ids.issubset(evidence_ids), "both sides of a conflict need evidence spans")
                self.assertTrue(all(any(ref["document_id"] == document_id for fact in case["expect"].get("answer_facts", []) for ref in fact.get("required_evidence", [])) for document_id in document_ids), "each compared source must support an answer fact")

    def test_answer_facts_and_non_answer_contracts_are_separated(self):
        for case in self.cases:
            with self.subTest(case=case["id"]):
                answer_claims = {fact["claim"] for fact in case["expect"].get("answer_facts", [])}
                self.assertEqual(set(case["expect"]["answer_contract"].get("required_facts", [])), answer_claims)
                for fact in case["expect"].get("answer_facts", []):
                    required_doc_ids = {evidence["document_id"] for evidence in fact["required_evidence"]}
                    cited_doc_ids = {citation["document_id"] for citation in case["expect"].get("required_citations", []) if citation["fact_id"] == fact["fact_id"]}
                    self.assertTrue(required_doc_ids.issubset(cited_doc_ids), f"{fact['fact_id']} must cite all evidence sources")

    def test_temporal_cases_freeze_cutoff_order_and_future_exclusions(self):
        for case in self.cases:
            if case["category"] != "temporal_validity":
                continue
            with self.subTest(case=case["id"]):
                self.assertEqual(case["temporal_policy"]["as_of"], case["request"]["as_of"])
                self.assertTrue(case["expect"].get("first_document_id"), "temporal evidence order must be explicit")
                source_ids = {document["document_id"] for document in case.get("documents", [])}
                self.assertTrue(set(case["expect"]["required_document_ids"]).issubset(source_ids))
                self.assertTrue(set(case["expect"].get("forbidden_document_ids", [])).issubset(source_ids))
        notice = next(case for case in self.cases if case["id"] == "temporal_validity_05")
        self.assertEqual(notice["expect"]["first_document_id"], "time-5a")
        self.assertIn("time-5future", notice["expect"]["forbidden_document_ids"])

    def test_hybrid_case_identifies_relevant_evidence_and_lexical_distractor_order(self):
        case = next(item for item in self.cases if item["id"] == "hybrid_rrf_shared_candidate")
        contract = case["expect"]["retrieval_relevance_contract"]
        self.assertEqual(set(contract["relevant_document_ids"]), {"hybrid-shared", "hybrid-vector-only"})
        self.assertEqual(contract["irrelevant_document_ids"], ["hybrid-lexical-only"])
        self.assertTrue(contract["relevant_before_irrelevant"])
        spans = {(span["document_id"], span["sentence"]) for span in case["expect"]["required_evidence_sentences"]}
        self.assertEqual({document_id for document_id, _ in spans}, set(contract["relevant_document_ids"]))

    def test_specialized_policies_are_explicit(self):
        for case in self.cases:
            if case["category"] == "temporal_validity":
                self.assertIn("temporal_policy", case)
            if case["category"] == "private_evidence":
                self.assertIn("privacy_policy", case)
        self.assertTrue(any(case.get("trust_boundary") == "retrieved_document_is_untrusted_input" for case in self.cases if case["category"] == "agent_trace"))

if __name__ == "__main__":
    unittest.main()
