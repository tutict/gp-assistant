import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type InvokeFn = <T = unknown>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

describe("research Tauri routes", () => {
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
      ["/api/research/rebuild-index", "api_research_rebuild_index", false],
      ["/api/research/rebuild-embeddings", "api_research_rebuild_embeddings", false],
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
});
