# GP Assistant DeepEval harness

The harness has three separate evaluation constructs: retrieval evidence, generation quality, and Agent trajectory. Deterministic replay is the PR hard gate; it does not claim product-model answer quality.

This directory contains an evaluation-only harness. It is not imported by the
Tauri or Android runtime.

## Input contract

The runner only accepts the SHA-256-approved `frozen_public_results.jsonl` listed in
`frozen_manifest.json`; arbitrary or user-provided result files are rejected before
any judge call. The frozen payload contains one JSON object per case:

```json
{
  "id": "case-id",
  "question": "...",
  "actual_output": "...",
  "expected_output": "...",
  "retrieval_context": ["evidence text ..."],
  "expected_retrieval_context": ["required evidence text ..."],
  "retrieved_document_ids": ["doc-1"],
  "required_document_ids": ["doc-1"],
  "forbidden_document_ids": [],
  "agent_trace": {"tools": [{"name": "news_evidence", "arguments": {"query": "..."}}]},
  "expected_tools": [{"name": "news_evidence", "arguments": {"query": "..."}}],
  "forbidden_tools": []
}
```

The adapter that produces this file must run the application against the
frozen corpus. This harness never reads the live research SQLite database and
never invents a system answer from the fixture alone.

The deterministic golden fixture now contains 40 synthetic component cases in
`app/prompts/research_retrieval_eval_cases.json` (`research-retrieval-eval-v4`),
with eight balanced categories: fact queries, multi-evidence synthesis,
temporal validity, source-tier conflict, community-only evidence, no-evidence
refusal, private-evidence handling, and Agent trace contracts. The replay adapter now
produces actual outputs and traces for all 40 cases. `frozen_public_results.jsonl`
contains the 35 public cases approved for cloud judging. The five private-evidence
cases remain in the local replay output only and must never be sent to a remote
judge.

## Local relay configuration

1. Copy `.env.example` to `.env` in this directory.
2. Fill `DEEPEVAL_JUDGE_BASE_URL` with the relay's OpenAI-compatible API base URL (usually ending in `/v1`).
3. Fill `DEEPEVAL_JUDGE_API_KEY` with the relay key. Keep `.env` local; it is ignored by Git.
4. Run the `report` command below. The runner uses DeepEval's built-in `DeepSeekModel` with fixed `deepseek-flash` and temperature `0`, overrides only the configured base URL, and never writes the API key into reports. Structured metrics use Chat Completions JSON mode (`json_object`).

## Modes

```powershell
# deterministic preflight only; no network or judge credentials
python runner.py --mode deterministic --track deterministic

# exploratory DeepEval report
# Credentials are loaded from evals/deepeval/.env
python runner.py --mode report --track full --repetitions 3 --output deepeval-report.json

# strict gate; fails closed if judge configuration or deterministic evidence is missing
python runner.py --mode gate --track full --repetitions 3 --output deepeval-report.json
```

The judge model, temperature (`0`), metric thresholds, prompt version, and
frozen dataset version are recorded in every report. The relay must expose the
OpenAI Chat Completions-compatible interface used by DeepEval's `OpenAIModel`;
a Codex Responses-only route is not sufficient. Metrics marked `flaky` in
`judge_policy.json` are reported but cannot fail the gate. DeepEval requires one
non-flaky evaluation anchor internally; `acceptable_answer` is used only as that
engine anchor while `policy_flaky` remains true and release gating stays disabled. Strict mode is only
enabled for metrics explicitly listed in `strict_metrics` after human
calibration.

## Model-backed replay

The Rust replay binary supports `--profile deterministic` and `--profile model-backed`.
Model-backed replay requires the independent `RAG_EVAL_AGENT_*` variables from `.env.example`; it never falls back to `DEEPEVAL_JUDGE_*`. Private cases remain local and are rejected by `promote_replay.py` before any judge call. Generated judge input is accepted only at the fixed `generated_model_results.jsonl` path with its matching `generated_model_manifest.json`.

`calibration_report.json` is intentionally pending until two human annotators provide at least 50 labels per candidate metric and Cohen's kappa is at least 0.80. Until then `strict_metrics` remains empty.