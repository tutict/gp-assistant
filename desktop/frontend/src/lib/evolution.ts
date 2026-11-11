import type { AgentResult, LlmClientConfig, ScreenCriteria } from "../types";
import { getJson, postJson } from "./tauri";
import {
  parseEvolutionProfile,
  parseEvolutionReview,
  parseEvolutionSettings,
  parseSentimentStrategies,
  parseSentimentStrategyVersions,
  type EvolutionProfileDto,
  type EvolutionReviewDto,
  type EvolutionRuleDto,
  type EvolutionSettingsDto,
  type EvolutionReviewRequest,
  type EvolutionRuleConfirmRequest,
  type EvolutionRuleEditRequest,
  type SentimentStrategyDto,
  type SentimentStrategySaveRequest,
  type SentimentStrategyVersionDto,
  type SentimentStrategyStatus,
  type SentimentStrategySaveResponse,
  parseSentimentStrategySaveResponse,
  type EvolutionRuleStatus,
} from "./evolutionContracts";

export type EvolutionSettings = EvolutionSettingsDto;
export type EvolutionRule = EvolutionRuleDto;
export type EvolutionProfile = EvolutionProfileDto;
export type EvolutionReview = EvolutionReviewDto;
export type SentimentStrategy = SentimentStrategyDto;
export type SentimentStrategyVersion = SentimentStrategyVersionDto;
export type { EvolutionRuleStatus, SentimentStrategyStatus };

export function extractEvolutionEvidenceIds(result: AgentResult): string[] {
  const ids = new Set<string>();
  const visit = (value: unknown, evidenceContext = false, depth = 0): void => {
    if (depth > 6 || value == null) return;
    if (Array.isArray(value)) { value.forEach((item) => visit(item, evidenceContext, depth + 1)); return; }
    if (typeof value !== "object") return;
    const record = Object.fromEntries(Object.entries(value));
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

export async function getEvolutionSettings(): Promise<EvolutionSettings> { return parseEvolutionSettings(await getJson<unknown>("/api/evolution/settings")); }
export async function updateEvolutionSettings(value: Partial<Pick<EvolutionSettings, "enabled" | "coach_mode">>): Promise<EvolutionSettings> { return parseEvolutionSettings(await postJson<unknown>("/api/evolution/settings", value)); }
export async function getEvolutionProfile(): Promise<EvolutionProfile> { return parseEvolutionProfile(await getJson<unknown>("/api/evolution/profile")); }
export async function resetEvolutionProfile(): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/profile/reset", {})); }
export async function createEvolutionReview(payload: EvolutionReviewRequest): Promise<EvolutionReview> { return parseEvolutionReview(await postJson<unknown>("/api/evolution/review", payload)); }
export async function confirmEvolutionRule(payload: EvolutionRuleConfirmRequest): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/rules/confirm", payload)); }
export async function deleteEvolutionRule(ruleId: string): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/rules/delete", { rule_id: ruleId })); }
export async function editEvolutionRule(payload: EvolutionRuleEditRequest): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/rules/edit", payload)); }
export async function setEvolutionRuleStatus(ruleId: string, status: EvolutionRuleStatus): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/rules/status", { rule_id: ruleId, status })); }
export async function suppressEvolutionKind(payload: { kind: string; statement: string; source_review_id: string }): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/suppress-kind", payload)); }
export async function unsuppressEvolutionKind(kind: string): Promise<EvolutionProfile> { return parseEvolutionProfile(await postJson<unknown>("/api/evolution/unsuppress-kind", { kind })); }
export async function saveSentimentStrategy(payload: SentimentStrategySaveRequest): Promise<SentimentStrategySaveResponse> { return parseSentimentStrategySaveResponse(await postJson<unknown>("/api/sentiment/strategies", payload)); }
export async function getSentimentStrategies(): Promise<{ items: SentimentStrategy[] }> { return parseSentimentStrategies(await getJson<unknown>("/api/sentiment/strategies")); }
export async function getSentimentStrategyVersions(strategyId: string): Promise<{ strategy_id: string; items: SentimentStrategyVersion[] }> { return parseSentimentStrategyVersions(await getJson<unknown>(`/api/sentiment/strategies/versions?strategy_id=${encodeURIComponent(strategyId)}`)); }
export async function setSentimentStrategyStatus(strategyId: string, status: SentimentStrategyStatus): Promise<{ items: SentimentStrategy[] }> { return parseSentimentStrategies(await postJson<unknown>("/api/sentiment/strategies/status", { strategy_id: strategyId, status })); }
export async function rollbackSentimentStrategy(strategyId: string, version: number): Promise<{ items: SentimentStrategy[] }> { return parseSentimentStrategies(await postJson<unknown>("/api/sentiment/strategies/rollback", { strategy_id: strategyId, version })); }
export async function enhanceEvolutionReview(review: EvolutionReview, llm: LlmClientConfig): Promise<EvolutionReview> { return parseEvolutionReview(await postJson<unknown>("/api/evolution/review/enhance", { review, llm })); }

export type EvolutionCriteria = ScreenCriteria;
