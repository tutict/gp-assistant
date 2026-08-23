import type { WatchlistItem } from "../types";
import { buildNewsRagRequest } from "./contracts";
import { normalizeStockCode } from "./format";
import { postJson } from "./tauri";

type ResearchRefreshRequest = (
  path: string,
  payload: Record<string, unknown>,
) => Promise<unknown>;

export interface ResearchWatchlistRefreshResult {
  refreshed: string[];
  failed: Array<{ code: string; error: string }>;
}

export async function refreshResearchWatchlist(
  watchlist: WatchlistItem[],
  request: ResearchRefreshRequest = postJson,
): Promise<ResearchWatchlistRefreshResult> {
  const codes = [...new Set(watchlist
    .map((item) => normalizeStockCode(item.code))
    .filter(Boolean))];
  const result: ResearchWatchlistRefreshResult = { refreshed: [], failed: [] };
  for (const [index, code] of codes.entries()) {
    try {
      await request(
        "/api/research/refresh",
        buildNewsRagRequest(code, 30, undefined, watchlist, index === 0),
      );
      result.refreshed.push(code);
    } catch (error) {
      result.failed.push({
        code,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return result;
}
