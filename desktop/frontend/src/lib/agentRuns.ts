import type {
  AgentAnswerSection,
  AgentEvidenceItem,
  AgentHarnessMeta,
  AgentIntent,
  AgentResult,
  AgentStreamEvent,
  AgentToolCall,
} from "../types";
import { normalizeAgentResult, normalizeAgentStreamEvent } from "./contracts";
import { getJson } from "./tauri";

export type AgentRunStatus = "running" | "completed" | "failed" | "unknown";

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
}

export interface ListAgentRunsOptions {
  conversationId?: string;
  limit?: number;
  signal?: AbortSignal;
}

const SHORT_TEXT_MAX = 64;
const METADATA_TEXT_MAX = 256;
const DISPLAY_TEXT_MAX = 2_000;
const LONG_TEXT_MAX = 8_000;
const COLLECTION_ITEMS_MAX = 100;
const DOMAIN_DEPTH_MAX = 6;
const DOMAIN_KEYS_MAX = 100;
const SENSITIVE_KEY_PARTS = [
  "request",
  "llm",
  "network",
  "apikey",
  "authorization",
  "headers",
  "proxy",
  "credentials",
  "secret",
  "token",
  "config",
] as const;
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
  return identity && identity.length <= 256 ? identity : undefined;
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
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) return 50;
  return Math.min(200, Math.max(1, value));
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
  const normalized = normalizeAgentStreamEvent(value);
  const record = asRecord(normalized);
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
  const compactKey = key.toLowerCase().replace(/[^a-z0-9]/g, "");
  return SENSITIVE_KEY_PARTS.some((part) => compactKey.includes(part));
}

function cloneReplayJson(value: unknown, depth = 0, ancestors = new WeakSet<object>()): unknown {
  if (depth > DOMAIN_DEPTH_MAX) return undefined;
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") return value.slice(0, LONG_TEXT_MAX);
  if (typeof value === "number") return Number.isFinite(value) ? value : undefined;

  if (Array.isArray(value)) {
    if (ancestors.has(value)) return undefined;
    ancestors.add(value);
    const items = value
      .slice(0, COLLECTION_ITEMS_MAX)
      .map((item) => cloneReplayJson(item, depth + 1, ancestors))
      .filter((item) => item !== undefined);
    ancestors.delete(value);
    return items;
  }

  if (!isPlainRecord(value) || ancestors.has(value)) return undefined;
  ancestors.add(value);
  const cloned: Record<string, unknown> = {};
  let keptKeys = 0;
  for (const [key, item] of Object.entries(value)) {
    if (keptKeys >= DOMAIN_KEYS_MAX) break;
    if (
      !key
      || key.length > METADATA_TEXT_MAX
      || key === "__proto__"
      || key === "prototype"
      || key === "constructor"
      || isSensitiveKey(key)
    ) continue;
    const clonedItem = cloneReplayJson(item, depth + 1, ancestors);
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
    intent.symbols = value.symbols
      .slice(0, COLLECTION_ITEMS_MAX)
      .map(boundedIdentity)
      .filter((symbol): symbol is string => Boolean(symbol));
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
  if (input) call.input = input;
  if (Array.isArray(value.warnings)) call.warnings = normalizeStringList(value.warnings);
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
  if (Array.isArray(value.bullets)) section.bullets = normalizeStringList(value.bullets);
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

function normalizeReplayResult(value: unknown): AgentResult {
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
  for (const key of DOMAIN_RESULT_KEYS) {
    const domainValue = normalizeReplayRecord(normalizedRecord[key]);
    if (domainValue) resultRecord[key] = domainValue;
  }
  return result;
}

export function normalizeAgentRunSummary(value: unknown): AgentRunSummary | null {
  const record = asRecord(value);
  const runId = boundedIdentity(record.run_id);
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
      .map(normalizeReplayEvent)
      .filter((event): event is AgentStreamEvent => Boolean(event))
    : [];
  const rawResult = record.result;
  const result = rawResult && typeof rawResult === "object" && !Array.isArray(rawResult)
    ? normalizeReplayResult(rawResult)
    : undefined;

  return { ...summary, events, result };
}

export async function listAgentRuns(options: ListAgentRunsOptions = {}): Promise<AgentRunSummary[]> {
  const params = new URLSearchParams({ limit: String(normalizeLimit(options.limit)) });
  if (Object.prototype.hasOwnProperty.call(options, "conversationId")) {
    const conversationId = boundedIdentity(options.conversationId);
    if (!conversationId) return [];
    params.set("conversation_id", conversationId);
  }

  const response = asRecord(await getJson(`/api/agent/runs?${params.toString()}`, { signal: options.signal }));
  return Array.isArray(response.runs)
    ? response.runs
      .map(normalizeAgentRunSummary)
      .filter((run): run is AgentRunSummary => Boolean(run))
    : [];
}

export async function getAgentRun(runId: string, signal?: AbortSignal): Promise<AgentRunDetail | null> {
  const identity = boundedIdentity(runId);
  if (!identity) return null;
  const encodedRunId = identity === "." ? "%2E" : identity === ".." ? "%2E%2E" : encodeURIComponent(identity);
  const response = asRecord(await getJson(`/api/agent/runs/${encodedRunId}`, { signal }));
  return response.run == null ? null : normalizeAgentRunDetail(response.run);
}
