import { describe, expect, it, vi } from "vitest";
import { TAURI_GET_ROUTES, TAURI_GET_PREFIX_ROUTES, TAURI_POST_ROUTES } from "./tauri";

describe("Hong Kong market Tauri routes", () => {
  it("maps HK status and search to isolated native commands", async () => {
    const invoke = vi.fn().mockResolvedValue({ market: "hk", items: [] });
    await TAURI_GET_ROUTES["/api/hk/market-status"]({ invoke, parsed: new URL("https://app.test/api/hk/market-status"), path: "/api/hk/market-status" });
    await TAURI_GET_ROUTES["/api/hk/stock-search"]({ invoke, parsed: new URL("https://app.test/api/hk/stock-search?q=700&limit=5"), path: "/api/hk/stock-search" });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(["api_hk_market_status", "api_hk_stock_search"]);
    expect(invoke.mock.calls[1][1]).toEqual({ payload: { query: "700", limit: 5 } });
  });

  it("maps HK observe and refresh without touching mainland commands", async () => {
    const invoke = vi.fn().mockResolvedValue({ market: "hk" });
    const observeRoute = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/hk/observe/")!;
    await observeRoute.handler({ invoke, parsed: new URL("https://app.test/api/hk/observe/00700.HK?data_policy=cache_only"), path: "/api/hk/observe/00700.HK" });
    await TAURI_POST_ROUTES["/api/hk/refresh"]({ invoke, parsed: new URL("https://app.test/api/hk/refresh"), path: "/api/hk/refresh" });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(["api_hk_observe", "api_hk_market_refresh"]);
    expect(invoke.mock.calls[0][1]).toEqual({ payload: { code: "00700.HK", data_policy: "cache_only" } });
  });
});
