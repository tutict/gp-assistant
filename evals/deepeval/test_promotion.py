import json
import unittest
import hashlib
import tempfile
from pathlib import Path
import promote_replay

ROOT = Path(__file__).resolve().parent

class PromotionTests(unittest.TestCase):
    def test_promotes_35_public_cases_and_excludes_private_payloads(self):
        rows = promote_replay.read_rows(ROOT / "frozen_public_results.jsonl")
        fixture = json.loads(promote_replay.FIXTURE_PATH.read_text(encoding="utf-8-sig"))
        for case in fixture["cases"]:
            if case["cloud_eval_allowed"] is not True:
                rows.append({"id": case["id"], "dataset_version": promote_replay.DATASET_VERSION,
                             "cloud_eval_allowed": False, "private_text": "PRIVATE_SENTINEL"})
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "public.jsonl"
            manifest = Path(directory) / "manifest.json"
            result = promote_replay.promote(rows, output, manifest, False)
            self.assertEqual(result["case_count"], 35)
            self.assertEqual(result["excluded_private_case_count"], 5)
            self.assertEqual(len(promote_replay.read_rows(output)), 35)
            self.assertNotIn("PRIVATE_SENTINEL", output.read_text(encoding="utf-8"))
            self.assertEqual(result["sha256"], hashlib.sha256(output.read_bytes()).hexdigest())
            self.assertNotIn(b"\r\n", output.read_bytes())
            self.assertNotIn(b"\r\n", manifest.read_bytes())
            self.assertEqual(json.loads(manifest.read_text())[promote_replay.DATASET_VERSION]["sha256"], result["sha256"])

    def test_failed_hard_gate_does_not_overwrite_frozen_artifacts(self):
        for gate in ("retrieval_gate", "trace_gate"):
            with self.subTest(gate=gate), tempfile.TemporaryDirectory() as directory:
                rows = promote_replay.read_rows(ROOT / "frozen_public_results.jsonl")
                rows[0][gate]["passed"] = False
                output = Path(directory) / "public.jsonl"
                manifest = Path(directory) / "manifest.json"
                output.write_text("existing-public", encoding="utf-8")
                manifest.write_text("existing-manifest", encoding="utf-8")
                with self.assertRaisesRegex(SystemExit, "failed deterministic"):
                    promote_replay.promote(rows, output, manifest, False)
                self.assertEqual(output.read_text(), "existing-public")
                self.assertEqual(manifest.read_text(), "existing-manifest")

    def test_deterministic_replay_is_not_model_backed(self):
        rows = promote_replay.read_rows(ROOT / "frozen_public_results.jsonl")
        with self.assertRaisesRegex(SystemExit, "non-generative public case"):
            promote_replay.validate_rows(rows, True)

    def test_rejects_duplicate_case_ids(self):
        rows = [json.loads(line) for line in (ROOT / "frozen_public_results.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
        rows.append(dict(rows[0]))
        with self.assertRaisesRegex(SystemExit, "duplicate case IDs"):
            promote_replay.validate_rows(rows, False)

    def test_rejects_public_private_flags(self):
        rows = [json.loads(line) for line in (ROOT / "frozen_public_results.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
        rows[0]["private_evidence_present"] = True
        with self.assertRaisesRegex(SystemExit, "unsafe privacy flags"):
            promote_replay.validate_rows(rows, False)

    def test_rejects_missing_canonical_public_case(self):
        rows = [json.loads(line) for line in (ROOT / "frozen_public_results.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
        rows.pop()
        with self.assertRaisesRegex(SystemExit, "public case IDs"):
            promote_replay.validate_rows(rows, False)

if __name__ == "__main__":
    unittest.main()
