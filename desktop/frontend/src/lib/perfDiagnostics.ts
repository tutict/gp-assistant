export type PerfFlow = "observe" | "screening";

export type PerfStage =
  | "user_trigger"
  | "financial_snapshot_prefetch"
  | "history_prefetch"
  | "cache_snapshot_prepare"
  | "financial_supplement"
  | "history_supplement"
  | "network_supplement"
  | "candidate_generation"
  | "rust_calculation"
  | "result_persistence"
  | "command_total"
  | "ipc_roundtrip"
  | "trend_hydration"
  | "frontend_quant"
  | "frontend_kdj"
  | "first_stable_render"
  | "universe_refresh";

export type PerfCacheStrategy = "cache_only" | "refresh" | "full" | "local" | "mixed";
export type PerfOutcome = "ok" | "error" | "timeout" | "cache_hit" | "cache_miss" | "partial";

const PERF_FLOWS: readonly PerfFlow[] = ["observe", "screening"];
const PERF_STAGES: readonly PerfStage[] = [
  "user_trigger", "financial_snapshot_prefetch", "history_prefetch", "cache_snapshot_prepare",
  "financial_supplement", "history_supplement", "network_supplement", "candidate_generation",
  "rust_calculation", "result_persistence", "command_total", "ipc_roundtrip", "trend_hydration",
  "frontend_quant", "frontend_kdj", "first_stable_render", "universe_refresh",
];
const PERF_CACHE_STRATEGIES: readonly PerfCacheStrategy[] = ["cache_only", "refresh", "full", "local", "mixed"];
const PERF_OUTCOMES: readonly PerfOutcome[] = ["ok", "error", "timeout", "cache_hit", "cache_miss", "partial"];

export interface PerfRecord {
  flow: PerfFlow;
  stage: PerfStage;
  elapsedMs: number;
  cacheStrategy?: PerfCacheStrategy;
  stockCount?: number;
  candidateCount?: number;
  historyRows?: number;
  financialRecords?: number;
  requestBytes?: number;
  responseBytes?: number;
  outcome?: PerfOutcome;
}

export interface PerfDiagnosticsOptions {
  mobileRuntime: boolean;
  dev: boolean;
  testBuildFlag?: string;
}

export function shouldEnablePerfDiagnostics(options: PerfDiagnosticsOptions): boolean {
  return options.mobileRuntime && (options.dev || options.testBuildFlag === "1");
}

export function perfDiagnosticsAreEnabled(mobileRuntime: boolean): boolean {
  return shouldEnablePerfDiagnostics({
    mobileRuntime,
    dev: import.meta.env.DEV,
    testBuildFlag: import.meta.env.VITE_PERF_DIAGNOSTICS,
  });
}

export function formatPerfRecord(record: PerfRecord): string {
  const safe = {
    flow: PERF_FLOWS.includes(record.flow) ? record.flow : "unknown",
    stage: PERF_STAGES.includes(record.stage) ? record.stage : "unknown",
    elapsed_ms: Math.max(0, Math.round(Number.isFinite(record.elapsedMs) ? record.elapsedMs : 0)),
    ...(record.cacheStrategy && PERF_CACHE_STRATEGIES.includes(record.cacheStrategy)
      ? { cache_strategy: record.cacheStrategy }
      : {}),
    ...safeCount("stock_count", record.stockCount),
    ...safeCount("candidate_count", record.candidateCount),
    ...safeCount("history_rows", record.historyRows),
    ...safeCount("financial_records", record.financialRecords),
    ...safeCount("request_bytes", record.requestBytes),
    ...safeCount("response_bytes", record.responseBytes),
    ...(record.outcome && PERF_OUTCOMES.includes(record.outcome) ? { outcome: record.outcome } : {}),
  };
  return `[gp-perf] ${JSON.stringify(safe)}`;
}

export function logPerfRecord(record: PerfRecord, mobileRuntime: boolean): void {
  if (!mobileRuntime) return;
  if (import.meta.env.DEV) {
    console.info(formatPerfRecord(record));
    return;
  }
  if (import.meta.env.VITE_PERF_DIAGNOSTICS === "1") {
    console.info(formatPerfRecord(record));
  }
}

export function jsonByteLength(value: unknown): number | undefined {
  try {
    const serialized = JSON.stringify(value);
    return serialized === undefined ? undefined : new TextEncoder().encode(serialized).byteLength;
  } catch {
    return undefined;
  }
}

export function objectRecordCount(value: unknown): number | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  return Object.keys(value).length;
}

export function perfOutcomeForError(error: unknown): "error" | "timeout" {
  const message = error instanceof Error ? error.message : String(error);
  return /timeout|timed out|time out|超时|超过.{0,16}秒/i.test(message) ? "timeout" : "error";
}

function safeCount(key: string, value: number | undefined): Record<string, number> {
  return value !== undefined && Number.isFinite(value) && value >= 0
    ? { [key]: Math.round(value) }
    : {};
}
