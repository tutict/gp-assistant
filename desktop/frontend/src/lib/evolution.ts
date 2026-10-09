import { getJson, postJson } from "./tauri";
import type { AgentResult } from "../types";


function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

export function extractEvolutionEvidenceIds(result: AgentResult): string[] {
  const ids = new Set<string>();
  const visit = (value: unknown, evidenceContext = false, depth = 0) => {
    if (depth > 6 || value == null) return;
    if (Array.isArray(value)) { value.forEach((item) => visit(item, evidenceContext, depth + 1)); return; }
    if (typeof value !== "object") return;
    const record = asRecord(value);
    const isEvidenceRecord = evidenceContext || ["citations", "evidence", "evidence_summary", "evidence_ids"].some((key) => key in record);
    for (const key of ["document_id", "evidence_id", "citation_id"]) {
      const id = record[key];
      if (isEvidenceRecord && typeof id === "string" && id.trim()) ids.add(id.trim());
    }
    for (const key of ["citations", "evidence", "evidence_summary", "evidence_ids", "news", "news_rag", "data"]) {
      if (key in record) visit(record[key], ["citations", "evidence", "evidence_summary", "evidence_ids"].includes(key), depth + 1);
    }
  };
  visit(result);
  return Array.from(ids).slice(0, 100);
}

export interface EvolutionSettings {
  enabled: boolean;
  coach_mode: boolean;
  local_only: boolean;
  review_trigger: "explicit" | string;
  retain_raw_conversation: boolean;
  profile_version: number;
}

export interface EvolutionRule {
  rule_id: string;
  kind: string;
  statement: string;
  source_review_id: string;
  evidence_refs: string[];
  status: string;
  version: number;
  confirmed_at: number;
}

export interface EvolutionProfile {
  settings: EvolutionSettings;
  profile_version: number;
  rules: EvolutionRule[];
  suppressed_kinds: string[];
}


export interface SentimentStrategy {
  strategy_id: string;
  name: string;
  stage: string;
  market_regime?: string | null;
  payload: Record<string, unknown>;
  status: "active" | "disabled" | string;
  version: number;
  created_at: number;
  source_analysis_id?: string | null;
}

export interface SentimentStrategyVersion {
  version: number;
  payload: Record<string, unknown>;
  status: string;
  created_at: number;
}

export interface EvolutionReview {
  review_id: string;
  run_id: string;
  conversation_id?: string | null;
  status: string;
  review_mode: "deterministic" | "model" | string;
  research_goal: string;
  evidence_and_process: { evidence_ids: string[]; tool_calls: unknown[] };
  conclusion_quality: { answer: string; has_answer: boolean };
  blind_spot_candidates: Array<{ kind: string; statement: string; evidence_refs: string[] }>;
  rule_candidates: Array<{ rule_id: string; kind: string; statement: string; evidence_refs: string[] }>;
  model_feedback?: string | null;
}

export async function getEvolutionSettings(): Promise<EvolutionSettings> {
  return getJson("/api/evolution/settings");
}

export async function updateEvolutionSettings(value: Partial<EvolutionSettings>): Promise<EvolutionSettings> {
  return postJson("/api/evolution/settings", value);
}

export async function getEvolutionProfile(): Promise<EvolutionProfile> {
  return getJson("/api/evolution/profile");
}

export async function resetEvolutionProfile(): Promise<EvolutionProfile> {
  return postJson("/api/evolution/profile/reset", {});
}

export async function createEvolutionReview(payload: Record<string, unknown>): Promise<EvolutionReview> {
  return postJson("/api/evolution/review", payload);
}

export async function confirmEvolutionRule(payload: Record<string, unknown>): Promise<EvolutionProfile> {
  return postJson("/api/evolution/rules/confirm", payload);
}

export async function deleteEvolutionRule(ruleId: string): Promise<EvolutionProfile> {
  return postJson("/api/evolution/rules/delete", { rule_id: ruleId });
}

export async function editEvolutionRule(payload: Record<string, unknown>): Promise<EvolutionProfile> {
  return postJson("/api/evolution/rules/edit", payload);
}

export async function setEvolutionRuleStatus(ruleId: string, status: string): Promise<EvolutionProfile> {
  return postJson("/api/evolution/rules/status", { rule_id: ruleId, status });
}

export async function suppressEvolutionKind(payload: Record<string, unknown>): Promise<EvolutionProfile> {
  return postJson("/api/evolution/suppress-kind", payload);
}

export async function unsuppressEvolutionKind(kind: string): Promise<EvolutionProfile> {
  return postJson("/api/evolution/unsuppress-kind", { kind });
}

export async function saveSentimentStrategy(payload: Record<string, unknown>): Promise<Record<string, unknown>> {
  return postJson("/api/sentiment/strategies", payload);
}

export async function getSentimentStrategies(): Promise<{ items: SentimentStrategy[] }> {
  return getJson("/api/sentiment/strategies");
}

export async function getSentimentStrategyVersions(strategyId: string): Promise<{ strategy_id: string; items: SentimentStrategyVersion[] }> {
  return getJson(`/api/sentiment/strategies/versions?strategy_id=${encodeURIComponent(strategyId)}`);
}

export async function setSentimentStrategyStatus(strategyId: string, status: "active" | "disabled"): Promise<{ items: SentimentStrategy[] }> {
  return postJson("/api/sentiment/strategies/status", { strategy_id: strategyId, status });
}

export async function rollbackSentimentStrategy(strategyId: string, version: number): Promise<{ items: SentimentStrategy[] }> {
  return postJson("/api/sentiment/strategies/rollback", { strategy_id: strategyId, version });
}

export async function enhanceEvolutionReview(review: EvolutionReview, llm: Record<string, unknown>): Promise<EvolutionReview> {
  return postJson("/api/evolution/review/enhance", { review, llm });
}
