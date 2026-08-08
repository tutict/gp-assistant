import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

let AgentPanel: typeof import("./AgentPanel").AgentPanel;
let sanitizeAgentConversations: typeof import("./AgentPanel").sanitizeAgentConversations;

beforeAll(async () => {
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
