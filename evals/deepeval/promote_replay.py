from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

DATASET_VERSION = "research-retrieval-eval-v4"
ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = ROOT / "app" / "prompts" / "research_retrieval_eval_cases.json"


def read_rows(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip()]


def canonical_case_ids() -> tuple[set[str], dict[str, set[str]]]:
    fixture = json.loads(FIXTURE_PATH.read_text(encoding="utf-8-sig"))
    if fixture.get("version") != DATASET_VERSION:
        raise SystemExit("canonical fixture version mismatch")
    cases = fixture.get("cases") or []
    public: set[str] = set()
    documents_by_case: dict[str, set[str]] = {}
    for case in cases:
        case_id = str(case["id"])
        documents = {str(document["document_id"]) for document in case.get("documents", [])}
        generated = case.get("generated_documents") or {}
        prefix = generated.get("document_id_prefix", "generated-")
        for index in range(int(generated.get("count", 0))):
            documents.add(f"{prefix}{index}")
        documents_by_case[case_id] = documents
        if case.get("cloud_eval_allowed") is True:
            public.add(case_id)
    return public, documents_by_case


def validate_rows(rows: list[dict], require_model: bool) -> tuple[list[dict], int]:
    if not rows:
        raise SystemExit("replay payload is empty")
    versions = {row.get("dataset_version") for row in rows}
    if versions != {DATASET_VERSION}:
        raise SystemExit(f"unexpected replay dataset versions: {sorted(versions)}")
    expected_public, documents_by_case = canonical_case_ids()
    actual_ids = [str(row.get("id", "")) for row in rows]
    if len(actual_ids) != len(set(actual_ids)):
        raise SystemExit("replay payload contains duplicate case IDs")
    actual_public = {case_id for case_id in actual_ids if any(row.get("id") == case_id and row.get("cloud_eval_allowed") is True for row in rows)}
    if actual_public != expected_public:
        raise SystemExit(f"replay public case IDs do not match canonical fixture: expected {len(expected_public)}, got {len(actual_public)}")
    public = []
    for row in rows:
        if row.get("cloud_eval_allowed") is not True:
            continue
        if row.get("privacy_reviewed") is not True:
            raise SystemExit(f"public case {row.get('id')} is not privacy reviewed")
        if row.get("private_evidence_present") is not False or row.get("remote_export_allowed") is not True:
            raise SystemExit(f"public case {row.get('id')} has unsafe privacy flags")
        if row.get("retrieval_gate", {}).get("passed") is not True or row.get("trace_gate", {}).get("passed") is not True:
            raise SystemExit(f"public case {row.get('id')} failed deterministic retrieval/trajectory gates")
        remote_ids = set(row.get("remote_retrieved_document_ids") or [])
        if not remote_ids.issubset(documents_by_case.get(str(row.get("id")), set())):
            raise SystemExit(f"public case {row.get('id')} contains document IDs outside canonical fixture")
        forbidden_ids = set(row.get("remote_forbidden_document_ids") or [])
        if remote_ids & forbidden_ids:
            raise SystemExit(f"public case {row.get('id')} contains a remote private-document hit")
        if require_model and (not row.get("generation_eval_eligible") or not row.get("replay", {}).get("generative_model_used")):
            raise SystemExit(f"model-backed payload contains a non-generative public case: {row.get('id')}")
        public.append(row)
    if len(public) != len(expected_public):
        raise SystemExit(f"expected {len(expected_public)} public cases, got {len(public)}")
    return public, len(rows) - len(public)


def promote(rows: list[dict], output: Path, manifest_path: Path, require_model: bool) -> dict:
    public, excluded_private = validate_rows(rows, require_model)
    output.write_text("".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in public), encoding="utf-8")
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    manifest = {DATASET_VERSION: {"path": output.name, "sha256": digest, "case_count": len(public), "excluded_private_case_count": excluded_private}}
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return {"case_count": len(public), "excluded_private_case_count": excluded_private, "sha256": digest}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--require-model", action="store_true")
    args = parser.parse_args()
    print(json.dumps(promote(read_rows(args.input), args.output, args.manifest, args.require_model), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
