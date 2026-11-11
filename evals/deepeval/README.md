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

Temporal requests accept `as_of` as an ISO 8601 date or timestamp. A date covers
the entire UTC calendar day; an explicit timestamp preserves fractional seconds
while normalizing its timezone to UTC. Unknown publication dates and evidence
after the cutoff are excluded before candidate limits. The replay carries
`deterministic_retrieval_citations` from its declared local or requested assertion
projection so each required sentence is checked against its own document.

The deterministic golden fixture now contains 40 synthetic component cases in
`app/prompts/research_retrieval_eval_cases.json` (`research-retrieval-eval-v5`),
with eight balanced categories: fact queries, multi-evidence synthesis,
temporal validity, source-tier conflict, community-only evidence, no-evidence
refusal, private-evidence handling, and Agent trace contracts. The replay adapter now
produces actual outputs and traces for all 40 cases. `frozen_public_results.jsonl`
contains the 35 public cases approved for cloud judging. The five private-evidence
cases remain in the local replay output only and must never be sent to a remote
judge.

## Candidate audit (observational, not a gate)

The deterministic report includes `retrieval_audit` for each case. It observes
only the citations returned by `replay.response`, not the pre-Top-K candidate
pool. Fixture `retrieval_relevance_contract` annotations are copied to replay
output **after** application execution; they never become runtime inputs.

- Low-information and irrelevant hits are reported only when explicitly labeled
  by the synthetic fixture. Unannotated short text is not automatically noise.
- Relevant/irrelevant citation ranks expose observed ordering. The ordering field
  is `null` when no annotated pair is available, rather than an assumed pass.
- Same-document extra citations and whitespace-normalized exact duplicate excerpt
  groups are separate observations. Neither proves semantic redundancy or that
  a full document is redundant; excerpts may be truncated.
- Every audit has `affects_gate: false`. These fields do not suppress candidates,
  change RRF/Top-K/chunking, or enable strict LLM metrics. Missing citation data
  is reported as `unavailable`, not as a successful empty retrieval.

The v5 snapshot exposes two observations, not tuning recommendations:
`hybrid_rrf_shared_candidate` returns the annotated low-information document at
rank 2, between relevant documents at ranks 1 and 3. Its desired relevance order
is therefore **not** satisfied even though the existing retrieval/trajectory
hard gates pass. `top_k_hard_cap` returns eight identical excerpts from eight
synthetic documents (seven extra duplicate excerpts), as expected for that
capacity fixture; this does not establish production redundancy.

To regenerate the public snapshot and its manifest from a real offline replay:

```powershell
cargo run --manifest-path desktop/src-tauri/Cargo.toml --features eval-replay --bin rag_replay -- app/prompts/research_retrieval_eval_cases.json evals/deepeval/tmp/replay-v5.jsonl --profile deterministic
python evals/deepeval/promote_replay.py --input evals/deepeval/tmp/replay-v5.jsonl --output evals/deepeval/frozen_public_results.jsonl --manifest evals/deepeval/frozen_manifest.json
python evals/deepeval/runner.py --mode deterministic --track deterministic --output evals/deepeval/tmp/v5-deterministic-report.json
```

Promotion must pass before replacing frozen artifacts. Five private cases stay
local. Retrieval assertions explicitly select the requested or local response,
while remote privacy checks always inspect the requested remote-safe response.
The `remote_export_permission` case can retain private evidence in its extra local
query without treating that evidence as a remote hit. Negative regression cases
still fail when required remote evidence is available only locally or private
evidence leaks into a remote response.

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
