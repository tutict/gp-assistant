import type { WatchlistItem } from "../types";
import { buildNewsRagRequest } from "./contracts";
import { normalizeStockCode } from "./format";
import { postJson } from "./tauri";

type ResearchRefreshRequest = (
  path: string,
  payload: Record<string, unknown>,
  options?: { signal?: AbortSignal },
) => Promise<unknown>;

export interface ResearchWatchlistRefreshResult {
  refreshed: string[];
  failed: Array<{ code: string; error: string }>;
}

export async function refreshResearchWatchlist(
  watchlist: WatchlistItem[],
  request: ResearchRefreshRequest = postJson,
  signal?: AbortSignal,
): Promise<ResearchWatchlistRefreshResult> {
  const codes = [...new Set(watchlist
    .map((item) => normalizeStockCode(item.code))
    .filter(Boolean))];
  const result: ResearchWatchlistRefreshResult = { refreshed: [], failed: [] };
  for (const [index, code] of codes.entries()) {
    if (signal?.aborted) break;
    try {
      const payload = buildNewsRagRequest(code, 30, undefined, watchlist, index === 0);
      if (signal) await request("/api/research/refresh", payload, { signal });
      else await request("/api/research/refresh", payload);
      result.refreshed.push(code);
    } catch (error) {
      if (signal?.aborted) break;
      result.failed.push({
        code,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return result;
}
