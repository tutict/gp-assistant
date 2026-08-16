import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

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
const renderers = new Set<ReactTestRenderer>();

function stubTestGlobals() {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
}

beforeAll(async () => {
  stubTestGlobals();
  try {
    ({ AgentPanel, sanitizeAgentConversations } = await import("./AgentPanel"));
  } finally {
    vi.unstubAllGlobals();
  }
});

beforeEach(() => {
  stubTestGlobals();
  tauriMocks.getTauriInvoke.mockReset();
  tauriMocks.getTauriListen.mockReset();
  tauriMocks.isTauriRuntime.mockReset();
  baseProps.onLlmSettingsChange.mockReset();
  baseProps.onWatchlistChange.mockReset();
});

afterEach(async () => {
  try {
    await act(async () => {
      for (const renderer of renderers) renderer.unmount();
    });
  } finally {
    renderers.clear();
    vi.unstubAllGlobals();
  }
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

  it("omits blank and overlong UTF-8 run IDs but retains exactly 256 bytes", () => {
    const exactRunId = "x".repeat(256);
    const sanitized = sanitizeAgentConversations([{
      id: "conversation-3",
      title: "history",
      mode: "quick",
      messages: [
        { role: "assistant", content: "blank", timestamp: 1, runId: "   " },
        { role: "assistant", content: "overlong", timestamp: 2, runId: "x".repeat(257) },
        { role: "assistant", content: "exact", timestamp: 3, runId: exactRunId },
        { role: "assistant", content: "multibyte", timestamp: 4, runId: "中".repeat(100) },
      ],
      createdAt: 1,
      updatedAt: 3,
    }]);

    expect(sanitized[0].messages[0]).not.toHaveProperty("runId");
    expect(sanitized[0].messages[1]).not.toHaveProperty("runId");
    expect(sanitized[0].messages[2]).toHaveProperty("runId", exactRunId);
    expect(sanitized[0].messages[3]).not.toHaveProperty("runId");
  });

  it("replaces restored conversation IDs outside the UTF-8 boundary", () => {
    const sanitized = sanitizeAgentConversations([{
      id: "中".repeat(100),
      title: "history",
      mode: "quick",
      messages: [],
      createdAt: 1,
      updatedAt: 1,
    }]);

    expect(new TextEncoder().encode(sanitized[0].id).byteLength).toBeLessThanOrEqual(256);
    expect(sanitized[0].id).not.toBe("中".repeat(100));
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
    expect(html).toContain('aria-label="运行历史"');
    expect(html).toContain('class="icon-button agent-thread-history"');
    expect(html).toContain('class="icon-button agent-mobile-history"');
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
    let resolveInvoke: ((value: { reply: string }) => void) | undefined;
    const invokeResult = new Promise<{ reply: string }>((resolve) => {
      resolveInvoke = resolve;
    });
    let payloadRunIdAtInvoke = "";
    let payloadHadRunIdBeforeResolve = false;
    const invoke = vi.fn().mockImplementation((command: string, args: { payload: { run_id?: unknown } }) => {
      expect(command).toBe("api_agent_stream");
      payloadRunIdAtInvoke = String(args.payload.run_id || "");
      payloadHadRunIdBeforeResolve = payloadRunIdAtInvoke === "run-send";
      const persistedAtInvoke = JSON.parse(storage.get("stock-optimizer-agent-conversations") || "null");
      const assistantMessageAtInvoke = persistedAtInvoke[0].messages.find((message: { role: string }) => message.role === "assistant");
      expect(assistantMessageAtInvoke).toMatchObject({ role: "assistant", runId: "run-send" });
      expect(assistantMessageAtInvoke).not.toHaveProperty("result");
      expect(assistantMessageAtInvoke).not.toHaveProperty("steps");
      return invokeResult;
    });
    tauriMocks.isTauriRuntime.mockReturnValue(true);
    tauriMocks.getTauriListen.mockReturnValue(listen);
    tauriMocks.getTauriInvoke.mockReturnValue(invoke);

    let renderer: ReturnType<typeof create> | undefined;
    await act(async () => {
      renderer = create(<AgentPanel {...baseProps} llmSettings={null} />);
    });
    renderers.add(renderer!);

    const textarea = renderer!.root.findByType("textarea");
    await act(async () => {
      textarea.props.onChange({ target: { value: "send this" } });
    });
    expect(textarea.props.value).toBe("send this");

    const sendButton = renderer!.root.find((node) => node.type === "button" && node.props.className === "send-btn");
    let sendPromise: Promise<void> | undefined;
    await act(async () => {
      sendPromise = sendButton.props.onClick();
      await Promise.resolve();
    });

    expect(tauriMocks.isTauriRuntime).toHaveBeenCalled();
    expect(tauriMocks.getTauriInvoke).toHaveBeenCalled();
    expect(tauriMocks.getTauriListen).toHaveBeenCalled();
    expect(listen).toHaveBeenCalledWith("agent-stream-event", expect.any(Function));
    expect(payloadRunIdAtInvoke).toBe("run-send");
    expect(payloadHadRunIdBeforeResolve).toBe(true);

    await act(async () => {
      resolveInvoke!({ reply: "finished" });
      await sendPromise;
    });

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

  });
});
