import type { ScreenCriteria } from "../types";

export type EvolutionSentimentStage = "过热" | "降温" | "中性／分歧" | "低迷" | "修复" | "证据不足";
export interface SentimentBacktestSummary { period: string; baseline: { returned: number; maxDrawdown: number | null; concentration: number | null }; candidate: { returned: number; maxDrawdown: number | null; concentration: number | null } }
export interface SentimentProposalInput { stage: EvolutionSentimentStage; marketRegime?: string | null; currentCriteria: ScreenCriteria; evidenceIds: string[]; evidenceSummary: string; backtest: SentimentBacktestSummary }
export interface SentimentParameterChange { field: keyof ScreenCriteria; before: number | string | boolean | undefined; after: number | string | boolean | undefined; relativeChange: number; reason: string }
export interface SentimentParameterProposal { templateId: string; stage: EvolutionSentimentStage; marketRegime: string | null; changes: SentimentParameterChange[]; protectedFields: Array<keyof ScreenCriteria | "data_source" | "tool_permissions">; evidenceIds: string[]; evidenceSummary: string; backtest: SentimentBacktestSummary; requiresConfirmation: true; canSaveStrategy: boolean }

const PROTECTED_FIELDS: SentimentParameterProposal["protectedFields"] = ["include_st", "market_scope", "industry", "data_source", "tool_permissions"];
const round = (value: number) => Math.round(value * 100) / 100;
function boundedNumericChange(field: keyof ScreenCriteria, before: number | undefined, ratio: number, reason: string): SentimentParameterChange | null {
  if (before == null || !Number.isFinite(before)) return null;
  return { field, before, after: round(before * (1 + ratio)), relativeChange: ratio, reason };
}
function proposalChanges(input: SentimentProposalInput): SentimentParameterChange[] {
  const { currentCriteria: criteria, stage } = input;
  if (stage === "证据不足") return [];
  const changes: Array<SentimentParameterChange | null> = [];
  if (stage === "过热") changes.push(boundedNumericChange("max_pe", criteria.max_pe, -0.1, "情绪过热时收紧估值上限"), boundedNumericChange("max_pb", criteria.max_pb, -0.1, "情绪过热时收紧估值上限"), boundedNumericChange("limit", criteria.limit, -0.1, "情绪过热时减少候选噪声"));
  else if (stage === "降温") changes.push(boundedNumericChange("max_pe", criteria.max_pe, -0.2, "降温阶段收紧估值上限"), boundedNumericChange("max_pb", criteria.max_pb, -0.15, "降温阶段收紧估值上限"), boundedNumericChange("min_roe", criteria.min_roe, 0.1, "降温阶段提高质量门槛"), boundedNumericChange("min_deducted_net_profit_growth_rate", criteria.min_deducted_net_profit_growth_rate, 0.1, "降温阶段提高盈利增长门槛"));
  else if (stage === "低迷") changes.push(boundedNumericChange("min_market_cap_billion", criteria.min_market_cap_billion, 0.2, "低迷阶段优先保留流动性与规模"), boundedNumericChange("min_roe", criteria.min_roe, 0.1, "低迷阶段提高质量门槛"));
  else if (stage === "修复") changes.push(boundedNumericChange("min_deducted_net_profit_growth_rate", criteria.min_deducted_net_profit_growth_rate, 0.1, "修复阶段提高盈利修复门槛"), boundedNumericChange("limit", criteria.limit, 0.1, "修复阶段保留适度探索空间"));
  else if (stage === "中性／分歧") changes.push(boundedNumericChange("min_roe", criteria.min_roe, 0.05, "分歧阶段轻微提高质量门槛"));
  return changes.filter((change): change is SentimentParameterChange => Boolean(change));
}
export function buildSentimentParameterProposal(input: SentimentProposalInput): SentimentParameterProposal {
  const changes = proposalChanges(input).map((change) => ({ ...change, relativeChange: Math.max(-0.2, Math.min(0.2, change.relativeChange)) }));
  const candidateDrawdown = input.backtest.candidate.maxDrawdown;
  const baselineDrawdown = input.backtest.baseline.maxDrawdown;
  const backtestIsNonRegressive = candidateDrawdown != null && baselineDrawdown != null && Math.abs(candidateDrawdown) <= Math.abs(baselineDrawdown);
  return { templateId: input.stage === "证据不足" ? "insufficient-evidence-v1" : `sentiment-${input.stage}-v1`, stage: input.stage, marketRegime: input.marketRegime ?? null, changes, protectedFields: [...PROTECTED_FIELDS], evidenceIds: [...input.evidenceIds], evidenceSummary: input.evidenceSummary, backtest: input.backtest, requiresConfirmation: true, canSaveStrategy: input.stage !== "证据不足" && input.evidenceIds.length > 0 && backtestIsNonRegressive };
}
