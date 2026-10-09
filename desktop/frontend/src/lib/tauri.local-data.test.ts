import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
function setup() {
  vi.stubGlobal("window", { location: { href: "http://tauri.localhost/" }, __TAURI_INTERNALS__: {} });
  vi.stubGlobal("navigator", { userAgent: "Windows" });
  vi.stubGlobal("localStorage", { getItem: vi.fn(() => null) });
  const fetch = vi.fn(() => { throw new Error("cache-only must not prefetch"); });
  vi.stubGlobal("fetch", fetch);
  const invoke = vi.fn(async () => ({ data_metadata: { source: "local_cache" } }));
  return { fetch, invoke: invoke as typeof invoke & (<T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>) };
}
describe("local-first IPC", () => {
  it.each(["Windows", "Android"])("observe invokes native immediately on %s without prefetch or hydration", async userAgent => {
    const { invoke, fetch } = setup(); vi.stubGlobal("navigator", { userAgent });
    const { TAURI_GET_PREFIX_ROUTES } = await import("./tauri");
    const route = TAURI_GET_PREFIX_ROUTES.find(r => r.prefix === "/api/observe/")!;
    const result = await route.handler({ invoke, path: "/api/observe/000001.SZ", parsed: new URL("http://tauri.localhost/api/observe/000001.SZ?request_id=req-1&job_id=job-1") });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("api_observe", { payload: expect.objectContaining({ code: "000001.SZ", data_policy: "cache_only", request_id: "req-1", job_id: "job-1" }) });
    expect(fetch).not.toHaveBeenCalled();
    expect(result).toEqual({ data_metadata: { source: "local_cache" } });
  });
  it.each(["screen", "sector-screen", "custom-screen", "graph-screen", "trend", "trend-screen", "backtest"])("%s defaults to cache_only without fetching bundled data", async name => {
    const { invoke, fetch } = setup(); const { TAURI_POST_ROUTES } = await import("./tauri");
    const path = "/api/" + name;
    await TAURI_POST_ROUTES[path]({ invoke, path, parsed: new URL("http://tauri.localhost" + path), payload: { request_id: "r", job_id: "j" } });
    expect(invoke).toHaveBeenCalledWith(expect.any(String), { payload: { data_policy: "cache_only", request_id: "r", job_id: "j" } });
    expect(fetch).not.toHaveBeenCalled();
  });
  it("preserves explicit refresh and cold-start flags", async () => {
    const { invoke } = setup(); const { TAURI_POST_ROUTES } = await import("./tauri");
    await TAURI_POST_ROUTES["/api/screen"]({ invoke, path: "/api/screen", parsed: new URL("http://tauri.localhost/api/screen"), payload: { data_policy: "refresh", internal_release_validation_cold_start: true } });
    expect(invoke).toHaveBeenCalledWith("api_screen", { payload: expect.objectContaining({ data_policy: "refresh", internal_release_validation_cold_start: true }) });
  });
  it("formats structured missing-data errors without pretending to return a result", async () => {
    const { invoke } = setup(); invoke.mockRejectedValueOnce(JSON.stringify({ code: "LOCAL_DATA_MISSING", operation: "observe", missing_count: 1, missing: [{ kind: "history", code: "000001.SZ", available_bars: 0, required_bars: 3 }], action: "refresh" }));
    const { TAURI_GET_PREFIX_ROUTES } = await import("./tauri");
    await expect(TAURI_GET_PREFIX_ROUTES.find(r => r.prefix === "/api/observe/")!.handler({ invoke, path: "/api/observe/000001.SZ", parsed: new URL("http://tauri.localhost/api/observe/000001.SZ") })).rejects.toThrow(/本地数据不足.*000001.SZ.*刷新/s);
  });
});

describe("local metadata UI", () => {
  it("shows actual cache dates and coverage without calling data live", async () => {
    const { localDataSummary } = await import("./localData");
    const summary = localDataSummary({ data_metadata: { data_policy: "cache_only", quote_as_of: "20200103", history_as_of: "20200102", coverage_count: 1, coverage: [{code:"000001.SZ",available_bars:60,required_bars:60}] } });
    expect(summary).toContain("未联网"); expect(summary).toContain("20200102"); expect(summary).toContain("60/60");
    expect(localDataSummary({})).toBeNull();
  });
});
