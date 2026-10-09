import json
import unittest
from pathlib import Path
import promote_replay

ROOT = Path(__file__).resolve().parent

class PromotionTests(unittest.TestCase):
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