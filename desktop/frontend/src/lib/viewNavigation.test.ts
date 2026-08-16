import { describe, expect, it, vi } from "vitest";

import { consumeBacktestRouteRequest, nextBacktestRouteRequest, revealActivePanels } from "./viewNavigation";
import { DEFAULT_FILTER_CRITERIA } from "./screenCriteria";

describe("view navigation", () => {
  it("creates a new request when the same backtest source is selected again", () => {
    const first = nextBacktestRouteRequest(null, "watchlist");
    const second = nextBacktestRouteRequest(first, "watchlist");

    expect(first).toEqual({ source: "watchlist", requestId: 1 });
    expect(second).toEqual({ source: "watchlist", requestId: 2 });
  });

  it("only consumes the route request that the panel actually applied", () => {
    const current = { source: "watchlist" as const, requestId: 2 };

    expect(consumeBacktestRouteRequest(current, 1)).toBe(current);
    expect(consumeBacktestRouteRequest(current, 2)).toBeNull();
  });

  it("carries the complete adaptive screen spec into the backtest route", () => {
    const request = nextBacktestRouteRequest(null, "criteria", {
      criteria: { min_roe: 0.1 },
      mode: "auto",
      horizon: "swing_10_30d",
      primary_limit: 10,
      exploration_limit: 10,
      run_id: "screen-run",
    });
    expect(request.adaptiveScreenSpec).toMatchObject({
      mode: "auto",
      horizon: "swing_10_30d",
      primary_limit: 10,
      exploration_limit: 10,
    });
  });

  it("carries the originating tab criteria into the backtest route", () => {
    const criteriaSnapshot = {
      ...DEFAULT_FILTER_CRITERIA,
      industry: "影视院线",
      marketScope: "北交所",
    };
    const request = nextBacktestRouteRequest(null, "criteria", undefined, criteriaSnapshot);

    expect(request.criteriaSnapshot).toEqual(criteriaSnapshot);
  });

  it("reveals the main panels after navigating from the watchlist at the page bottom", () => {
    const scrollIntoView = vi.fn();
    const root = {
      querySelector: vi.fn(() => ({ scrollIntoView })),
    };

    revealActivePanels(root);

    expect(root.querySelector).toHaveBeenCalledWith(".panels");
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "start", behavior: "auto" });
  });
});
