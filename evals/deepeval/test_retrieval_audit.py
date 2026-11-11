import copy
import unittest

import runner


class RetrievalAuditTests(unittest.TestCase):
    def make_case(self):
        return {
            "retrieval_relevance_contract": {
                "relevant_document_ids": ["filing", "context"],
                "irrelevant_document_ids": ["noise"],
                "low_information_document_ids": ["noise", "not-retrieved"],
                "relevant_before_irrelevant": True,
            },
            "replay": {"response": {"citations": [
                {"document_id": "filing", "chunk_id": "filing:0", "excerpt": "Orders grew."},
                {"document_id": "context", "chunk_id": "context:0", "excerpt": "Sector context."},
                {"document_id": "noise", "chunk_id": "noise:0", "excerpt": "keyword"},
                {"document_id": "filing", "chunk_id": "filing:1", "excerpt": " Orders   grew. "},
                {"document_id": "mirror", "chunk_id": "mirror:0", "excerpt": "Orders grew."},
            ]}},
        }

    def test_audit_reports_observed_labels_and_duplicate_excerpts_without_mutation(self):
        case = self.make_case()
        before = copy.deepcopy(case)
        audit = runner.retrieval_audit(case)
        self.assertEqual(case, before)
        self.assertEqual(audit["scope"], "returned_citations_only")
        self.assertFalse(audit["affects_gate"])
        self.assertEqual(audit["annotation_source"], "synthetic_fixture_not_human_reviewed")
        self.assertEqual(audit["citation_count"], 5)
        self.assertEqual(audit["unique_document_count"], 4)
        self.assertEqual(audit["same_document_extra_citation_count"], 1)
        self.assertEqual(audit["low_information_document_ids"], ["noise"])
        self.assertEqual(audit["low_information_citation_ranks"], [3])
        self.assertEqual(audit["irrelevant_citation_ranks"], [3])
        self.assertFalse(audit["relevant_before_irrelevant"])
        self.assertEqual(audit["duplicate_excerpt_extra_citation_count"], 2)
        self.assertEqual(audit["duplicate_excerpt_groups"], [[
            {"rank": 1, "document_id": "filing", "chunk_id": "filing:0"},
            {"rank": 4, "document_id": "filing", "chunk_id": "filing:1"},
            {"rank": 5, "document_id": "mirror", "chunk_id": "mirror:0"},
        ]])

    def test_relevant_before_irrelevant_is_observed_not_assumed(self):
        case = self.make_case()
        case["replay"]["response"]["citations"] = case["replay"]["response"]["citations"][:3]
        self.assertTrue(runner.retrieval_audit(case)["relevant_before_irrelevant"])
        case["replay"]["response"]["citations"].pop()
        self.assertIsNone(runner.retrieval_audit(case)["relevant_before_irrelevant"])

    def test_unannotated_short_text_is_not_classified_as_low_information(self):
        case = self.make_case()
        del case["retrieval_relevance_contract"]
        audit = runner.retrieval_audit(case)
        self.assertEqual(audit["annotation_source"], "not_annotated")
        self.assertEqual(audit["low_information_document_ids"], [])
        self.assertEqual(audit["irrelevant_citation_ranks"], [])
        self.assertIsNone(audit["relevant_before_irrelevant"])

    def test_empty_excerpts_and_different_facts_are_not_duplicates(self):
        case = {"replay": {"response": {"citations": [
            {"document_id": "a", "excerpt": ""},
            {"document_id": "b", "excerpt": " "},
            {"document_id": "c", "excerpt": "Orders grew 10%."},
            {"document_id": "d", "excerpt": "Orders grew 11%."},
        ]}}}
        audit = runner.retrieval_audit(case)
        self.assertEqual(audit["duplicate_excerpt_groups"], [])
        self.assertEqual(audit["duplicate_excerpt_extra_citation_count"], 0)

    def test_missing_citations_are_not_claimed_to_be_an_empty_observed_result(self):
        self.assertEqual(runner.retrieval_audit({})["status"], "unavailable")
        audit = runner.retrieval_audit({"replay": {"response": {"citations": []}}})
        self.assertEqual(audit["status"], "observed")
        self.assertEqual(audit["citation_count"], 0)

    def test_frozen_v5_hybrid_case_exposes_unsatisfied_relevance_order(self):
        cases = runner.load_cases(runner.ROOT / "frozen_public_results.jsonl")
        case = next(case for case in cases if case["id"] == "hybrid_rrf_shared_candidate")
        audit = runner.retrieval_audit(case)
        self.assertEqual(audit["low_information_document_ids"], ["hybrid-lexical-only"])
        # This snapshot exposes a quality issue; do not rewrite the gate to hide it.
        self.assertEqual(audit["low_information_citation_ranks"], [2])
        self.assertEqual(audit["relevant_citation_ranks"], [1, 3])
        self.assertTrue(case["retrieval_relevance_contract"]["relevant_before_irrelevant"])
        self.assertFalse(audit["relevant_before_irrelevant"])
        self.assertTrue(runner.deterministic_case(case)["passed"])

    def test_frozen_capacity_fixture_exposes_exact_duplicate_excerpt_cost(self):
        cases = runner.load_cases(runner.ROOT / "frozen_public_results.jsonl")
        case = next(case for case in cases if case["id"] == "top_k_hard_cap")
        audit = runner.retrieval_audit(case)
        self.assertEqual(audit["citation_count"], 8)
        self.assertEqual(audit["unique_document_count"], 8)
        self.assertEqual(audit["same_document_extra_citation_count"], 0)
        self.assertEqual(audit["duplicate_excerpt_extra_citation_count"], 7)
        self.assertEqual(len(audit["duplicate_excerpt_groups"]), 1)
        self.assertTrue(runner.deterministic_case(case)["passed"])

    def test_deterministic_report_includes_audit_without_changing_gate(self):
        cases = runner.load_cases(runner.ROOT / "frozen_public_results.jsonl")
        case = copy.deepcopy(cases[0])
        baseline = runner.deterministic_case(case)
        case.update(self.make_case())
        report = runner.deterministic_case(case)
        self.assertEqual(report["passed"], baseline["passed"])
        self.assertEqual(report["retrieval_gate"], baseline["retrieval_gate"])
        self.assertEqual(report["retrieval_audit"]["low_information_document_ids"], ["noise"])


if __name__ == "__main__":
    unittest.main()
