import { getJson, postJson } from "./tauri";

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

export async function saveSentimentStrategy(payload: Record<string, unknown>): Promise<Record<string, unknown>> {
  return postJson("/api/sentiment/strategies", payload);
}

export async function getSentimentStrategies(): Promise<{ items: unknown[] }> {
  return getJson("/api/sentiment/strategies");
}
