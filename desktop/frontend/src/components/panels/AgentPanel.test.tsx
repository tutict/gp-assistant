import { renderToStaticMarkup } from "react-dom/server";
import { act, create } from "react-test-renderer";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getTauriInvoke: vi.fn(),
  getTauriListen: vi.fn(),
  isTauriRuntime: vi.fn(),
}));

vi.mock("../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/tauri")>()),
  getTauriInvoke: tauriMocks.getTauriInvoke,
  getTauriListen: tauriMocks.getTauriListen,
  isTauriRuntime: tauriMocks.isTauriRuntime,
}));

let AgentPanel: typeof import("./AgentPanel").AgentPanel;
let sanitizeAgentConversations: typeof import("./AgentPanel").sanitizeAgentConversations;

beforeAll(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ AgentPanel, sanitizeAgentConversations } = await import("./AgentPanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

const baseProps = {
  onLlmSettingsChange: vi.fn(),
  watchlist: [],
  onWatchlistChange: vi.fn(),
};

describe("sanitizeAgentConversations", () => {
  it("preserves assistant run IDs while removing result and steps", () => {
    const sanitized = sanitizeAgentConversations([{
      id: "conversation-1",
      title: "history",
      mode: "quick",
      messages: [{
        role: "assistant",
        content: "done",
        timestamp: 1,
        runId: "  run-123  ",
        result: { reply: "large result" },
        steps: [{ stage: "final", label: "done", percent: 100 }],
      }],
      createdAt: 1,
      updatedAt: 1,
    }]);

    expect(sanitized[0].messages[0]).toMatchObject({
      role: "assistant",
      content: "done",
      runId: "run-123",
    });
    expect(sanitized[0].messages[0]).not.toHaveProperty("result");
    expect(sanitized[0].messages[0]).not.toHaveProperty("steps");
  });

  it("keeps historical messages without a run ID valid", () => {
    const sanitized = sanitizeAgentConversations([{
      id: "conversation-2",
      title: "history",
      mode: "quick",
      messages: [
        { role: "user", content: "old question", timestamp: 1 },
        { role: "assistant", content: "old answer", timestamp: 2 },
      ],
      createdAt: 1,
      updatedAt: 2,
    }]);

    expect(sanitized[0].messages).toEqual([
      { role: "user", content: "old question", timestamp: 1, error: false },
      { role: "assistant", content: "old answer", timestamp: 2, error: false },
    ]);
  });

  it("omits blank and overlong run IDs but retains exactly 256 characters", () => {
    const exactRunId = "x".repeat(256);
    const sanitized = sanitizeAgentConversations([{
      id: "conversation-3",
      title: "history",
      mode: "quick",
      messages: [
        { role: "assistant", content: "blank", timestamp: 1, runId: "   " },
        { role: "assistant", content: "overlong", timestamp: 2, runId: "x".repeat(257) },
        { role: "assistant", content: "exact", timestamp: 3, runId: exactRunId },
      ],
      createdAt: 1,
      updatedAt: 3,
    }]);

    expect(sanitized[0].messages[0]).not.toHaveProperty("runId");
    expect(sanitized[0].messages[1]).not.toHaveProperty("runId");
    expect(sanitized[0].messages[2]).toHaveProperty("runId", exactRunId);
  });
});

describe("AgentPanel empty state", () => {
  it("renders a compact mode selector and model setup hint", () => {
    const html = renderToStaticMarkup(<AgentPanel {...baseProps} llmSettings={null} />);

    expect(html).toContain("<h2>开始对话</h2>");
    expect(html).toContain('role="group"');
    expect(html).toContain('maxLength="8000"');
    expect(html).toContain('aria-pressed="true"');
    expect(html).toContain("请先配置模型");
    expect(html).toContain("游资早期框架：环境、主线、情绪周期与失效条件");
    expect(html).toContain("价值复利框架：企业质量、资本配置与估值");
    expect(html).not.toContain("快速模式能力");
  });

  it("hides the setup hint when a model is configured", () => {
    const html = renderToStaticMarkup(
      <AgentPanel
        {...baseProps}
        llmSettings={{
          active_provider_id: "test",
          providers: [{ id: "test", name: "Test", provider: "openai-compatible", model: "test-model", api_key: "test-key" }],
        }}
      />,
    );

    expect(html).not.toContain("请先配置模型");
  });
});

describe("AgentPanel send run ID", () => {
  it("sends and persists the generated run ID in the lightweight transcript", async () => {
    const storage = new Map<string, string>([
      ["stock-optimizer-agent-conversations", JSON.stringify([{
        id: "conversation-interaction",
        title: "history",
        mode: "quick",
        messages: [],
        createdAt: 1,
        updatedAt: 1,
      }])],
      ["stock-optimizer-agent-active-conversation", "conversation-interaction"],
    ]);
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    });
    vi.stubGlobal("crypto", { randomUUID: () => "run-send" });

    let streamHandler: ((event: unknown) => void) | undefined;
    const unlisten = vi.fn();
    const listen = vi.fn(async (_event: string, handler: (event: unknown) => void) => {
      streamHandler = handler;
      return unlisten;
    });
    const invoke = vi.fn().mockImplementation((command: string) => {
      expect(command).toBe("api_agent_stream");
      const persistedAtInvoke = JSON.parse(storage.get("stock-optimizer-agent-conversations") || "null");
      const assistantMessageAtInvoke = persistedAtInvoke[0].messages.find((message: { role: string }) => message.role === "assistant");
      expect(assistantMessageAtInvoke).toMatchObject({ role: "assistant", runId: "run-send" });
      expect(assistantMessageAtInvoke).not.toHaveProperty("result");
      expect(assistantMessageAtInvoke).not.toHaveProperty("steps");
      return Promise.resolve({ reply: "finished" });
    });
    tauriMocks.isTauriRuntime.mockReturnValue(true);
    tauriMocks.getTauriListen.mockReturnValue(listen);
    tauriMocks.getTauriInvoke.mockReturnValue(invoke);

    let renderer: ReturnType<typeof create> | undefined;
    await act(async () => {
      renderer = create(<AgentPanel {...baseProps} llmSettings={null} />);
    });

    const textarea = renderer!.root.findByType("textarea");
    await act(async () => {
      textarea.props.onChange({ target: { value: "send this" } });
    });
    expect(textarea.props.value).toBe("send this");

    const sendButton = renderer!.root.find((node) => node.type === "button" && node.props.className === "send-btn");
    await act(async () => {
      await sendButton.props.onClick();
    });

    expect(tauriMocks.isTauriRuntime).toHaveBeenCalled();
    expect(tauriMocks.getTauriInvoke).toHaveBeenCalled();
    expect(tauriMocks.getTauriListen).toHaveBeenCalled();
    expect(listen).toHaveBeenCalledWith("agent-stream-event", expect.any(Function));
    expect(invoke).toHaveBeenCalledWith("api_agent_stream", {
      payload: expect.objectContaining({ run_id: "run-send" }),
    });
    expect(streamHandler).toBeTypeOf("function");
    expect(unlisten).toHaveBeenCalledTimes(1);

    const persisted = JSON.parse(storage.get("stock-optimizer-agent-conversations") || "null");
    const assistantMessage = persisted[0].messages.find((message: { role: string }) => message.role === "assistant");
    expect(assistantMessage).toMatchObject({ role: "assistant", runId: "run-send" });
    expect(assistantMessage).not.toHaveProperty("result");
    expect(assistantMessage).not.toHaveProperty("steps");

    renderer!.unmount();
  });
});
