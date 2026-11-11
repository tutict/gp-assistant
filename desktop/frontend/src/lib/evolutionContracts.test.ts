import { describe, expect, it } from "vitest";
import { parseEvolutionProfile, parseEvolutionReview, parseSentimentStrategies, parseSentimentStrategyVersions } from "./evolutionContracts";
describe("evolution API contracts", () => {
  it("preserves a review of a run that produced no answer", () => {
    const review = { review_id: "review-empty", run_id: "failed-run", conversation_id: null, status: "draft", review_mode: "deterministic", research_goal: "Review cancelled research", evidence_and_process: { evidence_ids: [], tool_calls: [] }, conclusion_quality: { answer: "", has_answer: false }, blind_spot_candidates: [], rule_candidates: [], model_feedback: null };
    expect(parseEvolutionReview(review).conclusion_quality).toEqual({ answer: "", has_answer: false });
    expect(() => parseEvolutionReview({ ...review, conclusion_quality: { answer: 42, has_answer: false } })).toThrow(/conclusion answer/i);
  });
  it("parses a local profile and rejects unknown rule states", () => {
    const valid = { settings: { enabled: true, coach_mode: true, local_only: true, review_trigger: "explicit", retain_raw_conversation: false, profile_version: 3 }, profile_version: 3, rules: [{ rule_id: "r1", kind: "invalidation", statement: "write invalidation", source_review_id: "review-1", evidence_refs: ["doc-1"], status: "active", version: 3, confirmed_at: 100 }], suppressed_kinds: ["invalidation"] };
    expect(parseEvolutionProfile(valid).rules[0].status).toBe("active");
    expect(() => parseEvolutionProfile({ ...valid, rules: [{ ...valid.rules[0], status: "maybe" }] })).toThrow(/rule status/i);
  });
  it("preserves evidence-free review candidates without treating them as confirmed rules", () => {
    const valid = { review_id: "review-1", run_id: "run-1", conversation_id: null, status: "draft", review_mode: "deterministic", research_goal: "question", evidence_and_process: { evidence_ids: ["doc-1"], tool_calls: [] }, conclusion_quality: { answer: "answer", has_answer: true }, blind_spot_candidates: [{ kind: "invalidation", statement: "Add expiry condition", evidence_refs: ["doc-1"] }, { kind: "evidence_coverage", statement: "No evidence was available", evidence_refs: [] }], rule_candidates: [{ rule_id: "rule-1", kind: "invalidation", statement: "Always state invalidation conditions", evidence_refs: ["doc-1"] }], model_feedback: "Keep the conclusion tied to the cited evidence." };
    const parsed = parseEvolutionReview(valid);
    expect(parsed.blind_spot_candidates[0].evidence_refs).toEqual(["doc-1"]);
    expect(parsed.blind_spot_candidates[1].evidence_refs).toEqual([]);
    expect(parsed.rule_candidates[0].rule_id).toBe("rule-1");
    expect(parsed.model_feedback).toContain("cited evidence");
  });
  it("parses strategy states and rejects arbitrary strings", () => {
    const strategy = { strategy_id: "s1", name: "cooldown", stage: "降温", payload: { criteria: { max_pe: 20 } }, status: "disabled", version: 2, created_at: 100, source_analysis_id: null };
    expect(parseSentimentStrategies({ items: [strategy] }).items[0].status).toBe("disabled");
    expect(() => parseSentimentStrategies({ items: [{ ...strategy, status: "maybe" }] })).toThrow(/strategy status/i);
  });
  it("parses version history without accepting malformed payloads", () => {
    const version = { version: 1, payload: { criteria: { max_pe: 20 } }, status: "superseded", created_at: 100 };
    expect(parseSentimentStrategyVersions({ strategy_id: "s1", items: [version] }).items[0].status).toBe("superseded");
    expect(() => parseSentimentStrategyVersions({ strategy_id: "s1", items: [{ ...version, version: "one" }] })).toThrow(/version/i);
  });
});
