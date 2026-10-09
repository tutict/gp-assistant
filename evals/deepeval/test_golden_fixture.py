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
        self.assertEqual(self.fixture["version"], "research-retrieval-eval-v4")
        self.assertEqual(len(self.cases), 40)
        self.assertEqual(set(case["category"] for case in self.cases), EXPECTED)
        self.assertEqual(Counter(case["category"] for case in self.cases), Counter({category: 5 for category in EXPECTED}))

    def test_every_case_has_real_evidence_and_contracts(self):
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
                self.assertTrue(case["expect"]["answer_contract"]["required_facts"])
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

    def test_specialized_policies_are_explicit(self):
        for case in self.cases:
            if case["category"] == "temporal_validity":
                self.assertIn("temporal_policy", case)
            if case["category"] == "private_evidence":
                self.assertIn("privacy_policy", case)
        self.assertTrue(any(case.get("trust_boundary") == "retrieved_document_is_untrusted_input" for case in self.cases if case["category"] == "agent_trace"))

if __name__ == "__main__":
    unittest.main()
