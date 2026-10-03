import os
import unittest
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