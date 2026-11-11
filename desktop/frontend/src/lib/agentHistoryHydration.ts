import type { AgentHistoryMessage } from "./agent";
import { boundedAgentRunId } from "./agent";
import { getAgentRun } from "./agentRuns";
import type { AgentResult } from "../types";

export type AgentHistoryResultCache = Map<string, Promise<AgentResult | undefined>>;
const MAX_HISTORY_RUNS = 12;

export function awaitAgentHistory<T extends AgentHistoryMessage>(pending: Promise<T[]>, signals: AbortSignal[]): Promise<T[]> {
  return new Promise((resolve, reject) => {
    const cleanup = () => signals.forEach(signal => signal.removeEventListener("abort", abort));
    const abort = () => {
      cleanup();
      const error = new Error("已取消");
      error.name = "AbortError";
      reject(error);
    };
    if (signals.some(signal => signal.aborted)) { abort(); return; }
    signals.forEach(signal => signal.addEventListener("abort", abort, { once: true }));
    pending.then(history => { cleanup(); resolve(history); }, error => { cleanup(); reject(error); });
  });
}

/** Reads only ledger-backed HK snapshots; the workspace keeps its lightweight run references. */
export async function hydrateAgentHistory<T extends AgentHistoryMessage>(
  history: T[], conversationId: string, market: "cn" | "hk", signal: AbortSignal,
  cache: AgentHistoryResultCache, excludedRunIds: ReadonlySet<string> = new Set(),
): Promise<T[]> {
  if (market !== "hk" || signal.aborted || !boundedAgentRunId(conversationId)) return history;
  const candidates = history.filter(message => message.role === "assistant" && !message.error && !message.result
    && boundedAgentRunId(message.runId) && !excludedRunIds.has(message.runId!)).slice(-MAX_HISTORY_RUNS);
  const runIds = [...new Set(candidates.map(message => boundedAgentRunId(message.runId)!))];
  // A long-lived chat may accumulate unavailable cancelled runs. Keep only a bounded retry cache.
  for (const key of cache.keys()) {
    if (cache.size <= MAX_HISTORY_RUNS * 2) break;
    if (!runIds.includes(key)) cache.delete(key);
  }
  await Promise.all(runIds.map(runId => {
    if (!cache.has(runId)) {
      const request = getAgentRun(runId, signal).then(detail => {
        if (signal.aborted) { cache.delete(runId); return undefined; }
        return detail?.status === "completed" && detail.conversationId === conversationId && !detail.resultUnavailable
          && detail.result?.research_context?.market === "hk" ? detail.result : undefined;
      }).catch(() => { cache.delete(runId); return undefined; });
      cache.set(runId, request);
    }
    return cache.get(runId);
  }));
  if (signal.aborted) return history;
  const results = new Map(await Promise.all(runIds.map(async runId => [runId, await cache.get(runId)] as const)));
  let changed = false;
  const hydrated = history.map(message => {
    const result = message.role === "assistant" && !message.error && !message.result ? results.get(boundedAgentRunId(message.runId) || "") : undefined;
    if (!result) return message;
    changed = true;
    return { ...message, result };
  });
  return changed ? hydrated : history;
}
