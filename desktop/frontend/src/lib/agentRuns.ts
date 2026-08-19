import type {
  AgentAnswerSection,
  AgentEvidenceItem,
  AgentHarnessMeta,
  AgentIntent,
  AgentResult,
  AgentStreamEvent,
  AgentToolCall,
} from "../types";
import {
  actionResultKind,
  normalizeAgentResult,
  normalizeAgentStreamEvent,
  requireBacktestResult,
} from "./contracts";
import { getJson, postJson } from "./tauri";

export type AgentRunStatus = "running" | "completed" | "failed" | "unknown";
export const MAX_AGENT_REPLAY_EVENTS = 1_000;

export interface AgentRunSummary {
  runId: string;
  conversationId?: string;
  question: string;
  mode: string;
  status: AgentRunStatus;
  startedAtEpochMs: number;
  completedAtEpochMs?: number;
  durationMs?: number;
  error?: string;
}

export interface AgentRunDetail extends AgentRunSummary {
  events: AgentStreamEvent[];
  result?: AgentResult;
  resultUnavailable?: boolean;
}

export interface ListAgentRunsOptions {
  conversationId?: string;
  limit?: number;
  signal?: AbortSignal;
}

export interface AgentRunMetrics {
  schemaVersion: number;
  sampleSize: number;
  sampleLimit: number;
  statusCounts: Record<string, number>;
  profileCounts: Record<string, {
    count: number;
    completed: number;
    failed: number;
    modelUsed: number;
    fallback: number;
  }>;
  modelOutcomeCounts: Record<string, number>;
  apiFormatCounts: Record<string, number>;
  durationMs: {
    count: number;
    averageMs?: number;
    p50Ms?: number;
    p95Ms?: number;
    maxMs?: number;
  };
}

const SHORT_TEXT_MAX = 64;
const METADATA_TEXT_MAX = 256;
const DISPLAY_TEXT_MAX = 2_000;
const LONG_TEXT_MAX = 8_000;
const COLLECTION_ITEMS_MAX = 100;
const DOMAIN_COLLECTION_ITEMS_MAX = 10_000;
const DOMAIN_DEPTH_MAX = 6;
const DOMAIN_KEYS_MAX = 100;
const MAX_AGENT_METRICS_ROWS = 2_000;
const SENSITIVE_KEYS = new Set([
  "request",
  "request_config",
  "llm",
  "llm_config",
  "network",
  "network_config",
  "api_key",
  "authorization",
  "authorization_header",
  "headers",
  "proxy",
  "proxy_config",
  "proxy_url",
  "credentials",
  "secret",
  "client_secret",
  "token",
  "access_token",
  "runtime_config",
  "provider_api_key_config",
  "config",
  "password",
  "refresh_token",
  "bearer_token",
  "cookie",
  "cookies",
  "session_token",
  "private_key",
  "client_key",
]);
const DOMAIN_RESULT_KEYS = [
  "criteria",
  "backtest",
  "news_rag",
  "observe",
  "sector_screen",
  "graph_screen",
  "trend_screen",
  "data",
] as const;

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  if (!isRecord(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

function boundedText(value: unknown, maxLength: number): string {
  return typeof value === "string" ? value.trim().slice(0, maxLength) : "";
}

function optionalBoundedText(value: unknown, maxLength: number): string | undefined {
  return boundedText(value, maxLength) || undefined;
}

function boundedIdentity(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const identity = value.trim();
  return identity && new TextEncoder().encode(identity).byteLength <= 256 ? identity : undefined;
}

function boundedRunIdentity(value: unknown): string | undefined {
  const identity = boundedIdentity(value);
  return identity === "." || identity === ".." ? undefined : identity;
}

function optionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : undefined;
}

function normalizeStatus(value: unknown): AgentRunStatus {
  return value === "running" || value === "completed" || value === "failed"
    ? value
    : "unknown";
}

function normalizeLimit(value: number | undefined): number {
  if (value === undefined) return 50;
  if (typeof value !== "number" || !Number.isFinite(value)) return 50;
  if (value <= 0) return 1;
  if (!Number.isSafeInteger(value)) return 50;
  return Math.min(200, value);
}

function normalizeMetricsLimit(value: number | undefined): number {
  if (value === undefined) return 200;
  if (typeof value !== "number" || !Number.isFinite(value)) return 200;
  if (value <= 0) return 1;
  if (!Number.isSafeInteger(value)) return 200;
  return Math.min(MAX_AGENT_METRICS_ROWS, value);
}

function normalizeTimelinePayload(value: unknown): Record<string, string> | undefined {
  const record = asRecord(value);
  const payload: Record<string, string> = {};
  const id = boundedIdentity(record.id);
  if (id) payload.id = id;

  for (const [key, maxLength] of [
    ["tool", SHORT_TEXT_MAX],
    ["label", DISPLAY_TEXT_MAX],
    ["status", SHORT_TEXT_MAX],
    ["output_summary", DISPLAY_TEXT_MAX],
  ] as const) {
    const text = optionalBoundedText(record[key], maxLength);
    if (text) payload[key] = text;
  }

  return Object.keys(payload).length ? payload : undefined;
}

function normalizeReplayEvent(value: unknown): AgentStreamEvent | null {
  const outerRecord = asRecord(value);
  const directType = typeof outerRecord.type === "string" ? outerRecord.type.trim() : "";
  const record = directType
    ? outerRecord
    : asRecord(normalizeAgentStreamEvent(value));
  const type = optionalBoundedText(record.type, SHORT_TEXT_MAX);
  if (!type) return null;

  const event: AgentStreamEvent = { type };
  const runId = boundedIdentity(record.run_id);
  if (runId) event.run_id = runId;

  for (const [key, maxLength] of [
    ["stage", SHORT_TEXT_MAX],
    ["label", DISPLAY_TEXT_MAX],
    ["action", SHORT_TEXT_MAX],
    ["message", DISPLAY_TEXT_MAX],
  ] as const) {
    const text = optionalBoundedText(record[key], maxLength);
    if (text) event[key] = text;
  }

  const percent = optionalNumber(record.percent);
  if (percent !== undefined && percent <= 100) event.percent = percent;
  const payload = normalizeTimelinePayload(record.payload);
  if (payload) event.payload = payload;
  return event;
}

function normalizeStringList(value: unknown, maxLength = DISPLAY_TEXT_MAX): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .slice(0, COLLECTION_ITEMS_MAX)
    .map((item) => optionalBoundedText(item, maxLength))
    .filter((item): item is string => Boolean(item));
}

function hasFields(value: object): boolean {
  return Object.keys(value).length > 0;
}

function isSensitiveKey(key: string): boolean {
  const normalizedKey = key
    .trim()
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[^a-zA-Z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();
  return SENSITIVE_KEYS.has(normalizedKey);
}

interface ReplayCloneState {
  incomplete: boolean;
}

function cloneReplayJson(
  value: unknown,
  depth = 0,
  ancestors = new WeakSet<object>(),
  state: ReplayCloneState = { incomplete: false },
): unknown {
  if (depth > DOMAIN_DEPTH_MAX) {
    state.incomplete = true;
    return undefined;
  }
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") {
    if (value.length > LONG_TEXT_MAX) state.incomplete = true;
    return value.slice(0, LONG_TEXT_MAX);
  }
  if (typeof value === "number") return Number.isFinite(value) ? value : undefined;

  if (Array.isArray(value)) {
    if (ancestors.has(value)) {
      state.incomplete = true;
      return undefined;
    }
    if (value.length > DOMAIN_COLLECTION_ITEMS_MAX) {
      state.incomplete = true;
      return undefined;
    }
    ancestors.add(value);
    const items = value
      .map((item) => cloneReplayJson(item, depth + 1, ancestors, state))
      .filter((item) => item !== undefined);
    ancestors.delete(value);
    return items;
  }

  if (!isPlainRecord(value) || ancestors.has(value)) {
    state.incomplete = true;
    return undefined;
  }
  ancestors.add(value);
  const cloned: Record<string, unknown> = {};
  let keptKeys = 0;
  for (const [key, item] of Object.entries(value)) {
    if (keptKeys >= DOMAIN_KEYS_MAX) {
      state.incomplete = true;
      break;
    }
    if (isSensitiveKey(key)) continue;
    if (
      !key
      || key.length > METADATA_TEXT_MAX
      || key === "__proto__"
      || key === "prototype"
      || key === "constructor"
    ) {
      state.incomplete = true;
      continue;
    }
    const clonedItem = cloneReplayJson(item, depth + 1, ancestors, state);
    if (clonedItem === undefined) continue;
    cloned[key] = clonedItem;
    keptKeys += 1;
  }
  ancestors.delete(value);
  return cloned;
}

function normalizeReplayRecord(value: unknown): Record<string, unknown> | undefined {
  const cloned = cloneReplayJson(value);
  return isPlainRecord(cloned) ? cloned : undefined;
}

function normalizeReplayDomainRecord(value: unknown): {
  complete: boolean;
  record?: Record<string, unknown>;
} {
  const state: ReplayCloneState = { incomplete: false };
  const cloned = cloneReplayJson(value, 0, new WeakSet<object>(), state);
  return {
    complete: !state.incomplete,
    ...(isPlainRecord(cloned) ? { record: cloned } : {}),
  };
}

function normalizeReplayIntent(value: unknown): AgentIntent | undefined {
  if (!isRecord(value)) return undefined;
  const intent: AgentIntent = {};
  for (const key of ["kind", "window", "depth", "mode"] as const) {
    const text = optionalBoundedText(value[key], SHORT_TEXT_MAX);
    if (text) intent[key] = text;
  }
  const query = optionalBoundedText(value.query, DISPLAY_TEXT_MAX);
  if (query) intent.query = query;
  if (Array.isArray(value.symbols)) {
    const symbols = value.symbols
      .slice(0, COLLECTION_ITEMS_MAX)
      .map(boundedIdentity)
      .filter((symbol): symbol is string => Boolean(symbol));
    if (symbols.length) intent.symbols = symbols;
  }
  return hasFields(intent) ? intent : undefined;
}

function normalizeReplayToolCall(value: unknown): AgentToolCall | null {
  if (!isRecord(value)) return null;
  const call: AgentToolCall = {};
  const id = boundedIdentity(value.id);
  if (id) call.id = id;
  for (const [key, maxLength] of [
    ["tool", SHORT_TEXT_MAX],
    ["label", DISPLAY_TEXT_MAX],
    ["status", SHORT_TEXT_MAX],
    ["output_summary", DISPLAY_TEXT_MAX],
  ] as const) {
    const text = optionalBoundedText(value[key], maxLength);
    if (text) call[key] = text;
  }
  const input = normalizeReplayRecord(value.input);
  if (input && hasFields(input)) call.input = input;
  if (Array.isArray(value.warnings)) {
    const warnings = normalizeStringList(value.warnings);
    if (warnings.length) call.warnings = warnings;
  }
  return hasFields(call) ? call : null;
}

function normalizeReplayEvidence(value: unknown): AgentEvidenceItem | null {
  if (!isRecord(value)) return null;
  const evidence: AgentEvidenceItem = {};
  for (const [key, maxLength] of [
    ["title", METADATA_TEXT_MAX],
    ["source", METADATA_TEXT_MAX],
    ["level", SHORT_TEXT_MAX],
    ["summary", DISPLAY_TEXT_MAX],
  ] as const) {
    const text = optionalBoundedText(value[key], maxLength);
    if (text) evidence[key] = text;
  }
  return hasFields(evidence) ? evidence : null;
}

function normalizeReplayAnswerSection(value: unknown): AgentAnswerSection | null {
  if (!isRecord(value)) return null;
  const section: AgentAnswerSection = {};
  const title = optionalBoundedText(value.title, METADATA_TEXT_MAX);
  if (title) section.title = title;
  if (Array.isArray(value.bullets)) {
    const bullets = normalizeStringList(value.bullets);
    if (bullets.length) section.bullets = bullets;
  }
  const provenance = optionalBoundedText(value.provenance, SHORT_TEXT_MAX);
  if (provenance) section.provenance = provenance;
  const evidenceBasis = optionalBoundedText(value.evidence_basis, DISPLAY_TEXT_MAX);
  if (evidenceBasis) section.evidence_basis = evidenceBasis;
  return hasFields(section) ? section : null;
}

function normalizeReplayHarness(value: unknown): AgentHarnessMeta | undefined {
  if (!isRecord(value)) return undefined;
  const harness: AgentHarnessMeta = {};
  for (const key of ["prompt_version", "profile_id", "model"] as const) {
    const text = optionalBoundedText(value[key], METADATA_TEXT_MAX);
    if (text) harness[key] = text;
  }
  if (typeof value.model_used === "boolean") harness.model_used = value.model_used;
  return hasFields(harness) ? harness : undefined;
}

function normalizeObjectList<T>(value: unknown, normalize: (item: unknown) => T | null): T[] {
  if (!Array.isArray(value)) return [];
  return value
    .slice(0, COLLECTION_ITEMS_MAX)
    .map(normalize)
    .filter((item): item is T => Boolean(item));
}

function normalizeReplayResult(value: unknown): {
  domainComplete: boolean;
  result: AgentResult;
} {
  const normalized = normalizeAgentResult(value);
  const result: AgentResult = {
    tool_calls: normalizeObjectList(normalized.tool_calls, normalizeReplayToolCall),
    evidence_summary: normalizeObjectList(normalized.evidence_summary, normalizeReplayEvidence),
    answer_sections: normalizeObjectList(normalized.answer_sections, normalizeReplayAnswerSection),
    model_answer_sections: normalizeObjectList(normalized.model_answer_sections, normalizeReplayAnswerSection),
    warnings: normalizeStringList(normalized.warnings),
    next_actions: normalizeStringList(normalized.next_actions),
  };

  const reply = optionalBoundedText(normalized.reply, LONG_TEXT_MAX);
  if (reply) result.reply = reply;
  const action = optionalBoundedText(normalized.action, SHORT_TEXT_MAX);
  if (action) result.action = action;
  const intent = normalizeReplayIntent(normalized.intent);
  if (intent) result.intent = intent;
  const harness = normalizeReplayHarness(normalized.harness);
  if (harness) result.harness = harness;
  const normalizedRecord = normalized as unknown as Record<string, unknown>;
  const resultRecord = result as unknown as Record<string, unknown>;
  let domainComplete = true;
  for (const key of DOMAIN_RESULT_KEYS) {
    if (normalizedRecord[key] == null) continue;
    const domain = normalizeReplayDomainRecord(normalizedRecord[key]);
    domainComplete &&= domain.complete;
    if (domain.record && hasFields(domain.record)) resultRecord[key] = domain.record;
  }
  return { domainComplete, result };
}

function hasMeaningfulReplayResult(result: AgentResult): boolean {
  if (result.reply || result.action) return true;
  if (result.intent && hasFields(result.intent)) return true;
  if (result.harness && hasFields(result.harness)) return true;
  if ([
    result.tool_calls,
    result.evidence_summary,
    result.answer_sections,
    result.model_answer_sections,
    result.warnings,
    result.next_actions,
  ].some((items) => Array.isArray(items) && items.length > 0)) return true;

  const resultRecord = result as unknown as Record<string, unknown>;
  return DOMAIN_RESULT_KEYS.some((key) => {
    const domainValue = resultRecord[key];
    return isPlainRecord(domainValue) && hasFields(domainValue);
  });
}

function hasRenderableReplayDomainResult(result: AgentResult): boolean {
  if (actionResultKind(result) !== "backtest") return true;
  const data = asRecord(result.data);
  const backtest = hasFields(data) ? data : asRecord(result.backtest);
  try {
    requireBacktestResult(backtest);
    return true;
  } catch {
    return false;
  }
}

export function normalizeAgentRunSummary(value: unknown): AgentRunSummary | null {
  const record = asRecord(value);
  const runId = boundedRunIdentity(record.run_id);
  if (!runId) return null;
  const conversationId = boundedIdentity(record.conversation_id);

  return {
    runId,
    ...(conversationId ? { conversationId } : {}),
    question: boundedText(record.question, 8_000),
    mode: boundedText(record.mode, 64) || "quick",
    status: normalizeStatus(record.status),
    startedAtEpochMs: optionalNumber(record.started_at_epoch_ms) ?? 0,
    completedAtEpochMs: optionalNumber(record.completed_at_epoch_ms),
    durationMs: optionalNumber(record.duration_ms),
    error: boundedText(record.error, 2_000) || undefined,
  };
}

export function normalizeAgentRunDetail(value: unknown): AgentRunDetail | null {
  const record = asRecord(value);
  const summary = normalizeAgentRunSummary(record);
  if (!summary) return null;

  const events = Array.isArray(record.events)
    ? record.events
      .slice(0, MAX_AGENT_REPLAY_EVENTS)
      .map(normalizeReplayEvent)
      .filter((event): event is AgentStreamEvent => Boolean(event))
    : [];
  const rawResult = record.result;
  if (rawResult == null) {
    return summary.status === "completed"
      ? { ...summary, events, resultUnavailable: true }
      : { ...summary, events };
  }
  if (!isPlainRecord(rawResult)) return { ...summary, events, resultUnavailable: true };

  const normalizedResult = normalizeReplayResult(rawResult);
  if (
    !normalizedResult.domainComplete
    || !hasMeaningfulReplayResult(normalizedResult.result)
    || !hasRenderableReplayDomainResult(normalizedResult.result)
  ) {
    return { ...summary, events, resultUnavailable: true };
  }

  return { ...summary, events, result: normalizedResult.result };
}

export async function listAgentRuns(options: ListAgentRunsOptions = {}): Promise<AgentRunSummary[]> {
  const limit = normalizeLimit(options.limit);
  const params = new URLSearchParams({ limit: String(limit) });
  if (Object.prototype.hasOwnProperty.call(options, "conversationId")) {
    const conversationId = boundedIdentity(options.conversationId);
    if (!conversationId) return [];
    params.set("conversation_id", conversationId);
  }

  const response = asRecord(await getJson(`/api/agent/runs?${params.toString()}`, { signal: options.signal }));
  return Array.isArray(response.runs)
    ? response.runs
      .slice(0, limit)
      .map(normalizeAgentRunSummary)
      .filter((run): run is AgentRunSummary => Boolean(run))
    : [];
}

function normalizeCounts(value: unknown): Record<string, number> {
  if (!isPlainRecord(value)) return {};
  return Object.fromEntries(Object.entries(value)
    .map(([key, count]) => [boundedText(key, 64), optionalNumber(count) ?? 0] as const)
    .filter(([key]) => Boolean(key)));
}

export function normalizeAgentRunMetrics(value: unknown): AgentRunMetrics | null {
  const record = asRecord(value);
  const duration = asRecord(record.duration_ms);
  const sampleSize = optionalNumber(record.sample_size);
  const sampleLimit = optionalNumber(record.sample_limit);
  if (sampleSize === undefined || sampleLimit === undefined) return null;
  const rawProfiles = isPlainRecord(record.profile_counts) ? record.profile_counts : {};
  const profileCounts: AgentRunMetrics["profileCounts"] = {};
  for (const [profile, raw] of Object.entries(rawProfiles)) {
    const profileId = boundedText(profile, 64);
    if (!profileId) continue;
    const bucket = asRecord(raw);
    profileCounts[profileId] = {
      count: optionalNumber(bucket.count) ?? 0,
      completed: optionalNumber(bucket.completed) ?? 0,
      failed: optionalNumber(bucket.failed) ?? 0,
      modelUsed: optionalNumber(bucket.model_used) ?? 0,
      fallback: optionalNumber(bucket.fallback) ?? 0,
    };
  }
  return {
    schemaVersion: optionalNumber(record.schema_version) ?? 1,
    sampleSize,
    sampleLimit,
    statusCounts: normalizeCounts(record.status_counts),
    profileCounts,
    modelOutcomeCounts: normalizeCounts(record.model_outcome_counts),
    apiFormatCounts: normalizeCounts(record.api_format_counts),
    durationMs: {
      count: optionalNumber(duration.count) ?? 0,
      averageMs: optionalNumber(duration.average_ms),
      p50Ms: optionalNumber(duration.p50_ms),
      p95Ms: optionalNumber(duration.p95_ms),
      maxMs: optionalNumber(duration.max_ms),
    },
  };
}

export async function getAgentRunMetrics(options: ListAgentRunsOptions = {}): Promise<AgentRunMetrics | null> {
  const limit = normalizeMetricsLimit(options.limit);
  const params = new URLSearchParams({ limit: String(limit) });
  if (Object.prototype.hasOwnProperty.call(options, "conversationId")) {
    const conversationId = boundedIdentity(options.conversationId);
    if (!conversationId) return null;
    params.set("conversation_id", conversationId);
  }
  const response = await getJson(`/api/agent/metrics?${params.toString()}`, { signal: options.signal });
  return normalizeAgentRunMetrics(response);
}

export async function getAgentRun(runId: string, signal?: AbortSignal): Promise<AgentRunDetail | null> {
  const identity = boundedRunIdentity(runId);
  if (!identity) return null;
  const response = asRecord(await getJson(`/api/agent/runs/${encodeURIComponent(identity)}`, { signal }));
  if (response.run == null) return null;
  const detail = normalizeAgentRunDetail(response.run);
  return detail?.runId === identity ? detail : null;
}

export async function deleteAgentConversationRuns(conversationId: string): Promise<number> {
  const identity = boundedIdentity(conversationId);
  if (!identity) throw new Error("conversation_id is required");
  const response = asRecord(await postJson("/api/agent/runs/delete-conversation", {
    conversation_id: identity,
  }, { timeoutMs: 10_000 }));
  return optionalNumber(response.deleted) ?? 0;
}
