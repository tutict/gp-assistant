import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type InvokeFn = <T = unknown>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

describe("Agent Tauri routes", () => {
  beforeEach(() => {
    vi.stubGlobal("window", { location: { href: "http://tauri.localhost/" } });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("forwards model configuration and bounded history to the native harness", async () => {
    const { TAURI_POST_ROUTES } = await import("./tauri");
    const invokeMock = vi.fn(async (): Promise<unknown> => ({}));
    const invoke = invokeMock as InvokeFn;
    const payload = {
      message: "继续研究",
      run_id: "agent-run",
      mode: "research",
      llm: { base_url: "http://127.0.0.1:11434/v1", model: "qwen2.5:7b" },
      history: [{ role: "user", content: "先看商业模式" }],
      context: { watchlist: [{ code: "000001.SZ" }] },
    };

    await TAURI_POST_ROUTES["/api/agent/stream"]?.({
      invoke,
      path: "/api/agent/stream",
      parsed: new URL("http://tauri.localhost/api/agent/stream"),
      payload,
    });

    expect(invokeMock).toHaveBeenCalledWith("api_agent_stream", { payload });
  });

  it("uses one canonical field whitelist for native Agent calls", async () => {
    const { buildTauriAgentPayload } = await import("./tauri");
    expect(buildTauriAgentPayload({
      message: "test",
      run_id: "run-1",
      mode: "expert",
      llm: { model: "test-model" },
      ignored: "must not cross the bridge",
    })).toEqual({
      message: "test",
      run_id: "run-1",
      mode: "expert",
      context: undefined,
      platform: undefined,
      network: undefined,
      llm: { model: "test-model" },
      history: undefined,
    });
  });
});
