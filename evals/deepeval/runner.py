from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
from urllib.parse import urlsplit
from typing import Any

ROOT = Path(__file__).resolve().parent
POLICY_PATH = ROOT / "judge_policy.json"
MANIFEST_PATH = ROOT / "frozen_manifest.json"
GENERATED_RESULTS_PATH = ROOT / "generated_model_results.jsonl"
GENERATED_MANIFEST_PATH = ROOT / "generated_model_manifest.json"
FIXED_JUDGE_MODEL = "deepseek-flash"
TRACK_METRICS = {
    "deterministic": [],
    "retrieval": ["contextual_precision", "contextual_recall", "contextual_relevancy"],
    "generation": ["faithfulness", "answer_relevancy", "acceptable_answer"],
    "full": ["contextual_precision", "contextual_recall", "contextual_relevancy", "faithfulness", "answer_relevancy", "acceptable_answer"],
}


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def approved_results_path(path: Path, dataset_version: str) -> Path:
    resolved = path.resolve()
    if ROOT not in resolved.parents:
        raise SystemExit(f"results must stay under {ROOT}")
    if resolved == (ROOT / "frozen_public_results.jsonl").resolve():
        manifest_path = MANIFEST_PATH
    elif resolved == GENERATED_RESULTS_PATH.resolve():
        manifest_path = GENERATED_MANIFEST_PATH
    else:
        raise SystemExit("results path is not an approved fixed evaluation payload")
    manifest = read_json(manifest_path).get(dataset_version, {})
    expected_path = (ROOT / manifest.get("path", "")).resolve()
    if resolved != expected_path or sha256(resolved) != manifest.get("sha256"):
        raise SystemExit("results file is not an approved immutable frozen evaluation payload")
    return resolved


def load_cases(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip()]


def normalize_tool(tool: dict[str, Any]) -> dict[str, Any]:
    return {
        "name": str(tool.get("name", "")),
        "arguments": tool.get("arguments", {}) or {},
        "status": str(tool.get("status", "ok")),
    }


def metric_names_for_track(track: str) -> list[str]:
    try:
        return list(TRACK_METRICS[track])
    except KeyError as error:
        raise SystemExit(f"unknown evaluation track: {track}") from error


def _validated_endpoint(value: str, variable: str) -> str:
    parsed = urlsplit(value)
    if parsed.scheme not in {"https", "http"} or not parsed.hostname:
        raise SystemExit(f"{variable} must be an absolute http(s) URL")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise SystemExit(f"{variable} must not contain credentials, query, or fragment")
    return value.rstrip("/")


def resolve_agent_config() -> dict[str, Any]:
    base_url = (os.environ.get("RAG_EVAL_AGENT_BASE_URL") or "").strip()
    api_key = (os.environ.get("RAG_EVAL_AGENT_API_KEY") or "").strip()
    model = (os.environ.get("RAG_EVAL_AGENT_MODEL") or "").strip()
    api_format = (os.environ.get("RAG_EVAL_AGENT_API_FORMAT") or "openai_chat").strip()
    prompt_version = (os.environ.get("RAG_EVAL_AGENT_PROMPT_VERSION") or "").strip()
    temperature_text = (os.environ.get("RAG_EVAL_AGENT_TEMPERATURE") or "0").strip()
    if not base_url:
        raise SystemExit("missing RAG_EVAL_AGENT_BASE_URL; model-backed replay will not use judge configuration")
    if not api_key:
        raise SystemExit("missing RAG_EVAL_AGENT_API_KEY")
    if api_key.lower().startswith(("paste-", "replace-", "your-")):
        raise SystemExit("RAG_EVAL_AGENT_API_KEY is still a placeholder")
    if not model:
        raise SystemExit("missing RAG_EVAL_AGENT_MODEL")
    if not prompt_version:
        raise SystemExit("missing RAG_EVAL_AGENT_PROMPT_VERSION")
    try:
        temperature = float(temperature_text)
    except ValueError as error:
        raise SystemExit("RAG_EVAL_AGENT_TEMPERATURE must be numeric") from error
    if temperature != 0:
        raise SystemExit("RAG_EVAL_AGENT_TEMPERATURE must be 0 for reproducible replay")
    return {
        "base_url": _validated_endpoint(base_url, "RAG_EVAL_AGENT_BASE_URL"),
        "api_key": api_key,
        "model": model,
        "api_format": api_format,
        "temperature": temperature,
        "prompt_version": prompt_version,
    }


def agent_config_fingerprint(config: dict[str, Any]) -> str:
    public = {key: config[key] for key in ("base_url", "model", "api_format", "temperature", "prompt_version")}
    return hashlib.sha256(json.dumps(public, sort_keys=True).encode("utf-8")).hexdigest()


def deterministic_case(case: dict[str, Any]) -> dict[str, Any]:
    required = set(case.get("required_document_ids") or [])
    forbidden = set(case.get("forbidden_document_ids") or [])
    retrieved = set(case.get("retrieved_document_ids") or [])
    actual_tools = [normalize_tool(item) for item in ((case.get("agent_trace") or {}).get("tools") or [])]
    expected_tools = [normalize_tool(item) for item in (case.get("expected_tools") or [])]
    retrieved_context = [str(item) for item in case.get("retrieval_context", [])]
    evidence_context = [str(item) for item in (case.get("deterministic_retrieval_context") or case.get("retrieval_context") or [])]
    expected_evidence = case.get("expected_retrieval_evidence") or []
    if expected_evidence:
        missing_evidence = [
            item for item in expected_evidence
            if item.get("document_id") not in retrieved
            or not any(str(item.get("sentence", "")) in context for context in evidence_context)
        ]
    else:
        missing_evidence = [
            str(item) for item in case.get("expected_retrieval_context", [])
            if not any(str(item) in context for context in evidence_context)
        ]
    forbidden_tool_names = set(case.get("forbidden_tools") or [])
    forbidden_tool_hits = sorted({item["name"] for item in actual_tools} & forbidden_tool_names)
    private_ids = set(case.get("remote_forbidden_document_ids") or [])
    if "remote_retrieved_document_ids" in case:
        remote_ids = set(case.get("remote_retrieved_document_ids") or [])
    else:
        remote_ids = set(case.get("retrieved_document_ids") or [])
    remote_projection = bool(case.get("request", {}).get("remote_safe_only")) or not case.get("cloud_eval_allowed", True)
    forbidden_projection = remote_ids if remote_projection else retrieved
    forbidden_retrieved = forbidden & forbidden_projection
    privacy_ok = (
        case.get("private_evidence_present") is False
        and case.get("remote_export_allowed") is True
        if case.get("cloud_eval_allowed", True)
        else case.get("private_evidence_present") is True and private_ids.isdisjoint(remote_ids)
    )
    retrieval_passed = (
        case.get("privacy_reviewed") is True
        and privacy_ok
        and required <= retrieved
        and not forbidden_retrieved
        and not missing_evidence
    )
    failed_tool_statuses = [item for item in actual_tools if item["status"] not in {"ok", "degraded"}]
    actual_tool_shapes = [{"name": item["name"], "arguments": item["arguments"]} for item in actual_tools]
    expected_tool_shapes = [{"name": item["name"], "arguments": item["arguments"]} for item in expected_tools]
    tool_sequence_match = actual_tool_shapes == expected_tool_shapes
    trajectory_passed = not forbidden_tool_hits and not failed_tool_statuses and tool_sequence_match
    generation_eligible = bool(case.get("generation_eval_eligible")) and bool(
        case.get("replay", {}).get("generative_model_used")
    )
    actual_output = str(case.get("agent_model_output") or case.get("agent_actual_output", case.get("actual_output", "")))
    answer_facts = case.get("answer_facts") or []
    required_citations = case.get("required_citations") or []
    forbidden_claims = case.get("forbidden_claims") or case.get("answer_contract", {}).get("forbidden_claims", []) or []
    missing_answer_facts = [fact.get("fact_id", fact.get("claim", "")) for fact in answer_facts if fact.get("claim") and fact["claim"] not in actual_output]
    missing_required_citations = [citation for citation in required_citations if citation.get("document_id") not in retrieved or not any(marker in actual_output for marker in ("[C", "[E"))]
    forbidden_claim_hits = [claim for claim in forbidden_claims if claim and claim in actual_output]
    generation_passed = not missing_answer_facts and not missing_required_citations and not forbidden_claim_hits
    constructs = {
        "retrieval": {
            "passed": retrieval_passed,
            "missing_required_document_ids": sorted(required - retrieved),
            "forbidden_document_ids_retrieved": sorted(forbidden_retrieved),
            "missing_required_evidence": missing_evidence,
            "privacy_ok": privacy_ok,
        },
        "trajectory": {
            "passed": trajectory_passed,
            "forbidden_tools_called": forbidden_tool_hits,
            "tool_sequence_match": tool_sequence_match,
            "failed_tool_statuses": failed_tool_statuses,
        },
        "generation": {
            "status": "eligible" if generation_eligible else "not_applicable",
            "passed": generation_passed if generation_eligible else True,
            "missing_answer_facts": missing_answer_facts if generation_eligible else [],
            "missing_required_citations": missing_required_citations if generation_eligible else [],
            "forbidden_claim_hits": forbidden_claim_hits if generation_eligible else [],
            "reason": None if generation_eligible else "deterministic replay has no product-model answer",
        },
    }
    return {
        "case_id": case.get("id"),
        "passed": retrieval_passed and trajectory_passed and constructs["generation"]["passed"],
        "retrieval_gate": constructs["retrieval"],
        "trace_gate": constructs["trajectory"],
        "constructs": constructs,
        "missing_required_document_ids": sorted(required - retrieved),
        "forbidden_document_ids_retrieved": sorted(forbidden_retrieved),
        "missing_required_evidence": missing_evidence,
        "forbidden_tools_called": forbidden_tool_hits,
        "failed_tool_statuses": failed_tool_statuses,
        "tool_sequence_match": tool_sequence_match,
    }


def make_test_cases(cases: list[dict[str, Any]]) -> list[Any]:
    from deepeval.test_case import LLMTestCase, ToolCall
    output = []
    for case in cases:
        tools = [ToolCall(name=item["name"], input_parameters=item["arguments"]) for item in ((case.get("agent_trace") or {}).get("tools") or [])]
        expected_tools = [ToolCall(name=item["name"], input_parameters=item.get("arguments", {})) for item in case.get("expected_tools", [])]
        actual_output = case.get("agent_model_output") or case.get("agent_actual_output", case.get("actual_output", ""))
        output.append(LLMTestCase(name=str(case["id"]), input=str(case.get("question", "")), actual_output=str(actual_output), expected_output=str(case.get("expected_output", "")), context=[str(x) for x in case.get("context", [])], retrieval_context=[str(x) for x in case.get("retrieval_context", [])], tools_called=tools, expected_tools=expected_tools))
    return output


def build_judge_model() -> Any:
    """Build DeepEval's official DeepSeek adapter with a configurable OpenAI-compatible endpoint.

    DeepEval's DeepSeekModel uses Chat Completions JSON mode (`json_object`) for
    schema-backed metrics. We override only its base URL after construction so
    the same adapter works with the official API or an explicitly configured
    OpenAI-compatible relay.
    """
    from deepeval.models.llms.deepseek_model import DeepSeekModel

    api_key, base_url = resolve_judge_config()
    judge = DeepSeekModel(
        model=FIXED_JUDGE_MODEL,
        api_key=api_key,
        temperature=0,
    )
    judge.base_url = base_url
    return judge


def make_metrics(policy: dict[str, Any], judge: Any, track: str = "full") -> list[Any]:
    from deepeval.metrics import AnswerRelevancyMetric, ContextualPrecisionMetric, ContextualRecallMetric, ContextualRelevancyMetric, FaithfulnessMetric, GEval
    from deepeval.test_case import SingleTurnParams
    t = policy["thresholds"]
    strict = set(policy.get("strict_metrics", []))
    flaky = set(policy.get("flaky_metrics", []))
    anchor = policy.get("deepeval_evaluation_anchor_metric", "acceptable_answer")
    common = {"model": judge, "async_mode": False}

    def is_engine_flaky(name: str) -> bool:
        # DeepEval refuses to execute when every metric is marked flaky. Keep
        # one non-gating anchor only to satisfy its runner; policy_flaky below
        # remains the source of truth for release decisions.
        return name in flaky and name != anchor

    metrics = [
        ContextualPrecisionMetric(threshold=t["contextual_precision"], strict_mode="contextual_precision" in strict, flaky=is_engine_flaky("contextual_precision"), **common),
        ContextualRecallMetric(threshold=t["contextual_recall"], strict_mode="contextual_recall" in strict, flaky=is_engine_flaky("contextual_recall"), **common),
        ContextualRelevancyMetric(threshold=t["contextual_relevancy"], strict_mode="contextual_relevancy" in strict, flaky=is_engine_flaky("contextual_relevancy"), **common),
        FaithfulnessMetric(threshold=t["faithfulness"], strict_mode="faithfulness" in strict, flaky=is_engine_flaky("faithfulness"), **common),
        AnswerRelevancyMetric(threshold=t["answer_relevancy"], strict_mode="answer_relevancy" in strict, flaky=is_engine_flaky("answer_relevancy"), **common),
        GEval(name="acceptable_answer", evaluation_params=[SingleTurnParams.ACTUAL_OUTPUT, SingleTurnParams.EXPECTED_OUTPUT], criteria="The answer satisfies the expected answer without contradicting the evidence or adding unsupported factual claims.", threshold=t["acceptable_answer"], strict_mode="acceptable_answer" in strict, flaky=is_engine_flaky("acceptable_answer"), **common),
    ]
    names = metric_names_for_track(track)
    metric_index = {name: index for index, name in enumerate(TRACK_METRICS["full"])}
    return [metrics[metric_index[name]] for name in names]


def load_local_judge_config(path: Path = ROOT / ".env") -> None:
    """Load simple KEY=VALUE settings without overriding shell/CI configuration."""
    if not path.is_file():
        return
    for raw_line in path.read_text(encoding="utf-8-sig").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[7:].lstrip()
        key, separator, value = line.partition("=")
        key = key.strip()
        if not separator or not key or key in os.environ:
            continue
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"\"", "'"}:
            value = value[1:-1]
        os.environ[key] = value


def resolve_judge_config() -> tuple[str, str]:
    api_key = (os.environ.get("DEEPEVAL_JUDGE_API_KEY") or "").strip()
    base_url = (os.environ.get("DEEPEVAL_JUDGE_BASE_URL") or "").strip()
    if not api_key:
        raise SystemExit("missing DEEPEVAL_JUDGE_API_KEY; copy .env.example to .env and fill it")
    if not base_url:
        raise SystemExit("missing DEEPEVAL_JUDGE_BASE_URL; configure your OpenAI-compatible relay in .env")
    if api_key.lower().startswith(("paste-", "replace-", "your-")):
        raise SystemExit("DEEPEVAL_JUDGE_API_KEY is still a placeholder")
    parsed = urlsplit(base_url)
    if parsed.scheme not in {"https", "http"} or not parsed.hostname:
        raise SystemExit("DEEPEVAL_JUDGE_BASE_URL must be an absolute http(s) URL")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise SystemExit("DEEPEVAL_JUDGE_BASE_URL must not contain credentials, query, or fragment")
    return api_key, base_url.rstrip("/")


def run_judge(cases: list[dict[str, Any]], policy: dict[str, Any], repetitions: int, track: str) -> dict[str, Any]:
    from deepeval import evaluate
    from deepeval.evaluate.configs import AsyncConfig, CacheConfig, DisplayConfig

    if track == "deterministic":
        return {"status": "not_applicable", "reason": "deterministic track does not call an LLM judge", "runs": []}
    generation_cases = [
        case for case in cases
        if case.get("generation_eval_eligible") and case.get("replay", {}).get("generative_model_used")
    ]
    if track == "generation" and not generation_cases:
        return {"status": "not_applicable", "reason": "no model-backed product answers are present", "runs": []}
    judge = build_judge_model()
    groups: list[tuple[str, list[dict[str, Any]], str]] = []
    if track in {"retrieval", "full"}:
        groups.append(("retrieval", cases, "retrieval"))
    if track in {"generation", "full"} and generation_cases:
        groups.append(("generation", generation_cases, "generation"))
    runs = []
    for index in range(repetitions):
        merged: dict[str, dict[str, Any]] = {
            str(case["id"]): {"name": str(case["id"]), "success": True, "metrics": []}
            for case in cases
        }
        test_run_ids = []
        executed_tracks = []
        for label, group_cases, metric_track in groups:
            result = evaluate(
                test_cases=make_test_cases(group_cases),
                metrics=make_metrics(policy, judge, metric_track),
                identifier=f"gp-assistant-rag-{policy['dataset_version']}-{label}-run-{index + 1}",
                async_config=AsyncConfig(run_async=True, throttle_value=2.0, max_concurrent=1),
                display_config=DisplayConfig(show_indicator=False, print_results=False, inspect_after_run=False),
                cache_config=CacheConfig(write_cache=False, use_cache=False),
            )
            test_run_ids.append(result.test_run_id)
            executed_tracks.append(metric_track)
            for item in result.test_results:
                target = merged[item.name]
                target["success"] = target["success"] and bool(item.success)
                target["metrics"].extend({
                    "name": getattr(metric, "name", metric.__class__.__name__),
                    "score": getattr(metric, "score", None),
                    "success": getattr(metric, "success", None),
                    "reason": getattr(metric, "reason", None),
                    "strict_mode": getattr(metric, "strict_mode", False),
                    "engine_flaky": getattr(metric, "flaky", False),
                    "policy_flaky": getattr(metric, "name", metric.__class__.__name__) in set(policy.get("flaky_metrics", [])),
                } for metric in (item.metrics_data or []))
        runs.append({"repetition": index + 1, "test_run_ids": test_run_ids, "executed_tracks": executed_tracks, "cases": list(merged.values())})
    metric_counts: dict[str, list[bool]] = {}
    for run in runs:
        for case in run["cases"]:
            for metric in case["metrics"]:
                metric_counts.setdefault(metric["name"], []).append(bool(metric["success"]))
    metric_pass_rates = {name: (sum(values) / len(values) if values else 0.0) for name, values in metric_counts.items()}
    strict_names = set(policy.get("strict_metrics", []))
    strict_rates = [metric_pass_rates.get(name, 0.0) for name in metric_pass_rates if name.lower().replace(" ", "_") in strict_names]
    executed = sorted({track_name for run in runs for track_name in run["executed_tracks"]})
    return {
        "status": "completed",
        "requested_track": track,
        "executed_tracks": executed,
        "not_applicable_tracks": ["generation"] if "generation" not in executed else [],
        "judge_model": FIXED_JUDGE_MODEL,
        "judge_temperature": 0,
        "metric_pass_rates": metric_pass_rates,
        "aggregate_pass_rate": min(strict_rates) if strict_rates else None,
        "runs": runs,
    }

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", type=Path, default=ROOT / "frozen_public_results.jsonl")
    parser.add_argument("--mode", choices=["deterministic", "report", "gate"], default="deterministic")
    parser.add_argument("--track", choices=sorted(TRACK_METRICS), default="full")
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    load_local_judge_config()
    policy = read_json(POLICY_PATH)
    results_path = approved_results_path(args.results, policy["dataset_version"])
    cases = load_cases(results_path)
    if not cases or any(case.get("dataset_version") != policy["dataset_version"] for case in cases):
        raise SystemExit("approved frozen results do not match the policy dataset version")
    if results_path == GENERATED_RESULTS_PATH:
        if any(case.get("cloud_eval_allowed") is not True for case in cases):
            raise SystemExit("generated model payload contains a non-public case")
        if any(not case.get("generation_eval_eligible") or not case.get("replay", {}).get("generative_model_used") for case in cases):
            raise SystemExit("generated model payload contains a non-generative case")
    deterministic = [deterministic_case(case) for case in cases]
    report: dict[str, Any] = {"dataset_version": policy["dataset_version"], "track": args.track, "judge_model": FIXED_JUDGE_MODEL, "judge_temperature": 0, "results_sha256": sha256(results_path), "deterministic": deterministic, "deterministic_pass_rate": sum(item["passed"] for item in deterministic) / len(deterministic)}
    deterministic_ok = all(item["passed"] for item in deterministic)
    if args.mode in {"report", "gate"} and not deterministic_ok:
        report["deepeval"] = {"status": "blocked", "reason": "deterministic evidence, privacy, or trajectory assertions failed", "runs": []}
    elif args.mode in {"report", "gate"}:
        report["deepeval"] = run_judge(cases, policy, max(1, args.repetitions), args.track)
    if args.mode == "gate":
        if not deterministic_ok:
            report["gate"] = {"passed": False, "reason": "deterministic evidence, privacy, or trace assertions failed"}
        elif not policy.get("strict_metrics"):
            report["gate"] = {"passed": False, "reason": "no calibrated strict metrics configured"}
        else:
            aggregate = report["deepeval"].get("aggregate_pass_rate")
            minimum = float(policy.get("minimum_aggregate_pass_rate", 0.8))
            report["gate"] = {
                "passed": aggregate is not None and aggregate >= minimum,
                "aggregate_pass_rate": aggregate,
                "minimum_aggregate_pass_rate": minimum,
            }
    if args.output:
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if args.mode == "deterministic" or (args.mode == "report" and deterministic_ok) or report.get("gate", {}).get("passed") else 1


if __name__ == "__main__":
    raise SystemExit(main())
