import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from app.schemas import CapitalEvidenceResult, LlmClientConfig
from app.services.llm_support import create_openai_client, parse_json_response, resolve_llm_config, safe_llm_error
from app.services.runtime_config import coalesce_bool, coalesce_float, coalesce_str, env_bool, env_float, env_int, redact_error
from app.services.sqlite_json_cache import SQLiteJsonCache
from app.services.stock_code import compact_date, market_prefix, normalize_stock_code, stock_digits


class SharedModuleTests(unittest.TestCase):
    def test_stock_code_normalization_handles_common_a_share_forms(self):
        self.assertEqual(stock_digits("SZ300750"), "300750")
        self.assertEqual(normalize_stock_code("300750"), "300750.SZ")
        self.assertEqual(normalize_stock_code("600000"), "600000.SH")
        self.assertEqual(normalize_stock_code("688795", market="SH"), "688795.SH")
        self.assertEqual(market_prefix("600000.SH"), "sh")
        self.assertEqual(compact_date("2026/06/12"), "20260612")

    def test_runtime_config_parses_env_and_coalesces_values(self):
        with patch.dict(os.environ, {"X_BOOL": "yes", "X_INT": "9", "X_FLOAT": "3.5"}):
            self.assertTrue(env_bool("X_BOOL"))
            self.assertEqual(env_int("X_INT", 1, minimum=3, maximum=8), 8)
            self.assertEqual(env_float("X_FLOAT", 1.0, minimum=4.0), 4.0)
        self.assertEqual(coalesce_str(None, "", "ok"), "ok")
        self.assertEqual(coalesce_float(None, "2.5", 1.0), 2.5)
        self.assertTrue(coalesce_bool(None, "on", False))

    def test_runtime_config_redacts_remote_disconnected_errors(self):
        redacted = redact_error("('Connection aborted.', RemoteDisconnected('Remote end closed connection'))")
        self.assertNotIn("RemoteDisconnected", redacted)
        self.assertNotIn("Connection aborted", redacted)
        self.assertTrue(redacted)

    def test_llm_support_parses_json_and_redacts_errors(self):
        self.assertEqual(parse_json_response("prefix {\"ok\": true} suffix"), {"ok": True})
        with patch.dict(os.environ, {"OPENAI_API_KEY": "sk-testsecret123"}):
            self.assertNotIn("sk-testsecret123", safe_llm_error(RuntimeError("bad sk-testsecret123")))
        config = resolve_llm_config(LlmClientConfig(api_key="k", model="m", json_mode=False))
        self.assertEqual(config.api_key, "k")
        self.assertEqual(config.model, "m")
        self.assertFalse(config.json_mode)

    def test_llm_local_base_url_bypasses_proxy_environment(self):
        config = resolve_llm_config(
            LlmClientConfig(api_key="k", base_url="http://127.0.0.1:11434/v1", model="m")
        )
        with patch("openai.OpenAI") as openai_client:
            create_openai_client(config)

        kwargs = openai_client.call_args.kwargs
        self.assertEqual(kwargs["base_url"], "http://127.0.0.1:11434/v1")
        self.assertIn("http_client", kwargs)

    def test_sqlite_json_cache_loads_exact_and_latest_payloads(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = SQLiteJsonCache(
                Path(tmp) / "cache.sqlite",
                "capital_evidence_cache",
                ("stock_code", "as_of_trade_date"),
                order_columns=("as_of_trade_date", "generated_at"),
            )
            first = CapitalEvidenceResult(stock_code="300750.SZ", generated_at="2026-06-11T15:01:00")
            second = CapitalEvidenceResult(stock_code="300750.SZ", generated_at="2026-06-12T15:01:00")
            cache.store({"stock_code": "300750.SZ", "as_of_trade_date": "2026-06-11"}, first.generated_at, first)
            cache.store({"stock_code": "300750.SZ", "as_of_trade_date": "2026-06-12"}, second.generated_at, second)

            exact = cache.load(
                CapitalEvidenceResult,
                {"stock_code": "300750.SZ", "as_of_trade_date": "2026-06-11"},
            )
            latest = cache.load_latest(
                CapitalEvidenceResult,
                {"stock_code": "300750.SZ"},
                exclude={"as_of_trade_date": "2026-06-12"},
            )

        self.assertEqual(exact.generated_at, first.generated_at)
        self.assertEqual(latest.generated_at, first.generated_at)


if __name__ == "__main__":
    unittest.main()
