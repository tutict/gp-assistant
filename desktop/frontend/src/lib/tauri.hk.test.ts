import { describe, expect, it, vi } from "vitest";
import { TAURI_GET_ROUTES, TAURI_GET_PREFIX_ROUTES, TAURI_POST_ROUTES } from "./tauri";

describe("Hong Kong market Tauri routes", () => {
  it("maps HK status and search to isolated native commands", async () => {
    const invoke = vi.fn().mockResolvedValue({ market: "hk", items: [] });
    await TAURI_GET_ROUTES["/api/hk/market-status"]({ invoke, parsed: new URL("https://app.test/api/hk/market-status"), path: "/api/hk/market-status" });
    await TAURI_GET_ROUTES["/api/hk/stock-search"]({ invoke, parsed: new URL("https://app.test/api/hk/stock-search?q=700&limit=5"), path: "/api/hk/stock-search" });
    await TAURI_GET_ROUTES["/api/hk/network-probe"]({ invoke, parsed: new URL("https://app.test/api/hk/network-probe?code=00700.HK"), path: "/api/hk/network-probe" });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(["api_hk_market_status", "api_hk_stock_search", "api_hk_network_probe"]);
    expect(invoke.mock.calls[1][1]).toEqual({ payload: { query: "700", limit: 5 } });
    expect(invoke.mock.calls[2][1]).toEqual({ payload: { code: "00700.HK" } });
  });

  it("runs the HK proxy probe with app-level proxy options", async () => {
    vi.stubGlobal("window", {
      __TAURI_INTERNALS__: {},
      localStorage: {
        getItem: (key: string) => key === "stock-optimizer-network-proxy-mode" ? "manual" : "http://127.0.0.1:7890",
      },
    });
    vi.stubGlobal("navigator", { userAgent: "Windows" });
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => key === "stock-optimizer-proxy-mode" ? "manual" : "http://127.0.0.1:7890",
    });
    try {
      const invoke = vi.fn().mockResolvedValue({ market: "hk", code: "00700", sources: [] });
      await TAURI_GET_ROUTES["/api/hk/network-probe"]({
        invoke,
        parsed: new URL("https://app.test/api/hk/network-probe?code=700.HK"),
        path: "/api/hk/network-probe",
      });

      expect(invoke).toHaveBeenCalledWith("api_hk_network_probe", {
        payload: {
          code: "700.HK",
          network_proxy_mode: "manual",
          network_proxy_url: "http://127.0.0.1:7890",
        },
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("maps HK observe and refresh without touching mainland commands", async () => {
    const invoke = vi.fn().mockResolvedValue({ market: "hk", security: { code: "00700", classification_status: "verified_ordinary_share" } });
    const observeRoute = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/hk/observe/")!;
    await observeRoute.handler({ invoke, parsed: new URL("https://app.test/api/hk/observe/00700.HK?data_policy=refresh&request_id=refresh-7"), path: "/api/hk/observe/00700.HK" });
    await TAURI_POST_ROUTES["/api/hk/refresh"]({ invoke, parsed: new URL("https://app.test/api/hk/refresh"), path: "/api/hk/refresh" });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(["api_hk_observe", "api_hk_market_refresh"]);
    expect(invoke.mock.calls[0][1]).toEqual({ payload: { code: "00700.HK", data_policy: "refresh", request_id: "refresh-7" } });
  });

  it("persists a user-confirmed ordinary-share classification in the HK cache", async () => {
    const invoke = vi.fn().mockResolvedValue({
      code: "00700",
      classification_status: "user_confirmed_ordinary_share",
    });
    await TAURI_POST_ROUTES["/api/hk/confirm-ordinary-share"]({
      invoke,
      parsed: new URL("https://app.test/api/hk/confirm-ordinary-share"),
      path: "/api/hk/confirm-ordinary-share",
      payload: { code: "00700.HK" },
    });

    expect(invoke).toHaveBeenCalledWith("api_hk_confirm_ordinary_share", { payload: { code: "00700.HK" } });
  });

  it("rejects a cached HK entry whose ordinary-share identity is unverified", async () => {
    const invoke = vi.fn().mockResolvedValue({ market: "hk", security: { code: "00700", classification_status: "unverified" } });
    const observeRoute = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/hk/observe/")!;

    await expect(observeRoute.handler({
      invoke,
      parsed: new URL("https://app.test/api/hk/observe/00700.HK"),
      path: "/api/hk/observe/00700.HK",
    })).rejects.toThrow("港股证券的交易所上市分类或证券身份不可用");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("accepts an explicitly user-confirmed ordinary share for HK observation", async () => {
    const invoke = vi.fn().mockResolvedValue({
      market: "hk",
      security: { code: "00700", classification_status: "user_confirmed_ordinary_share" },
    });
    const observeRoute = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/hk/observe/")!;

    await expect(observeRoute.handler({
      invoke,
      parsed: new URL("https://app.test/api/hk/observe/00700.HK"),
      path: "/api/hk/observe/00700.HK",
    })).resolves.toMatchObject({ security: { classification_status: "user_confirmed_ordinary_share" } });
  });

  it("accepts exchange-listed HK equities with a distinct listed-equity identity", async () => {
    const invoke = vi.fn().mockResolvedValue({
      market: "hk",
      security: { code: "00700", classification_status: "exchange_listed_equity" },
    });
    const observeRoute = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/hk/observe/")!;
    const result = await observeRoute.handler({
      invoke,
      parsed: new URL("https://app.test/api/hk/observe/00700.HK?data_policy=cache_only"),
      path: "/api/hk/observe/00700.HK",
    });

    expect(result).toMatchObject({ security: { classification_status: "exchange_listed_equity" } });
  });

  it("maps financial status and explicit refresh to dedicated HK commands", async () => {
    const invoke = vi.fn().mockResolvedValue({ market: "hk", snapshot: null, fx_rate: null });
    await TAURI_GET_ROUTES["/api/hk/financial-status"]({ invoke, parsed: new URL("https://app.test/api/hk/financial-status?code=00700.HK"), path: "/api/hk/financial-status" });
    const statusRoute = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/hk/financial-status/")!;
    await statusRoute.handler({ invoke, parsed: new URL("https://app.test/api/hk/financial-status/00700.HK"), path: "/api/hk/financial-status/00700.HK" });
    await TAURI_POST_ROUTES["/api/hk/financial-refresh"]({ invoke, parsed: new URL("https://app.test/api/hk/financial-refresh"), path: "/api/hk/financial-refresh", payload: { code: "00700.HK" } });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(["api_hk_financial_status", "api_hk_financial_status", "api_hk_financial_refresh"]);
    expect(invoke.mock.calls[0][1]).toEqual({ payload: { code: "00700.HK" } });
    expect(invoke.mock.calls[1][1]).toEqual({ payload: { code: "00700.HK" } });
  });

  it("passes saved network proxy settings to HK financial refresh without overriding provider settings", async () => {
    const values = new Map([
      ["stock-optimizer-network-proxy-mode", "manual"],
      ["stock-optimizer-network-proxy-url", "http://127.0.0.1:7890"],
    ]);
    vi.stubGlobal("window", {
      __TAURI_INTERNALS__: {},
      localStorage: { getItem: (key: string) => values.get(key) ?? null },
    });
    vi.stubGlobal("navigator", { userAgent: "Windows" });
    try {
      const invoke = vi.fn().mockResolvedValue({ market: "hk", snapshot: null });
      await TAURI_POST_ROUTES["/api/hk/financial-refresh"]({
        invoke,
        parsed: new URL("https://app.test/api/hk/financial-refresh"),
        path: "/api/hk/financial-refresh",
        payload: { code: "00700.HK", proxy_mode: "system", proxy_url: "http://llm-proxy:8080" },
      });

      expect(invoke).toHaveBeenCalledWith("api_hk_financial_refresh", {
        payload: {
          code: "00700.HK",
          proxy_mode: "system",
          proxy_url: "http://llm-proxy:8080",
          network_proxy_mode: "manual",
          network_proxy_url: "http://127.0.0.1:7890",
        },
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("does not replace an LLM provider proxy with the market-data proxy", async () => {
    vi.stubGlobal("window", {
      __TAURI_INTERNALS__: {},
      localStorage: {
        getItem: (key: string) => key === "stock-optimizer-network-proxy-mode" ? "manual" : "http://127.0.0.1:7890",
      },
    });
    vi.stubGlobal("navigator", { userAgent: "Windows" });
    try {
      const invoke = vi.fn().mockResolvedValue({ ok: true });
      const payload = { proxy_mode: "manual", proxy_url: "http://llm-proxy.local:8080", provider_id: "provider-1" };
      await TAURI_POST_ROUTES["/api/llm/test"]({
        invoke,
        parsed: new URL("https://app.test/api/llm/test"),
        path: "/api/llm/test",
        payload,
      });

      expect(invoke).toHaveBeenCalledWith("api_llm_test", { payload });
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
