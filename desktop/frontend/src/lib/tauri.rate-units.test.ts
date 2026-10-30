import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  vi.stubGlobal("window", { location: { href: "http://localhost/" }, __TAURI_INTERNALS__: {} });
  vi.stubGlobal("navigator", { userAgent: "Windows" });
  vi.stubGlobal("localStorage", { getItem: () => null });
});
afterEach(() => vi.unstubAllGlobals());

describe("financial snapshot IPC units", () => {
  it("does not rescale canonical stock ratios while adding missing industry metadata", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok:true,json:async()=>({ industries:{"000001.SZ":"银行"},financials:{} }) })));
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    await TAURI_POST_ROUTES["/api/screen"]({ invoke,path:"/api/screen",parsed:new URL("http://localhost/api/screen"),payload:{data_policy:"refresh",criteria:{industry:"银行"},financial_snapshot:{
      schema_version:"mobile-financial-snapshot/v4",metric_units:{roe:"percentage_points"},stock_metric_units:{roe:"ratio"},
      stocks:[{code:"000001.SZ",roe:1.2}],financials:{"000001.SZ":{roe:120}},
    }} });
    expect(invoke.mock.calls[0][1].payload.financial_snapshot.stocks[0].roe).toBe(1.2);
  });

  it("requires explicit units for nested caller-provided financial rates", async () => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke=vi.fn().mockResolvedValue({});
    await expect(TAURI_POST_ROUTES["/api/screen"]({ invoke,path:"/api/screen",parsed:new URL("http://localhost/api/screen"),payload:{data_policy:"refresh",financial_snapshot:{stocks:[{code:"000001.SZ",financial:{roe:0.07}}]}} })).rejects.toThrow(/单位/);
    expect(invoke).not.toHaveBeenCalled();
  });

  it.each([
    { financials: { "000001.SZ": { roe: 0.12 } } },
    { stocks: [{ code: "000001.SZ", financial: { roe: 0.12 } }] },
    { stocks: [{ code: "000001.SZ", roe: 0.12 }] },
  ])("rejects undeclared v4 financial rates before native IPC: %j", async (rates) => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    const snapshot = { schema_version: "mobile-financial-snapshot/v4", ...rates };
    const original = structuredClone(snapshot);
    await expect(TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh", financial_snapshot: snapshot },
    })).rejects.toThrow(/缺少明确单位/);
    expect(invoke).not.toHaveBeenCalled();
    expect(snapshot).toEqual(original);
  });

  it("rejects an explicitly v4 bundled source without financial units before IPC", async () => {
    const snapshot = {
      schema_version: "mobile-financial-snapshot/v4",
      financials: { "000001.SZ": { roe: 0.12 } },
    };
    const original = structuredClone(snapshot);
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, json: async () => snapshot })));
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    await expect(TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh" },
    })).rejects.toThrow(/缺少明确单位/);
    expect(invoke).not.toHaveBeenCalled();
    expect(snapshot).toEqual(original);
  });

  it("preserves the fixed v3 percentage-point mapping through IPC", async () => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    await TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh", financial_snapshot: {
        schema_version: "mobile-financial-snapshot/v3",
        stocks: [{ code: "000001.SZ", roe: 0.12 }],
        financials: { "000001.SZ": { roe: 0.12 } },
      } },
    });
    const snapshot = invoke.mock.calls[0][1].payload.financial_snapshot;
    expect(snapshot.financials["000001.SZ"].roe).toBe(0.12);
    expect(snapshot.stocks[0].roe).toBe(0.0012);
    expect(snapshot.metric_units.roe).toBe("percentage_points");
    expect(snapshot.stock_metric_units.roe).toBe("ratio");
  });

  it("migrates a trusted unversioned bundled fixture without inferring units from magnitude", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: true, json: async () => ({ financials: { "000001.SZ": { roe: 0.12 } } }),
    })));
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    await TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh" },
    });
    expect(invoke.mock.calls[0][1].payload.financial_snapshot.financials["000001.SZ"].roe).toBe(0.12);
  });

  it("keeps stock ratios separate from the financial DTO unit declaration", async () => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    await TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh", financial_snapshot: {
        schema_version: "mobile-financial-snapshot/v4", metric_units: { roe: "ratio" },
        stock_metric_units: { roe: "percentage_points" },
        stocks: [{ code: "000001.SZ", roe: 120 }], financials: { "000001.SZ": { roe: 1.2 } },
      } },
    });
    const snapshot = invoke.mock.calls[0][1].payload.financial_snapshot;
    expect(snapshot.stocks[0].roe).toBe(1.2);
    expect(snapshot.stocks[0].financial.roe).toBe(120);
    expect(snapshot.financials["000001.SZ"].roe).toBe(120);
    expect(snapshot.stock_metric_units.roe).toBe("ratio");
    await TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh", financial_snapshot: snapshot },
    });
    expect(invoke.mock.calls[1][1].payload.financial_snapshot).toEqual(snapshot);
  });

  it("converts declared ratios once and carries canonical snapshot units through IPC", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ({
      ok: true,
      json: async () => ({
        schema_version: "mobile-financial-snapshot/v4",
        metric_units: { roe: "ratio", deducted_net_profit_growth_rate: "ratio" },
        financials: { "000001.SZ": { roe: 0.0007, deducted_net_profit_growth_rate: 0.00804352 } },
      }),
    })));
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({ total: 0, returned: 0, items: [] });
    await TAURI_POST_ROUTES["/api/screen"]({ invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"), payload: { data_policy: "refresh" } });
    const snapshot = invoke.mock.calls[0][1].payload.financial_snapshot;
    expect(snapshot.schema_version).toBe("mobile-financial-snapshot/v4");
    expect(snapshot.metric_contract_version).toBe(2);
    expect(snapshot.metric_units.roe).toBe("percentage_points");
    expect(snapshot.financials["000001.SZ"].roe).toBeCloseTo(0.07, 12);
    expect(snapshot.financials["000001.SZ"].deducted_net_profit_growth_rate).toBeCloseTo(0.804352, 12);
  });

  it("rejects a caller-supplied rate with no unit before invoking native screening", async () => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invoke = vi.fn().mockResolvedValue({});
    await expect(TAURI_POST_ROUTES["/api/screen"]({
      invoke, path: "/api/screen", parsed: new URL("http://localhost/api/screen"),
      payload: { data_policy: "refresh", financial_snapshot: { financials: { "000001.SZ": { roe: 0.07 } } } },
    })).rejects.toThrow(/单位/);
    expect(invoke).not.toHaveBeenCalled();
  });
});
