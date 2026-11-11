import os
import io
import sys
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

import runner


class ReplayContractTests(unittest.TestCase):
    def make_case(self, *, generation_eval_eligible=False):
        return {
            "id": "contract-case",
            "privacy_reviewed": True,
            "private_evidence_present": False,
            "cloud_eval_allowed": True,
            "remote_export_allowed": True,
            "retrieved_document_ids": ["doc-1"],
            "remote_retrieved_document_ids": ["doc-1"],
            "remote_forbidden_document_ids": [],
            "required_document_ids": ["doc-1"],
            "forbidden_document_ids": [],
            "request": {"remote_safe_only": False},
            "retrieval_context": ["evidence sentence"],
            "deterministic_retrieval_context": ["evidence sentence"],
            "expected_retrieval_context": ["evidence sentence"],
            "agent_trace": {"tools": [{"name": "news_evidence", "arguments": {"query": "q"}}]},
            "expected_tools": [{"name": "news_evidence", "arguments": {"query": "q"}}],
            "forbidden_tools": [],
            "generation_eval_eligible": generation_eval_eligible,
        }

    def test_deterministic_case_separates_retrieval_trace_and_generation(self):
        result = runner.deterministic_case(self.make_case())

        self.assertTrue(result["constructs"]["retrieval"]["passed"])
        self.assertTrue(result["constructs"]["trajectory"]["passed"])
        self.assertEqual(result["constructs"]["generation"]["status"], "not_applicable")
        self.assertEqual(result["constructs"]["generation"]["reason"], "deterministic replay has no product-model answer")
        self.assertTrue(result["passed"])

    def test_deterministic_cli_exits_with_failure_when_a_hard_gate_fails(self):
        case = self.make_case()
        case["dataset_version"] = runner.read_json(runner.POLICY_PATH)["dataset_version"]
        case["retrieved_document_ids"] = []
        with patch.object(sys, "argv", ["runner.py", "--mode", "deterministic", "--track", "deterministic"]), \
             patch.object(runner, "load_cases", return_value=[case]), redirect_stdout(io.StringIO()):
            self.assertEqual(runner.main(), 1)

    def local_private_case(self):
        case = self.make_case()
        case.update({
            "private_evidence_present": True, "cloud_eval_allowed": False,
            "remote_export_allowed": False, "retrieval_assertion_scope": "local",
            "remote_retrieved_document_ids": [], "remote_forbidden_document_ids": ["doc-1"],
            "request": {"remote_safe_only": True},
        })
        return case

    def test_local_required_hit_and_remote_exclusion_are_separate_assertions(self):
        case = self.local_private_case()
        self.assertTrue(runner.deterministic_case(case)["passed"])
        case["remote_retrieved_document_ids"] = ["doc-1"]
        self.assertFalse(runner.deterministic_case(case)["retrieval_gate"]["privacy_ok"])

    def test_forbidden_document_is_checked_in_the_same_projection_as_required(self):
        case = self.local_private_case()
        case["forbidden_document_ids"] = ["doc-1"]
        result = runner.deterministic_case(case)
        self.assertFalse(result["retrieval_gate"]["passed"])
        self.assertEqual(result["forbidden_document_ids_retrieved"], ["doc-1"])

    def test_empty_assertion_context_cannot_borrow_evidence_from_another_projection(self):
        case = self.make_case()
        case["deterministic_retrieval_context"] = []
        self.assertFalse(runner.deterministic_case(case)["retrieval_gate"]["passed"])

    def test_public_privacy_flags_do_not_waive_remote_forbidden_hits(self):
        case = self.make_case()
        case["remote_forbidden_document_ids"] = ["doc-1"]
        self.assertFalse(runner.deterministic_case(case)["retrieval_gate"]["privacy_ok"])

    def test_evidence_sentence_must_come_from_the_expected_document(self):
        case = self.make_case()
        case["retrieved_document_ids"] = ["doc-1", "doc-2"]
        case["deterministic_retrieval_context"] = ["unrelated excerpt", "evidence sentence"]
        case["expected_retrieval_evidence"] = [{"document_id": "doc-1", "sentence": "evidence sentence"}]
        case["replay"] = {"response": {"citations": [
            {"document_id": "doc-1", "excerpt": "unrelated excerpt"},
            {"document_id": "doc-2", "excerpt": "evidence sentence"},
        ]}}
        self.assertFalse(runner.deterministic_case(case)["retrieval_gate"]["passed"])

    def test_local_evidence_spans_use_the_local_citation_projection(self):
        case = self.local_private_case()
        case["expected_retrieval_evidence"] = [{"document_id": "doc-1", "sentence": "evidence sentence"}]
        case["replay"] = {"response": {"citations": []}}
        case["deterministic_retrieval_citations"] = [{"document_id": "doc-1", "excerpt": "evidence sentence"}]
        self.assertTrue(runner.deterministic_case(case)["retrieval_gate"]["passed"])
        case["deterministic_retrieval_citations"][0]["excerpt"] = "unrelated excerpt"
        self.assertFalse(runner.deterministic_case(case)["retrieval_gate"]["passed"])

    def test_metric_tracks_are_explicit(self):
        self.assertEqual(
            runner.metric_names_for_track("retrieval"),
            ["contextual_precision", "contextual_recall", "contextual_relevancy"],
        )
        self.assertEqual(
            runner.metric_names_for_track("generation"),
            ["faithfulness", "answer_relevancy", "acceptable_answer"],
        )
        self.assertEqual(
            runner.metric_names_for_track("full"),
            [
                "contextual_precision",
                "contextual_recall",
                "contextual_relevancy",
                "faithfulness",
                "answer_relevancy",
                "acceptable_answer",
            ],
        )

    def test_agent_config_does_not_fallback_to_judge_config(self):
        with patch.dict(
            os.environ,
            {
                "DEEPEVAL_JUDGE_BASE_URL": "https://judge.example/v1",
                "DEEPEVAL_JUDGE_API_KEY": "judge-key",
            },
            clear=True,
        ):
            with self.assertRaisesRegex(SystemExit, "RAG_EVAL_AGENT_BASE_URL"):
                runner.resolve_agent_config()

    def test_agent_config_requires_explicit_model_and_temperature(self):
        with patch.dict(
            os.environ,
            {
                "RAG_EVAL_AGENT_BASE_URL": "https://agent.example/v1",
                "RAG_EVAL_AGENT_API_KEY": "agent-key",
            },
            clear=True,
        ):
            with self.assertRaisesRegex(SystemExit, "RAG_EVAL_AGENT_MODEL"):
                runner.resolve_agent_config()


if __name__ == "__main__":
    unittest.main()
