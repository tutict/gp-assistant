import { describe, expect, it, vi } from "vitest";
import { TAURI_GET_PREFIX_ROUTES, TAURI_GET_ROUTES, TAURI_POST_ROUTES } from "./tauri";

describe("external data Tauri routes", () => {
  it("reads external data status without a refresh command", async () => {
    const invoke = vi.fn(async (_command: string, _args?: Record<string, unknown>) => ({ record_count: 0 }));
    const status = await TAURI_GET_ROUTES["/api/data/status"]({
      invoke: invoke as unknown as Parameters<typeof TAURI_GET_ROUTES["/api/data/status"]>[0]["invoke"],
      parsed: new URL("https://app.test/api/data/status"),
      path: "/api/data/status",
    });
    expect(invoke).toHaveBeenCalledWith("api_data_status");
    expect(status).toEqual({ record_count: 0 });
  });

  it("routes cleanup through preview and explicit confirmation payloads", async () => {
    const invoke = vi.fn(async (_command: string, _args?: Record<string, unknown>) => ({}));
    const routeInvoke = invoke as unknown as Parameters<typeof TAURI_GET_ROUTES["/api/data/status"]>[0]["invoke"];
    const payload = { domains: ["quote"], cutoff: "2025-01-01" };
    await TAURI_POST_ROUTES["/api/data/cleanup/preview"]({
      invoke: routeInvoke,
      parsed: new URL("https://app.test/api/data/cleanup/preview"),
      path: "/api/data/cleanup/preview",
      payload,
    });
    await TAURI_POST_ROUTES["/api/data/cleanup"]({
      invoke: routeInvoke,
      parsed: new URL("https://app.test/api/data/cleanup"),
      path: "/api/data/cleanup",
      payload: { ...payload, preview_token: "confirmed-preview" },
    });
    expect(invoke.mock.calls).toEqual([
      ["api_data_cleanup_preview", { payload }],
      ["api_data_cleanup", { payload: { ...payload, preview_token: "confirmed-preview" } }],
    ]);
  });

  it("routes generic explicit refresh, persisted run status, and foreground recovery", async () => {
    const invoke = vi.fn(async (_command: string, _args?: Record<string, unknown>) => ({}));
    const routeInvoke = invoke as unknown as Parameters<typeof TAURI_GET_ROUTES["/api/data/status"]>[0]["invoke"];
    await TAURI_POST_ROUTES["/api/data/refresh"]({
      invoke: routeInvoke,
      parsed: new URL("https://app.test/api/data/refresh"),
      path: "/api/data/refresh",
      payload: { market: "cn", domain: "quote", request_id: "user-refresh-1" },
    });
    const refreshStatus = TAURI_GET_PREFIX_ROUTES.find((route) => route.prefix === "/api/data/refresh/")!;
    await refreshStatus.handler({
      invoke: routeInvoke,
      parsed: new URL("https://app.test/api/data/refresh/run-123"),
      path: "/api/data/refresh/run-123",
    });
    await TAURI_POST_ROUTES["/api/data/resume-explicit"]({
      invoke: routeInvoke,
      parsed: new URL("https://app.test/api/data/resume-explicit"),
      path: "/api/data/resume-explicit",
    });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "api_data_refresh",
      "api_data_refresh_status",
      "api_data_resume_explicit",
    ]);
    expect(invoke.mock.calls[0][1]).toEqual({
      payload: { market: "cn", domain: "quote", request_id: "user-refresh-1" },
    });
    expect(invoke.mock.calls[1][1]).toEqual({ runId: "run-123" });
  });

  it("routes Hong Kong refresh through the durable refresh contract and preserves provider errors", async () => {
    const providerError = new Error("provider unavailable");
    const invoke = vi.fn(async () => { throw providerError; });
    const payload = {
      market: "hk",
      domain: "financial_snapshot",
      entity_codes: ["00700.HK"],
      request_id: "hk-financial-1",
    };

    await expect(TAURI_POST_ROUTES["/api/data/refresh"]({
      invoke: invoke as unknown as Parameters<typeof TAURI_POST_ROUTES["/api/data/refresh"]>[0]["invoke"],
      parsed: new URL("https://app.test/api/data/refresh"),
      path: "/api/data/refresh",
      payload,
    })).rejects.toBe(providerError);

    expect(invoke).toHaveBeenCalledWith("api_data_refresh", { payload });
  });
});
