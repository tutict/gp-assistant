import type { AdaptiveScreenRequest } from "../types";

export type BacktestSource = "criteria" | "watchlist";

export interface BacktestRouteRequest {
  source: BacktestSource;
  requestId: number;
  adaptiveScreenSpec?: AdaptiveScreenRequest;
}

interface PanelRevealRoot {
  querySelector(selector: string): { scrollIntoView(options: ScrollIntoViewOptions): void } | null;
}

export function nextBacktestRouteRequest(
  previous: BacktestRouteRequest | null,
  source: BacktestSource,
  adaptiveScreenSpec?: AdaptiveScreenRequest,
): BacktestRouteRequest {
  return {
    source,
    requestId: (previous?.requestId ?? 0) + 1,
    ...(adaptiveScreenSpec ? { adaptiveScreenSpec } : {}),
  };
}

export function consumeBacktestRouteRequest(
  current: BacktestRouteRequest | null,
  requestId: number,
): BacktestRouteRequest | null {
  return current?.requestId === requestId ? null : current;
}

export function revealActivePanels(root: PanelRevealRoot = document): void {
  root.querySelector(".panels")?.scrollIntoView({ block: "start", behavior: "auto" });
}
