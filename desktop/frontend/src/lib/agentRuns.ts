import type { AgentResult, AgentStreamEvent } from "../types";
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

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function boundedText(value: unknown, maxLength: number): string {
  return typeof value === "string" ? value.trim().slice(0, maxLength) : "";
}

function optionalNumber(value: unknown): number | undefined {
  if (value === null || value === undefined || value === "") return undefined;
  const number = Number(value);
  return Number.isFinite(number) ? number : undefined;
}

function normalizeStatus(value: unknown): AgentRunStatus {
  return value === "running" || value === "completed" || value === "failed"
    ? value
    : "unknown";
}

function normalizeLimit(value: number | undefined): number {
  if (value === undefined || Number.isNaN(value)) return 50;
  return Math.min(200, Math.max(1, Math.trunc(value)));
}

export function normalizeAgentRunSummary(value: unknown): AgentRunSummary | null {
  const record = asRecord(value);
  const runId = boundedText(record.run_id, 256);
  if (!runId) return null;

  return {
    runId,
    conversationId: boundedText(record.conversation_id, 256) || undefined,
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
      .map(normalizeAgentStreamEvent)
      .filter((event): event is AgentStreamEvent => Boolean(event && typeof event.type === "string"))
    : [];
  const rawResult = record.result;
  const result = rawResult && typeof rawResult === "object" && !Array.isArray(rawResult)
    ? normalizeAgentResult(rawResult)
    : undefined;

  return { ...summary, events, result };
}

export async function listAgentRuns(options: ListAgentRunsOptions = {}): Promise<AgentRunSummary[]> {
  const params = new URLSearchParams({ limit: String(normalizeLimit(options.limit)) });
  if (options.conversationId) params.set("conversation_id", options.conversationId);

  const response = asRecord(await getJson(`/api/agent/runs?${params.toString()}`, { signal: options.signal }));
  return Array.isArray(response.runs)
    ? response.runs
      .map(normalizeAgentRunSummary)
      .filter((run): run is AgentRunSummary => Boolean(run))
    : [];
}

export async function getAgentRun(runId: string, signal?: AbortSignal): Promise<AgentRunDetail | null> {
  const response = asRecord(await getJson(`/api/agent/runs/${encodeURIComponent(runId)}`, { signal }));
  return response.run == null ? null : normalizeAgentRunDetail(response.run);
}
