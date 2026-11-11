import type { AgentToolCall, ScreenCriteria } from "../types";

export type EvolutionRuleStatus = "active" | "paused" | "never_suggested" | "deleted";
export type SentimentStrategyStatus = "active" | "disabled";
export type SentimentStrategyVersionStatus = "active" | "disabled" | "superseded";
export type EvolutionReviewMode = "deterministic" | "model";

export interface EvolutionSettingsDto {
  enabled: boolean;
  coach_mode: boolean;
  local_only: true;
  review_trigger: "explicit";
  retain_raw_conversation: false;
  profile_version: number;
}

export interface EvolutionRuleDto {
  rule_id: string;
  kind: string;
  statement: string;
  source_review_id: string;
  evidence_refs: string[];
  status: EvolutionRuleStatus;
  version: number;
  confirmed_at: number;
}

export interface EvolutionProfileDto {
  settings: EvolutionSettingsDto;
  profile_version: number;
  rules: EvolutionRuleDto[];
  suppressed_kinds: string[];
}

export interface EvolutionToolCallDto extends AgentToolCall {
  name?: string;
  arguments?: Record<string, unknown>;
}

export interface EvolutionCandidateDto {
  kind: string;
  statement: string;
  evidence_refs: string[];
}

export interface EvolutionRuleCandidateDto extends EvolutionCandidateDto {
  rule_id: string;
}

export interface EvolutionReviewDto {
  review_id: string;
  run_id: string;
  conversation_id: string | null;
  status: string;
  review_mode: EvolutionReviewMode;
  research_goal: string;
  evidence_and_process: {
    evidence_ids: string[];
    tool_calls: EvolutionToolCallDto[];
  };
  conclusion_quality: { answer: string; has_answer: boolean };
  blind_spot_candidates: EvolutionCandidateDto[];
  rule_candidates: EvolutionRuleCandidateDto[];
  model_feedback: string | null;
  model_used?: boolean;
}

export interface SentimentStrategyChangeDto {
  field: string;
  relative_change: number;
  before?: unknown;
  after?: unknown;
  reason?: string;
}

export interface SentimentStrategyBacktestDto {
  period: string;
  baseline: Record<string, unknown>;
  candidate: Record<string, unknown>;
  passed?: boolean;
}

export interface SentimentStrategyPayloadDto {
  criteria?: ScreenCriteria;
  evidence_ids?: string[];
  changes?: SentimentStrategyChangeDto[];
  backtest?: SentimentStrategyBacktestDto;
  source_analysis_id?: string;
  rolled_back_from?: number;
}

export interface SentimentStrategyDto {
  strategy_id: string;
  name: string;
  stage: string;
  market_regime: string | null;
  payload: SentimentStrategyPayloadDto;
  status: SentimentStrategyStatus;
  version: number;
  created_at: number;
  source_analysis_id: string | null;
}

export interface SentimentStrategyVersionDto {
  version: number;
  payload: SentimentStrategyPayloadDto;
  status: SentimentStrategyVersionStatus;
  created_at: number;
}

export interface EvolutionReviewRequest {
  run_id: string;
  conversation_id?: string;
  question: string;
  answer: string;
  evidence_ids: string[];
  tool_calls: EvolutionToolCallDto[];
}

export interface EvolutionRuleConfirmRequest {
  rule_id: string;
  kind: string;
  statement: string;
  source_review_id: string;
  evidence_refs: string[];
}

export interface EvolutionRuleEditRequest {
  rule_id: string;
  statement: string;
  evidence_refs: string[];
}

export interface SentimentStrategySaveRequest {
  strategy_id: string;
  name: string;
  stage: string;
  market_regime?: string;
  evidence_ids: string[];
  changes: SentimentStrategyChangeDto[];
  criteria: ScreenCriteria;
  backtest: SentimentStrategyBacktestDto & { passed: true };
  source_analysis_id?: string;
}

export interface SentimentStrategySaveResponse {
  strategy_id: string;
  version: number;
  status: SentimentStrategyStatus;
}

function record(value: unknown, context: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`Expected ${context} object`);
  }
  return Object.fromEntries(Object.entries(value));
}

function stringValue(value: unknown, context: string): string {
  if (typeof value !== "string" || !value.trim()) throw new Error(`Expected ${context} string`);
  return value.trim();
}

function optionalString(value: unknown, context: string): string | null {
  if (value === null || value === undefined) return null;
  return stringValue(value, context);
}

function booleanValue(value: unknown, context: string): boolean {
  if (typeof value !== "boolean") throw new Error(`Expected ${context} boolean`);
  return value;
}

function numberValue(value: unknown, context: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`Expected ${context} number`);
  return value;
}

function arrayValue(value: unknown, context: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`Expected ${context} array`);
  return value;
}

function stringArray(value: unknown, context: string): string[] {
  return arrayValue(value, context).map((item) => stringValue(item, context));
}


function parseToolCall(value: unknown): EvolutionToolCallDto {
  const item = record(value, "tool call");
  const tool = item.tool === undefined ? undefined : stringValue(item.tool, "tool call tool");
  const name = item.name === undefined ? undefined : stringValue(item.name, "tool call name");
  if (!tool && !name) throw new Error("Tool call requires tool or name");

  const parsed: EvolutionToolCallDto = {};
  if (item.id !== undefined) parsed.id = stringValue(item.id, "tool call id");
  if (tool) parsed.tool = tool;
  if (name) parsed.name = name;
  if (item.label !== undefined) parsed.label = stringValue(item.label, "tool call label");
  if (item.status !== undefined) parsed.status = stringValue(item.status, "tool call status");
  if (item.input !== undefined && item.input !== null) parsed.input = record(item.input, "tool call input");
  if (item.arguments !== undefined) parsed.arguments = record(item.arguments, "tool call arguments");
  if (item.output_summary !== undefined) parsed.output_summary = optionalString(item.output_summary, "tool call output_summary");
  if (item.warnings !== undefined) parsed.warnings = stringArray(item.warnings, "tool call warnings");
  return parsed;
}

function parseRule(value: unknown): EvolutionRuleDto {
  const item = record(value, "rule");
  const status = stringValue(item.status, "rule status");
  if (!["active", "paused", "never_suggested", "deleted"].includes(status)) {
    throw new Error(`Invalid rule status: ${status}`);
  }
  return {
    rule_id: stringValue(item.rule_id, "rule_id"),
    kind: stringValue(item.kind, "rule kind"),
    statement: stringValue(item.statement, "rule statement"),
    source_review_id: stringValue(item.source_review_id, "rule source_review_id"),
    evidence_refs: stringArray(item.evidence_refs, "rule evidence_refs"),
    status: status as EvolutionRuleStatus,
    version: numberValue(item.version, "rule version"),
    confirmed_at: numberValue(item.confirmed_at, "rule confirmed_at"),
  };
}

function parseCriteria(value: unknown): ScreenCriteria | undefined {
  if (value === undefined || value === null) return undefined;
  const item = record(value, "strategy criteria");
  const criteria: ScreenCriteria = {};
  for (const field of [
    "min_roe",
    "max_pe",
    "max_pb",
    "min_market_cap_billion",
    "min_deducted_net_profit_billion",
    "min_deducted_net_profit_margin",
    "min_deducted_net_profit_growth_rate",
    "limit",
  ]) {
    if (item[field] !== undefined) criteria[field as keyof ScreenCriteria] = numberValue(item[field], `criteria ${field}`) as never;
  }
  for (const field of ["industry", "market_scope", "sort_by", "sort_dir"]) {
    if (item[field] !== undefined) criteria[field as keyof ScreenCriteria] = stringValue(item[field], `criteria ${field}`) as never;
  }
  if (item.include_st !== undefined) criteria.include_st = booleanValue(item.include_st, "criteria include_st");
  if (item.require_institution_buy_ratio_gt_sell_ratio !== undefined) {
    criteria.require_institution_buy_ratio_gt_sell_ratio = booleanValue(item.require_institution_buy_ratio_gt_sell_ratio, "criteria require_institution_buy_ratio_gt_sell_ratio");
  }
  if (item.score_profile !== undefined) criteria.score_profile = stringValue(item.score_profile, "criteria score_profile");
  return criteria;
}

function parseStrategyChange(value: unknown): SentimentStrategyChangeDto {
  const item = record(value, "strategy change");
  return {
    field: stringValue(item.field, "strategy change field"),
    relative_change: numberValue(item.relative_change, "strategy change relative_change"),
    ...(item.before !== undefined ? { before: item.before } : {}),
    ...(item.after !== undefined ? { after: item.after } : {}),
    ...(item.reason !== undefined ? { reason: stringValue(item.reason, "strategy change reason") } : {}),
  };
}

function parseStrategyBacktest(value: unknown): SentimentStrategyBacktestDto {
  const item = record(value, "strategy backtest");
  return {
    period: stringValue(item.period, "strategy backtest period"),
    baseline: record(item.baseline, "strategy backtest baseline"),
    candidate: record(item.candidate, "strategy backtest candidate"),
    ...(item.passed !== undefined ? { passed: booleanValue(item.passed, "strategy backtest passed") } : {}),
  };
}

function parseStrategyPayload(value: unknown): SentimentStrategyPayloadDto {
  const item = record(value, "strategy payload");
  return {
    criteria: parseCriteria(item.criteria),
    ...(item.evidence_ids !== undefined ? { evidence_ids: stringArray(item.evidence_ids, "strategy evidence_ids") } : {}),
    ...(item.changes !== undefined ? { changes: arrayValue(item.changes, "strategy changes").map(parseStrategyChange) } : {}),
    ...(item.backtest !== undefined ? { backtest: parseStrategyBacktest(item.backtest) } : {}),
    ...(item.source_analysis_id !== undefined ? { source_analysis_id: stringValue(item.source_analysis_id, "strategy source_analysis_id") } : {}),
    ...(item.rolled_back_from !== undefined ? { rolled_back_from: numberValue(item.rolled_back_from, "strategy rolled_back_from") } : {}),
  };
}

function parseCandidate(value: unknown, context: string): EvolutionCandidateDto {
  const item = record(value, context);
  return {
    kind: stringValue(item.kind, `${context} kind`),
    statement: stringValue(item.statement, `${context} statement`),
    evidence_refs: stringArray(item.evidence_refs ?? [], `${context} evidence_refs`),
  };
}

function parseRuleCandidate(value: unknown): EvolutionRuleCandidateDto {
  const candidate = parseCandidate(value, "rule candidate");
  return { ...candidate, rule_id: stringValue(record(value, "rule candidate").rule_id, "rule candidate rule_id") };
}

export function parseEvolutionSettings(value: unknown): EvolutionSettingsDto {
  const item = record(value, "evolution settings");
  if (item.local_only !== true || item.review_trigger !== "explicit" || item.retain_raw_conversation !== false) {
    throw new Error("Evolution settings violate local-only contract");
  }
  return {
    enabled: booleanValue(item.enabled, "settings enabled"),
    coach_mode: booleanValue(item.coach_mode, "settings coach_mode"),
    local_only: true,
    review_trigger: "explicit",
    retain_raw_conversation: false,
    profile_version: numberValue(item.profile_version, "settings profile_version"),
  };
}

export function parseEvolutionProfile(value: unknown): EvolutionProfileDto {
  const item = record(value, "evolution profile");
  return {
    settings: parseEvolutionSettings(item.settings),
    profile_version: numberValue(item.profile_version, "profile_version"),
    rules: arrayValue(item.rules, "rules").map(parseRule),
    suppressed_kinds: stringArray(item.suppressed_kinds, "suppressed_kinds"),
  };
}

export function parseEvolutionReview(value: unknown): EvolutionReviewDto {
  const item = record(value, "evolution review");
  const mode = stringValue(item.review_mode, "review_mode");
  if (mode !== "deterministic" && mode !== "model") throw new Error(`Invalid review_mode: ${mode}`);
  const process = record(item.evidence_and_process, "evidence_and_process");
  const quality = record(item.conclusion_quality, "conclusion_quality");
  const answer = quality.answer === undefined ? "" : quality.answer;
  if (typeof answer !== "string") throw new Error("Expected conclusion answer string");
  return {
    review_id: stringValue(item.review_id, "review_id"),
    run_id: stringValue(item.run_id, "run_id"),
    conversation_id: optionalString(item.conversation_id, "conversation_id"),
    status: stringValue(item.status, "review status"),
    review_mode: mode,
    research_goal: stringValue(item.research_goal, "research_goal"),
    evidence_and_process: {
      evidence_ids: stringArray(process.evidence_ids, "evidence_ids"),
      tool_calls: arrayValue(process.tool_calls ?? [], "tool_calls").map(parseToolCall),
    },
    conclusion_quality: {
      answer,
      has_answer: booleanValue(quality.has_answer, "has_answer"),
    },
    blind_spot_candidates: arrayValue(item.blind_spot_candidates ?? [], "blind_spot_candidates").map((candidate) => parseCandidate(candidate, "blind spot")),
    rule_candidates: arrayValue(item.rule_candidates ?? [], "rule_candidates").map(parseRuleCandidate),
    model_feedback: optionalString(item.model_feedback, "model_feedback"),
    model_used: item.model_used === undefined ? undefined : booleanValue(item.model_used, "model_used"),
  };
}

export function parseSentimentStrategies(value: unknown): { items: SentimentStrategyDto[] } {
  const item = record(value, "strategy response");
  return {
    items: arrayValue(item.items, "strategy items").map((entry) => {
      const strategy = record(entry, "strategy");
      const status = stringValue(strategy.status, "strategy status");
      if (status !== "active" && status !== "disabled") throw new Error(`Invalid strategy status: ${status}`);
      return {
        strategy_id: stringValue(strategy.strategy_id, "strategy_id"),
        name: stringValue(strategy.name, "strategy name"),
        stage: stringValue(strategy.stage, "strategy stage"),
        market_regime: optionalString(strategy.market_regime, "market_regime"),
        payload: parseStrategyPayload(strategy.payload),
        status: status as SentimentStrategyStatus,
        version: numberValue(strategy.version, "strategy version"),
        created_at: numberValue(strategy.created_at, "strategy created_at"),
        source_analysis_id: optionalString(strategy.source_analysis_id, "source_analysis_id"),
      };
    }),
  };
}

export function parseSentimentStrategyVersions(value: unknown): { strategy_id: string; items: SentimentStrategyVersionDto[] } {
  const item = record(value, "strategy versions response");
  return {
    strategy_id: stringValue(item.strategy_id, "strategy_id"),
    items: arrayValue(item.items, "strategy versions").map((entry) => {
      const version = record(entry, "strategy version");
      const status = stringValue(version.status, "strategy version status");
      if (status !== "active" && status !== "disabled" && status !== "superseded") throw new Error(`Invalid strategy version status: ${status}`);
      return {
        version: numberValue(version.version, "version"),
        payload: parseStrategyPayload(version.payload),
        status: status as SentimentStrategyVersionStatus,
        created_at: numberValue(version.created_at, "version created_at"),
      };
    }),
  };
}

export function parseSentimentStrategySaveResponse(value: unknown): SentimentStrategySaveResponse {
  const item = record(value, "strategy save response");
  const status = stringValue(item.status, "strategy save status");
  if (status !== "active" && status !== "disabled") throw new Error(`Invalid strategy save status: ${status}`);
  return {
    strategy_id: stringValue(item.strategy_id, "strategy_id"),
    version: numberValue(item.version, "strategy version"),
    status,
  };
}
