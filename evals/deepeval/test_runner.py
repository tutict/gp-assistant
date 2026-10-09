import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import runner


class JudgeConfigTests(unittest.TestCase):
    def test_rejects_arbitrary_results_path_even_inside_repository(self):
        with self.assertRaisesRegex(SystemExit, "approved fixed evaluation payload"):
            runner.approved_results_path(runner.ROOT / "arbitrary-results.jsonl", "research-retrieval-eval-v4")

    def test_local_env_loads_relay_url_and_key(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / ".env"
            config.write_text(
                "DEEPEVAL_JUDGE_BASE_URL=https://relay.example/v1\n"
                "DEEPEVAL_JUDGE_API_KEY='trial-secret'\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {}, clear=True):
                runner.load_local_judge_config(config)
                self.assertEqual(
                    runner.resolve_judge_config(),
                    ("trial-secret", "https://relay.example/v1"),
                )

    def test_deterministic_case_rejects_failed_tool_status(self):
        case = {
            "id": "failed-tool",
            "privacy_reviewed": True,
            "private_evidence_present": False,
            "cloud_eval_allowed": True,
            "remote_export_allowed": True,
            "retrieved_document_ids": ["doc"],
            "remote_retrieved_document_ids": ["doc"],
            "required_document_ids": ["doc"],
            "forbidden_document_ids": [],
            "remote_forbidden_document_ids": [],
            "request": {},
            "deterministic_retrieval_context": ["evidence"],
            "expected_retrieval_context": ["evidence"],
            "agent_trace": {"tools": [{"name": "news_evidence", "arguments": {}, "status": "error"}]},
            "expected_tools": [{"name": "news_evidence", "arguments": {}}],
            "forbidden_tools": [],
        }
        result = runner.deterministic_case(case)
        self.assertFalse(result["passed"])
        self.assertFalse(result["trace_gate"]["passed"])
        self.assertTrue(result["trace_gate"]["failed_tool_statuses"])

    def test_deterministic_case_requires_document_id_for_expected_evidence(self):
        case = {
            "id": "doc-aware",
            "privacy_reviewed": True,
            "private_evidence_present": False,
            "cloud_eval_allowed": True,
            "remote_export_allowed": True,
            "retrieved_document_ids": ["wrong-doc"],
            "remote_retrieved_document_ids": ["wrong-doc"],
            "required_document_ids": ["wrong-doc"],
            "forbidden_document_ids": [],
            "remote_forbidden_document_ids": [],
            "request": {},
            "deterministic_retrieval_context": ["same evidence"],
            "expected_retrieval_evidence": [{"document_id": "required-doc", "sentence": "same evidence"}],
            "agent_trace": {"tools": []},
            "expected_tools": [],
            "forbidden_tools": [],
        }
        result = runner.deterministic_case(case)
        self.assertFalse(result["retrieval_gate"]["passed"])
        self.assertEqual(result["retrieval_gate"]["missing_required_evidence"], [{"document_id": "required-doc", "sentence": "same evidence"}])
    def test_shell_values_take_precedence_over_local_env(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / ".env"
            config.write_text(
                "DEEPEVAL_JUDGE_BASE_URL=https://file.example/v1\n"
                "DEEPEVAL_JUDGE_API_KEY=file-key\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {
                "DEEPEVAL_JUDGE_BASE_URL": "https://shell.example/v1",
                "DEEPEVAL_JUDGE_API_KEY": "shell-key",
            }, clear=True):
                runner.load_local_judge_config(config)
                self.assertEqual(
                    runner.resolve_judge_config(),
                    ("shell-key", "https://shell.example/v1"),
                )

    def test_rejects_endpoint_credentials_and_query_parameters(self):
        for endpoint in (
            "https://user:pass@relay.example/v1",
            "https://relay.example/v1?api_key=secret",
        ):
            with self.subTest(endpoint=endpoint), patch.dict(os.environ, {
                "DEEPEVAL_JUDGE_API_KEY": "not-a-real-test-key",
                "DEEPEVAL_JUDGE_BASE_URL": endpoint,
            }, clear=True):
                with self.assertRaisesRegex(SystemExit, "must not contain"):
                    runner.resolve_judge_config()

    def test_rejects_missing_configuration_without_echoing_secrets(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(SystemExit, "missing DEEPEVAL_JUDGE_API_KEY"):
                runner.resolve_judge_config()

    def test_builds_deepseek_json_object_judge_with_configured_endpoint(self):
        with patch.dict(os.environ, {
            "DEEPEVAL_JUDGE_API_KEY": "deepseek-test-key",
            "DEEPEVAL_JUDGE_BASE_URL": "https://relay.example/v1",
        }, clear=True):
            judge = runner.build_judge_model()
            self.assertEqual(judge.name, "deepseek-flash")
            self.assertEqual(judge.base_url, "https://relay.example/v1")
            self.assertEqual(judge.temperature, 0.0)

    def test_private_replay_allows_local_hit_but_requires_remote_exclusion(self):
        case = {
            "id": "private-case",
            "privacy_reviewed": True,
            "private_evidence_present": True,
            "cloud_eval_allowed": False,
            "remote_export_allowed": False,
            "retrieved_document_ids": ["private-doc"],
            "remote_retrieved_document_ids": [],
            "remote_forbidden_document_ids": ["private-doc"],
            "required_document_ids": ["private-doc"],
            "forbidden_document_ids": ["private-doc"],
            "request": {"remote_safe_only": True},
            "deterministic_retrieval_context": ["private evidence sentence"],
            "expected_retrieval_context": ["private evidence sentence"],
            "agent_trace": {"tools": [{"name": "news_evidence", "arguments": {"query": "private"}}]},
            "expected_tools": [{"name": "news_evidence", "arguments": {"query": "private"}}],
            "forbidden_tools": [],
        }
        result = runner.deterministic_case(case)
        self.assertTrue(result["passed"])


if __name__ == "__main__":
    unittest.main()
