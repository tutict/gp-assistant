import type { LlmClientConfig } from "../types";
import { getJson, getTauriListen, isTauriRuntime, postJson } from "./tauri";

export interface GepaRunReport {
  run_id: string;
  status: string;
  profile_id: string;
  base_prompt_version: string;
  candidate_prompt_version?: string | null;
  eval_suite_version: string;
  dataset_sha256: string;
  engine_version: string;
  seed: number;
  max_metric_calls: number;
  baseline_validation_score?: number | null;
  baseline_holdout_score?: number | null;
  candidate_validation_score?: number | null;
  candidate_holdout_score?: number | null;
  baseline_holdout?: Array<{ id: string; score: number; hard_failure?: string | null; feedback?: string[]; response?: unknown }>;
  baseline_validation?: Array<{ id: string; score: number; hard_failure?: string | null; feedback?: string[]; response?: unknown }>;
  candidate_validation?: Array<{ id: string; score: number; hard_failure?: string | null; feedback?: string[]; response?: unknown }>;
  candidate_holdout?: Array<{ id: string; score: number; hard_failure?: string | null; feedback?: string[]; response?: unknown }>;
  candidate_body?: string | null;
  error?: string | null;
}

export interface GepaEvent {
  run_id?: string;
  type?: string;
  stage?: string;
  percent?: number;
  message?: string;
  report?: GepaRunReport;
}

export function gepaLabAvailable(): boolean {
  return isTauriRuntime();
}

export async function getGepaStatus(): Promise<{ enabled: boolean; engine_version?: string }> {
  return getJson("/api/agent/gepa/status");
}

export async function startGepaRun(llm: LlmClientConfig, profileId: string, maxMetricCalls: number): Promise<{ run_id: string; status: string; profile_id: string; base_prompt_version: string; dataset_sha256: string }> {
  return postJson("/api/agent/gepa/start", { llm, profile_id: profileId, max_metric_calls: maxMetricCalls });
}

export async function cancelGepaRun(runId: string): Promise<{ run_id: string; cancelled: boolean }> {
  return postJson("/api/agent/gepa/cancel", { run_id: runId });
}

export async function getGepaReport(runId: string): Promise<GepaRunReport> {
  return postJson("/api/agent/gepa/report", { run_id: runId });
}

export async function applyGepaRun(runId: string, expectedBasePromptVersion: string): Promise<Record<string, unknown>> {
  return postJson("/api/agent/gepa/apply", { run_id: runId, expected_base_prompt_version: expectedBasePromptVersion });
}

export async function listenGepaEvents(handler: (event: GepaEvent) => void): Promise<(() => void) | undefined> {
  const listen = getTauriListen();
  if (!listen) return undefined;
  return listen("agent-gepa-event", (event) => {
    const payload = event && typeof event === "object" && "payload" in event ? (event as { payload?: unknown }).payload : event;
    if (payload && typeof payload === "object") handler(payload as GepaEvent);
  });
}
