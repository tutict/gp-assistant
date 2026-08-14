import { afterEach, describe, expect, it, vi } from "vitest";

describe("desktop financial snapshot routes", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("attaches the bundled financial snapshot outside the mobile runtime", async () => {
    vi.stubGlobal("window", {
      location: { href: "http://tauri.localhost/" },
      __TAURI_INTERNALS__: {},
    });
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" });
    vi.stubGlobal("localStorage", { getItem: vi.fn(() => null) });
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/mobile-financial-snapshot.json")) {
        return new Response(JSON.stringify({
          financials: {
            "000100.SZ": {
              operating_revenue_billion: 434.778212,
              parent_net_profit_billion: 15.564526,
              gross_margin: 12.497484,
              asset_liability_ratio: 65.027185,
              period: "2026Q1",
            },
          },
        }));
      }
      return new Response(JSON.stringify({ data: null }));
    }));

    const { TAURI_GET_PREFIX_ROUTES } = await import("./tauri");
    const route = TAURI_GET_PREFIX_ROUTES.find(({ prefix }) => prefix === "/api/observe/");
    const invokeMock = vi.fn(async (
      _command: string,
      _args?: Record<string, unknown>,
    ): Promise<unknown> => ({}));
    const invoke = invokeMock as <T = unknown>(
      command: string,
      args?: Record<string, unknown>,
    ) => Promise<T>;

    await route?.handler({
      invoke,
      path: "/api/observe/000100.SZ",
      parsed: new URL("http://tauri.localhost/api/observe/000100.SZ"),
    });

    expect(invokeMock).toHaveBeenCalledWith("api_observe", {
      payload: expect.objectContaining({
        code: "000100.SZ",
        financial_snapshot: {
          stocks: [],
          financials: {
            "000100.SZ": expect.objectContaining({
              operating_revenue_billion: 434.778212,
              parent_net_profit_billion: 15.564526,
              gross_margin: 12.497484,
              asset_liability_ratio: 65.027185,
              period: "2026Q1",
            }),
          },
        },
      }),
    });
    const payload = (invokeMock.mock.calls[0]?.[1] as { payload: Record<string, unknown> }).payload;
    expect(payload).not.toHaveProperty("mobile_fast_observe");
  });

  it("attaches complete screening financials to every screen request and after cache clearing", async () => {
    vi.stubGlobal("window", {
      location: { href: "http://tauri.localhost/" },
      __TAURI_INTERNALS__: {},
    });
    vi.stubGlobal("navigator", { userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" });
    vi.stubGlobal("localStorage", { getItem: vi.fn(() => null) });
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/mobile-financial-snapshot.json")) {
        return new Response(JSON.stringify({
          financials: {
            "002432.SZ": {
              deducted_net_profit_billion: 11.551,
              deducted_net_profit_margin: 20.59,
              deducted_net_profit_growth_rate: 7.48,
              latest_eps: 0.7014,
              period: "2026Q1",
            },
          },
        }));
      }
      return new Response(JSON.stringify({ data: null }));
    }));

    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invokeMock = vi.fn(async (
      _command: string,
      _args?: Record<string, unknown>,
    ): Promise<unknown> => ({}));
    const invoke = invokeMock as <T = unknown>(
      command: string,
      args?: Record<string, unknown>,
    ) => Promise<T>;
    const screenRoute = TAURI_POST_ROUTES["/api/screen"];
    const screenContext = {
      invoke,
      path: "/api/screen",
      parsed: new URL("http://tauri.localhost/api/screen"),
      payload: { limit: 10 },
    };

    await screenRoute?.(screenContext);
    await screenRoute?.(screenContext);

    let screenCalls = invokeMock.mock.calls.filter(([command]) => command === "api_screen");
    const expectScreenPayloadWithFinancials = (call: unknown) => {
      expect(call).toEqual({
        payload: expect.objectContaining({
          limit: 10,
          financial_snapshot: {
            financials: {
              "002432.SZ": expect.objectContaining({
                deducted_net_profit_billion: 11.551,
                deducted_net_profit_margin: 20.59,
                deducted_net_profit_growth_rate: 7.48,
                latest_eps: 0.7014,
                period: "2026Q1",
              }),
            },
          },
        }),
      });
    };
    expectScreenPayloadWithFinancials(screenCalls[0]?.[1]);
    expectScreenPayloadWithFinancials(screenCalls[1]?.[1]);

    await TAURI_POST_ROUTES["/api/trend-screen"]?.({
      invoke,
      path: "/api/trend-screen",
      parsed: new URL("http://tauri.localhost/api/trend-screen"),
      payload: { limit: 10 },
    });
    const trendScreenCalls = invokeMock.mock.calls.filter(([command]) => command === "api_trend_screen");
    expect(trendScreenCalls[0]?.[1]).toEqual({
      payload: expect.objectContaining({
        limit: 10,
        financial_snapshot: {
          financials: {
            "002432.SZ": expect.objectContaining({
              deducted_net_profit_billion: 11.551,
              deducted_net_profit_margin: 20.59,
              deducted_net_profit_growth_rate: 7.48,
              latest_eps: 0.7014,
              period: "2026Q1",
            }),
          },
        },
      }),
    });

    await TAURI_POST_ROUTES["/api/data-sources/prune-cache"]?.({
      invoke,
      path: "/api/data-sources/prune-cache",
      parsed: new URL("http://tauri.localhost/api/data-sources/prune-cache"),
    });
    await screenRoute?.(screenContext);

    screenCalls = invokeMock.mock.calls.filter(([command]) => command === "api_screen");
    expect(invokeMock).toHaveBeenCalledWith("api_market_clear_cache");
    const refreshedPayload = (screenCalls[2]?.[1] as { payload: Record<string, unknown> }).payload;
    const refreshedFinancials = (refreshedPayload.financial_snapshot as { financials: Record<string, unknown> }).financials;
    expect(refreshedFinancials["002432.SZ"]).toEqual(expect.objectContaining({ latest_eps: 0.7014 }));
  });
});
