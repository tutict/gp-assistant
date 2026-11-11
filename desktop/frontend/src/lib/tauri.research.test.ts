import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type InvokeFn = <T = unknown>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

describe("research Tauri routes", () => {
  it("reads the HK index database through the top-level native market parameter", async () => {
    const { TAURI_GET_ROUTES } = await import("./tauri");
    const invoke: InvokeFn = async <T,>(_command: string, args?: Record<string, unknown>) => ({
      document_count: args?.market === "hk" ? 2 : 5,
    }) as T;
    const result = await TAURI_GET_ROUTES["/api/research/index-status"]({
      invoke, path: "/api/research/index-status", parsed: new URL("http://tauri.localhost/api/research/index-status?market=hk"),
    });
    expect(result).toEqual({ document_count: 2 });
  });
  it("selects the requested native market for index status and both index rebuilds", async () => {
    const { TAURI_GET_ROUTES, TAURI_POST_ROUTES } = await import("./tauri");
    const invoke: InvokeFn = async <T,>(command: string, args?: Record<string, unknown>) => {
      const payload = args?.payload as { market?: string } | undefined;
      if (command === "api_research_index_status") return { market: args?.market || "cn" } as T;
      if (!payload) throw new Error("missing required native payload");
      return { market: payload.market || "cn" } as T;
    };
    for (const market of ["cn", "hk"]) {
      const index = await TAURI_GET_ROUTES["/api/research/index-status"]({
        invoke, path: "/api/research/index-status", parsed: new URL(`http://tauri.localhost/api/research/index-status?market=${market}`),
      });
      expect(index).toEqual({ market });
      for (const path of ["/api/research/rebuild-index", "/api/research/rebuild-embeddings"]) {
        const result = await TAURI_POST_ROUTES[path]({ invoke, path, parsed: new URL(`http://tauri.localhost${path}`), payload: { market } });
        expect(result).toEqual({ market });
      }
    }
  });
  beforeEach(() => {
    vi.stubGlobal("window", {
      location: { href: "http://tauri.localhost/" },
      __TAURI_INTERNALS__: {},
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("maps research reads and their query parameters to backend commands", async () => {
    const { TAURI_GET_ROUTES } = await import("./tauri");
    const invokeMock = vi.fn(async (): Promise<unknown> => ({}));
    const invoke = invokeMock as InvokeFn;

    await TAURI_GET_ROUTES["/api/research/overview"]?.({
      invoke,
      path: "/api/research/overview",
      parsed: new URL("http://tauri.localhost/api/research/overview?stock_code=600000.SH"),
    });
    await TAURI_GET_ROUTES["/api/research/messages"]?.({
      invoke,
      path: "/api/research/messages",
      parsed: new URL("http://tauri.localhost/api/research/messages?stock_code=600000.SH&unread_only=true&limit=120"),
    });
    await TAURI_GET_ROUTES["/api/research/threads"]?.({
      invoke,
      path: "/api/research/threads",
      parsed: new URL("http://tauri.localhost/api/research/threads"),
    });
    await TAURI_GET_ROUTES["/api/research/index-status"]?.({
      invoke,
      path: "/api/research/index-status",
      parsed: new URL("http://tauri.localhost/api/research/index-status"),
    });

    expect(invokeMock.mock.calls).toEqual([
      ["api_research_overview", { payload: { stock_code: "600000.SH" } }],
      ["api_research_messages", {
        payload: { stock_code: "600000.SH", unread_only: true, limit: 120 },
      }],
      ["api_research_threads"],
      ["api_research_index_status"],
    ]);
  });

  it("maps every research mutation used by NewsRagPanel", async () => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invokeMock = vi.fn(async (): Promise<unknown> => ({}));
    const invoke = invokeMock as InvokeFn;
    const payload = { stock_code: "600000.SH" };
    const routes = [
      ["/api/research/refresh", "api_research_refresh", true],
      ["/api/research/mark-read", "api_research_mark_read", true],
      ["/api/research/query", "api_research_query", true],
      ["/api/research/threads/create", "api_research_thread_create", true],
      ["/api/research/threads/detail", "api_research_thread_detail", true],
      ["/api/research/threads/delete", "api_research_thread_delete", true],
      ["/api/research/rebuild-index", "api_research_rebuild_index", true],
      ["/api/research/rebuild-embeddings", "api_research_rebuild_embeddings", true],
      ["/api/research/import-url", "api_research_import_url", true],
      ["/api/research/import-pdf", "api_research_import_pdf", true],
      ["/api/research/pack/export", "api_research_pack_export", true],
      ["/api/research/pack/import", "api_research_pack_import", true],
      ["/api/research/pack/rollback", "api_research_pack_rollback", false],
    ] as const;

    for (const [path, command, forwardsPayload] of routes) {
      const route = TAURI_POST_ROUTES[path];
      expect(route, `${path} should be registered`).toBeTypeOf("function");
      await route?.({
        invoke,
        path,
        parsed: new URL(`http://tauri.localhost${path}`),
        payload,
      });
      expect(invokeMock).toHaveBeenLastCalledWith(
        command,
        ...(forwardsPayload ? [{ payload }] : []),
      );
    }
  });

  it("forwards HK market scope for research reads", async () => {
    const { TAURI_GET_ROUTES } = await import("./tauri");
    const invokeMock = vi.fn(async (): Promise<unknown> => ({}));
    const invoke = invokeMock as InvokeFn;
    await TAURI_GET_ROUTES["/api/research/overview"]?.({
      invoke,
      path: "/api/research/overview",
      parsed: new URL("http://tauri.localhost/api/research/overview?market=hk&stock_code=00700.HK"),
    });
    await TAURI_GET_ROUTES["/api/research/messages"]?.({
      invoke,
      path: "/api/research/messages",
      parsed: new URL("http://tauri.localhost/api/research/messages?market=hk&stock_code=00700.HK&limit=120"),
    });
    await TAURI_GET_ROUTES["/api/research/threads"]?.({
      invoke,
      path: "/api/research/threads",
      parsed: new URL("http://tauri.localhost/api/research/threads?market=hk"),
    });

    expect(invokeMock.mock.calls).toEqual([
      ["api_research_overview", { payload: { market: "hk", stock_code: "00700.HK" } }],
      ["api_research_messages", { payload: { market: "hk", stock_code: "00700.HK", unread_only: false, limit: 120 } }],
      ["api_research_threads", { payload: { market: "hk" } }],
    ]);
  });

  it("uses the saved market proxy for native HK news refresh", async () => {
    vi.stubGlobal("window", { location: { href: "http://tauri.localhost/" }, __TAURI__: { core: { invoke: vi.fn() } }, localStorage: {
      getItem: (key: string) => key === "stock-optimizer-network-proxy-mode" ? "manual"
        : key === "stock-optimizer-network-proxy-url" ? "http://127.0.0.1:7890" : null,
    } });
    vi.stubGlobal("navigator", { userAgent: "Windows" });
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invokeMock = vi.fn(async (): Promise<unknown> => ({}));
    await TAURI_POST_ROUTES["/api/research/refresh"]?.({
      invoke: invokeMock as InvokeFn, path: "/api/research/refresh",
      parsed: new URL("http://tauri.localhost/api/research/refresh"),
      payload: { market: "hk", code: "", seed_codes: ["00700.HK"] },
    });
    expect(invokeMock).toHaveBeenCalledWith("api_research_refresh", { payload: {
      market: "hk", code: "", seed_codes: ["00700.HK"],
      network_proxy_mode: "manual", network_proxy_url: "http://127.0.0.1:7890",
    } });
  });

  it("reports a failed native source launch instead of falling back to a blocked WebView popup", async () => {
    const nativeInvoke = vi.fn(async () => { throw new Error("native browser launch failed"); });
    const popup = vi.fn();
    vi.stubGlobal("window", { __TAURI__: { core: { invoke: nativeInvoke } }, location: { href: "http://tauri.localhost/" }, open: popup });
    const { openExternalUrl } = await import("./tauri");
    const source = "https://t.cj.sina.cn/articles/view/1651428902/626ece2602001lifc";
    await expect(openExternalUrl(source)).rejects.toThrow("系统浏览器未能打开，请复制链接后打开。");
    expect(nativeInvoke).toHaveBeenCalledWith("open_external_url", { url: source });
    expect(popup).not.toHaveBeenCalled();
  });
});
