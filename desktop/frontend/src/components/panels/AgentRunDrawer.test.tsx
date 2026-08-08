import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentRunDetail, AgentRunSummary } from "../../lib/agentRuns";
import type { AgentStreamEvent } from "../../types";

const agentRunMocks = vi.hoisted(() => ({
  getAgentRun: vi.fn(),
  listAgentRuns: vi.fn(),
}));

vi.mock("../../lib/agentRuns", () => agentRunMocks);

let AgentRunDrawer: typeof import("./AgentRunDrawer").AgentRunDrawer;
let buildAgentRunTimeline: typeof import("./AgentRunDrawer").buildAgentRunTimeline;

const baseProps = {
  activeConversationId: "conversation-1",
  onClose: vi.fn(),
  onToggleWatchlist: vi.fn(),
  watchlist: [],
};

function summary(runId: string, overrides: Partial<AgentRunSummary> = {}): AgentRunSummary {
  return {
    runId,
    question: `Question ${runId}`,
    mode: "quick",
    status: "completed",
    startedAtEpochMs: 1_700_000_000_000,
    durationMs: 2_500,
    ...overrides,
  };
}

function detail(runId: string, overrides: Partial<AgentRunDetail> = {}): AgentRunDetail {
  return {
    ...summary(runId),
    events: [],
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void;
  let reject: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, reject: reject!, resolve: resolve! };
}

async function renderDrawer(overrides: Partial<React.ComponentProps<typeof AgentRunDrawer>> = {}) {
  let renderer: ReactTestRenderer | undefined;
  await act(async () => {
    renderer = create(<AgentRunDrawer open={false} {...baseProps} {...overrides} />);
  });
  return renderer!;
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function button(renderer: ReactTestRenderer, label: string) {
  const match = renderer.root.findAll((node) => (
    node.type === "button" && nodeText(node).includes(label)
  ))[0];
  if (!match) throw new Error(`Missing button: ${label}`);
  return match;
}

function nodeText(node: ReactTestInstance): string {
  return node.children.map((child) => typeof child === "string" ? child : nodeText(child)).join("");
}

function renderedText(renderer: ReactTestRenderer) {
  return renderer.toJSON() ? JSON.stringify(renderer.toJSON()) : "";
}

beforeAll(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ AgentRunDrawer, buildAgentRunTimeline } = await import("./AgentRunDrawer"));
});

beforeEach(() => {
  agentRunMocks.getAgentRun.mockReset();
  agentRunMocks.listAgentRuns.mockReset();
  baseProps.onClose.mockReset();
  baseProps.onToggleWatchlist.mockReset();
});

afterAll(() => {
  vi.unstubAllGlobals();
});

describe("AgentRunDrawer list", () => {
  it("loads the current conversation on open and loads all runs when selected", async () => {
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([summary("current-run")])
      .mockResolvedValueOnce([summary("all-run")]);
    const renderer = await renderDrawer();

    expect(agentRunMocks.listAgentRuns).not.toHaveBeenCalled();

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} />);
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledWith({ conversationId: "conversation-1" });
    expect(renderedText(renderer)).toContain("Question current-run");

    await act(async () => {
      button(renderer, "全部运行").props.onClick();
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenLastCalledWith({});
    expect(renderedText(renderer)).toContain("Question all-run");
  });

  it("does not widen a current scope without a conversation id", async () => {
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined });
    await flush();

    expect(agentRunMocks.listAgentRuns).not.toHaveBeenCalled();
    expect(renderedText(renderer)).toContain("当前会话暂无运行记录");
  });

  it("does not let an older list response replace a newer scope", async () => {
    const current = deferred<AgentRunSummary[]>();
    const all = deferred<AgentRunSummary[]>();
    agentRunMocks.listAgentRuns
      .mockReturnValueOnce(current.promise)
      .mockReturnValueOnce(all.promise);
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      button(renderer, "全部运行").props.onClick();
    });
    expect(agentRunMocks.listAgentRuns).toHaveBeenLastCalledWith({});

    await act(async () => {
      all.resolve([summary("new-all")]);
      await Promise.resolve();
    });
    expect(renderedText(renderer)).toContain("Question new-all");

    await act(async () => {
      current.resolve([summary("stale-current")]);
      await Promise.resolve();
    });
    expect(renderedText(renderer)).not.toContain("Question stale-current");
  });
});

describe("AgentRunDrawer detail", () => {
  it("opens a supplied initial run directly and renders its normalized result", async () => {
    agentRunMocks.getAgentRun.mockResolvedValue(detail("run-1", {
      result: { reply: "Replay result is visible" },
    }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-1" });
    await flush();

    expect(agentRunMocks.listAgentRuns).not.toHaveBeenCalled();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledWith("run-1");
    expect(renderedText(renderer)).toContain("Replay result is visible");
  });

  it("shows a missing ledger record and retries failed detail requests", async () => {
    agentRunMocks.getAgentRun.mockResolvedValueOnce(null);
    const missingRenderer = await renderDrawer({ open: true, initialRunId: "missing-run" });
    await flush();
    expect(renderedText(missingRenderer)).toContain("本次运行未成功留痕");

    agentRunMocks.getAgentRun
      .mockRejectedValueOnce(new Error("ledger unavailable"))
      .mockResolvedValueOnce(detail("retry-run"));
    const retryRenderer = await renderDrawer({ open: true, initialRunId: "retry-run" });
    await flush();
    expect(renderedText(retryRenderer)).toContain("ledger unavailable");

    await act(async () => {
      button(retryRenderer, "重试").props.onClick();
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenLastCalledWith("retry-run");
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(3);
  });

  it("refreshes a running selected detail exactly once when it finishes", async () => {
    agentRunMocks.getAgentRun
      .mockResolvedValueOnce(detail("run-live", { status: "running" }))
      .mockResolvedValueOnce(detail("run-live", { status: "completed" }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-live" });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(1);

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-live" finishedRunId="run-live" />);
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-live" finishedRunId="run-live" />);
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
  });

  it("keeps the loaded list when returning from a selected detail", async () => {
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([summary("list-run")]);
    agentRunMocks.getAgentRun.mockResolvedValueOnce(detail("list-run"));
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      button(renderer, "Question list-run").props.onClick();
    });
    await flush();
    expect(renderedText(renderer)).toContain("返回运行列表");

    await act(async () => {
      button(renderer, "返回运行列表").props.onClick();
    });
    expect(renderedText(renderer)).toContain("Question list-run");
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(1);
  });
});

describe("buildAgentRunTimeline", () => {
  it("normalizes direct event types, bounds display text, and omits result events", () => {
    const events: AgentStreamEvent[] = [
      { type: "status", label: "Preparing", stage: "plan" },
      { type: "tool_start", action: "fallback", payload: { label: "Screen candidates", tool: "screen" } },
      { type: "tool_result", payload: { output_summary: "Found 3 candidates", status: "ok" } },
      { type: "evidence", label: "Do not use this as the title" },
      { type: "final", label: "Do not use this as the title" },
      { type: "error", message: "Provider failed" },
      { type: "result", message: "skip this" },
      { type: "x".repeat(800), label: "ignored" },
    ];

    const timeline = buildAgentRunTimeline(events);

    expect(timeline.map((item) => item.label)).toEqual([
      "Preparing",
      "Screen candidates",
      "Found 3 candidates",
      "证据整理",
      "最终答复",
      "Provider failed",
      "x".repeat(500),
    ]);
    expect(timeline[0].detail).toBe("plan");
    expect(timeline.at(-1)?.type).toHaveLength(500);
    expect(timeline.some((item) => item.label === "skip this")).toBe(false);
    for (const item of timeline) {
      expect(item.label.length).toBeLessThanOrEqual(500);
      expect(item.detail?.length ?? 0).toBeLessThanOrEqual(500);
    }
  });
});

describe("AgentRunDrawer accessibility", () => {
  it("exposes an accessible dialog and closes on Escape", async () => {
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined });
    const dialog = renderer.root.findByProps({ role: "dialog" });
    const preventDefault = vi.fn();

    expect(dialog.props["aria-modal"]).toBe(true);
    expect(dialog.props["aria-label"]).toBe("Agent 运行复盘");

    await act(async () => {
      dialog.props.onKeyDown({ key: "Escape", preventDefault });
    });
    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(baseProps.onClose).toHaveBeenCalledTimes(1);
  });
});
