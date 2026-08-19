import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentRunDetail, AgentRunMetrics, AgentRunSummary } from "../../lib/agentRuns";
import type { AgentStreamEvent } from "../../types";

const agentRunMocks = vi.hoisted(() => ({
  getAgentRun: vi.fn(),
  getAgentRunMetrics: vi.fn(),
  listAgentRuns: vi.fn(),
}));

vi.mock("../../lib/agentRuns", () => agentRunMocks);

let AgentRunDrawer: typeof import("./AgentRunDrawer").AgentRunDrawer;
let buildAgentRunTimeline: typeof import("./AgentRunDrawer").buildAgentRunTimeline;
const renderers = new Set<ReactTestRenderer>();

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
  return { ...summary(runId), events: [], ...overrides };
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

function abortError() {
  return Object.assign(new Error("aborted"), { name: "AbortError" });
}

function focusHarness() {
  const body = { name: "body" };
  let activeElement: unknown = body;
  const focusTarget = (name: string) => {
    const target = {
      name,
      focus: vi.fn(() => {
        activeElement = target;
      }),
      getAttribute: () => null,
    } as unknown as HTMLElement;
    return target;
  };
  const back = focusTarget("back");
  const close = focusTarget("close");
  const list = focusTarget("list");
  const drawer = {
    contains: (node: unknown) => [back, close, list].includes(node as HTMLElement),
    querySelector: vi.fn((selector: string) => {
      if (selector === ".agent-run-drawer-back") return back;
      if (selector === ".agent-run-scope-option[aria-pressed='true']") return list;
      return null;
    }),
  };
  vi.stubGlobal("document", {
    get activeElement() {
      return activeElement;
    },
  });
  return {
    back,
    body,
    close,
    drawer,
    list,
    get activeElement() {
      return activeElement;
    },
    createNodeMock: (element: React.ReactElement) => {
      const props = element.props as { className?: unknown };
      if (element.type === "aside" && props.className === "agent-run-drawer") return drawer;
      if (element.type === "div" && props.className === "agent-run-drawer-close-control") {
        return { querySelector: () => close };
      }
      return null;
    },
  };
}

async function renderDrawer(
  overrides: Partial<React.ComponentProps<typeof AgentRunDrawer>> = {},
  options?: Parameters<typeof create>[1],
) {
  let renderer: ReactTestRenderer | undefined;
  await act(async () => {
    renderer = create(<AgentRunDrawer open={false} {...baseProps} {...overrides} />, options);
  });
  renderers.add(renderer!);
  return renderer!;
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function nodeText(node: ReactTestInstance): string {
  return node.children.map((child) => typeof child === "string" ? child : nodeText(child)).join("");
}

function renderedText(renderer: ReactTestRenderer) {
  return renderer.toJSON() ? JSON.stringify(renderer.toJSON()) : "";
}

function classNodes(renderer: ReactTestRenderer, className: string) {
  return renderer.root.findAll((node) => typeof node.type === "string" && node.props.className === className);
}

function nodesWithClass(renderer: ReactTestRenderer, className: string) {
  return renderer.root.findAll((node) => (
    typeof node.type === "string"
    && typeof node.props.className === "string"
    && node.props.className.split(" ").includes(className)
  ));
}

function buttonWithText(renderer: ReactTestRenderer, text: string) {
  const match = renderer.root.findAll((node) => node.type === "button" && nodeText(node).includes(text))[0];
  if (!match) throw new Error(`Missing button containing ${text}`);
  return match;
}

function scopeButton(renderer: ReactTestRenderer, scope: "current" | "all") {
  return classNodes(renderer, "agent-run-scope-option")[scope === "current" ? 0 : 1];
}

function retryButton(renderer: ReactTestRenderer) {
  const match = classNodes(renderer, "agent-run-retry")[0];
  if (!match) throw new Error("Missing retry button");
  return match;
}

function backButton(renderer: ReactTestRenderer) {
  const match = nodesWithClass(renderer, "agent-run-drawer-back")[0];
  if (!match) throw new Error("Missing persistent back control");
  return match;
}

function stubTestGlobals() {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
}

beforeAll(async () => {
  stubTestGlobals();
  try {
    ({ AgentRunDrawer, buildAgentRunTimeline } = await import("./AgentRunDrawer"));
  } finally {
    vi.unstubAllGlobals();
  }
});

beforeEach(() => {
  stubTestGlobals();
  agentRunMocks.getAgentRun.mockReset();
  agentRunMocks.getAgentRunMetrics.mockReset().mockResolvedValue(null);
  agentRunMocks.listAgentRuns.mockReset();
  baseProps.onClose.mockReset();
  baseProps.onToggleWatchlist.mockReset();
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

describe("AgentRunDrawer list", () => {
  it("renders scope-specific quality metrics without leaking the other scope", async () => {
    const quality = (conversationId: string, sampleSize: number): AgentRunMetrics => ({
      schemaVersion: 1,
      sampleSize,
      sampleLimit: 200,
      conversationId,
      statusCounts: { completed: sampleSize, failed: 0 },
      profileCounts: {
        hot_money_early_v1: { count: sampleSize, completed: sampleSize, failed: 0, modelUsed: sampleSize, fallback: 0 },
      },
      modelOutcomeCounts: { model_success: sampleSize },
      apiFormatCounts: { openai_chat: sampleSize },
      durationMs: { count: sampleSize, averageMs: 100, p50Ms: 100, p95Ms: 100, maxMs: 100 },
    });
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([summary("current-run")])
      .mockResolvedValueOnce([summary("all-run")]);
    agentRunMocks.getAgentRunMetrics
      .mockResolvedValueOnce(quality("conversation-1", 1))
      .mockResolvedValueOnce(quality("", 2));

    const renderer = await renderDrawer({ open: true });
    await flush();

    expect(agentRunMocks.getAgentRunMetrics).toHaveBeenNthCalledWith(1, {
      conversationId: "conversation-1",
      signal: expect.any(AbortSignal),
      limit: 200,
    });
    expect(nodeText(classNodes(renderer, "agent-run-metrics")[0])).toContain("样本1");
    expect(classNodes(renderer, "agent-run-metrics")[0].props["aria-label"]).toBe("运行质量概览");

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();

    expect(agentRunMocks.getAgentRunMetrics).toHaveBeenLastCalledWith({
      signal: expect.any(AbortSignal),
      limit: 200,
    });
    expect(nodeText(classNodes(renderer, "agent-run-metrics")[0])).toContain("样本2");
    expect(nodeText(classNodes(renderer, "agent-run-metrics")[0])).not.toContain("样本1");
  });

  it("does not request while closed, then loads current and all scopes", async () => {
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([summary("current-run")])
      .mockResolvedValueOnce([summary("all-run")]);
    const renderer = await renderDrawer();

    expect(agentRunMocks.listAgentRuns).not.toHaveBeenCalled();
    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} />);
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      signal: expect.any(AbortSignal),
    });
    expect(renderedText(renderer)).toContain("Question current-run");

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenLastCalledWith({ signal: expect.any(AbortSignal) });
    expect(renderedText(renderer)).toContain("Question all-run");
  });

  it("invalidates the active scope cache after ledger deletion", async () => {
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([summary("stale-current")])
      .mockResolvedValueOnce([summary("fresh-current")]);
    const renderer = await renderDrawer({ open: true, ledgerRevision: 0 });
    await flush();
    expect(renderedText(renderer)).toContain("Question stale-current");

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} ledgerRevision={1} />);
    });
    await flush();

    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
    expect(renderedText(renderer)).not.toContain("Question stale-current");
    expect(renderedText(renderer)).toContain("Question fresh-current");
  });

  it("renders unknown instead of throwing for timestamps outside the Date range", async () => {
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([
      summary("out-of-range-time", { startedAtEpochMs: Number.MAX_SAFE_INTEGER }),
    ]);
    const renderer = await renderDrawer({ open: true });
    await flush();

    const timestamps = renderer.root.findAll((node) => node.type === "time");
    expect(timestamps).toHaveLength(1);
    expect(nodeText(timestamps[0])).toBe("未知");
  });

  it("renders unknown for a zero timestamp", async () => {
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([
      summary("zero-time", { startedAtEpochMs: 0 }),
    ]);
    const renderer = await renderDrawer({ open: true });
    await flush();

    const timestamps = renderer.root.findAll((node) => node.type === "time");
    expect(timestamps).toHaveLength(1);
    expect(nodeText(timestamps[0])).toBe("未知");
  });

  it("does not widen a current scope without a conversation id", async () => {
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined });
    await flush();

    expect(agentRunMocks.listAgentRuns).not.toHaveBeenCalled();
    expect(nodeText(classNodes(renderer, "agent-run-state")[0])).toBe("当前会话暂无运行记录");
  });

  it("does not let an older list response replace a newer scope", async () => {
    const current = deferred<AgentRunSummary[]>();
    const all = deferred<AgentRunSummary[]>();
    agentRunMocks.listAgentRuns
      .mockReturnValueOnce(current.promise)
      .mockReturnValueOnce(all.promise);
    const renderer = await renderDrawer({ open: true });

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
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

  it("passes signals and aborts list requests on scope changes, close, and unmount", async () => {
    agentRunMocks.listAgentRuns.mockImplementation(() => new Promise(() => undefined));
    const renderer = await renderDrawer({ open: true });
    const currentSignal = agentRunMocks.listAgentRuns.mock.calls[0][0].signal as AbortSignal;

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    const allSignal = agentRunMocks.listAgentRuns.mock.calls[1][0].signal as AbortSignal;
    expect(currentSignal.aborted).toBe(true);
    expect(allSignal.aborted).toBe(false);

    await act(async () => {
      renderer.update(<AgentRunDrawer open={false} {...baseProps} />);
    });
    expect(allSignal.aborted).toBe(true);

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} />);
    });
    const reopenedSignal = agentRunMocks.listAgentRuns.mock.calls[2][0].signal as AbortSignal;
    await act(async () => {
      renderer.unmount();
    });
    renderers.delete(renderer);
    expect(reopenedSignal.aborted).toBe(true);
  });

  it("does not render an AbortError as a list failure", async () => {
    agentRunMocks.listAgentRuns.mockRejectedValueOnce(abortError());
    const renderer = await renderDrawer({ open: true });
    await flush();

    expect(renderer.root.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(renderedText(renderer)).not.toContain("aborted");
  });

  it("retries a failed list request and bounds its visible error", async () => {
    const longError = "l".repeat(2_001);
    agentRunMocks.listAgentRuns
      .mockRejectedValueOnce(new Error(longError))
      .mockResolvedValueOnce([summary("recovered-list")]);
    const renderer = await renderDrawer({ open: true });
    await flush();

    expect(renderedText(renderer)).toContain("l".repeat(2_000));
    expect(renderedText(renderer)).not.toContain(longError);
    await act(async () => {
      retryButton(renderer).props.onClick();
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
    expect(renderedText(renderer)).toContain("Question recovered-list");
  });

  it("shows loading and empty list states before icon and duration status summaries", async () => {
    const current = deferred<AgentRunSummary[]>();
    agentRunMocks.listAgentRuns
      .mockReturnValueOnce(current.promise)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        summary("running", { status: "running" }),
        summary("completed", { status: "completed" }),
        summary("failed", { status: "failed" }),
        summary("unknown", { status: "unknown" }),
      ]);
    const renderer = await renderDrawer({ open: true });

    expect(nodeText(classNodes(renderer, "agent-run-state")[0])).toBe("正在加载运行记录");
    expect(classNodes(renderer, "agent-run-state")[0].props.role).toBe("status");
    await act(async () => {
      current.resolve([]);
      await Promise.resolve();
    });
    expect(nodeText(classNodes(renderer, "agent-run-state")[0])).toBe("当前会话暂无运行记录");
    expect(classNodes(renderer, "agent-run-state")[0].props.role).toBe("status");

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();
    expect(nodeText(classNodes(renderer, "agent-run-state")[0])).toBe("暂无运行记录");

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();
    expect(classNodes(renderer, "agent-run-status-icon")).toHaveLength(4);
    expect(classNodes(renderer, "agent-run-status-label").map(nodeText)).toEqual([
      "运行中", "已完成", "失败", "状态未知",
    ]);
    expect(classNodes(renderer, "agent-run-status").map((node) => node.props["data-status"])).toEqual([
      "running", "completed", "failed", "unknown",
    ]);
    expect(classNodes(renderer, "agent-run-duration").map(nodeText)).toEqual([
      expect.stringContaining("2.5"),
      expect.stringContaining("2.5"),
      expect.stringContaining("2.5"),
      expect.stringContaining("2.5"),
    ]);
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
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledWith("run-1", expect.any(AbortSignal));
    expect(renderedText(renderer)).toContain("Replay result is visible");
    expect(classNodes(renderer, "agent-run-overview-status")).toHaveLength(1);
  });

  it("renders the persisted final reply before the replay result details", async () => {
    agentRunMocks.getAgentRun.mockResolvedValue(detail("run-reply", {
      result: {
        action: "screen",
        reply: "Persisted final conclusion",
        data: { rows: [{ code: "000001.SZ", name: "Candidate" }] },
      },
    }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-reply" });
    await flush();

    const reply = classNodes(renderer, "agent-final-reply agent-run-final-reply");
    expect(reply).toHaveLength(1);
    expect(nodeText(reply[0])).toBe("Persisted final conclusion");
  });

  it("retries a failed detail request and bounds its visible error", async () => {
    const longError = "d".repeat(2_001);
    agentRunMocks.getAgentRun
      .mockRejectedValueOnce(new Error(longError))
      .mockResolvedValueOnce(detail("retry-run"));
    const renderer = await renderDrawer({ open: true, initialRunId: "retry-run" });
    await flush();

    expect(renderedText(renderer)).toContain("d".repeat(2_000));
    expect(renderedText(renderer)).not.toContain(longError);
    await act(async () => {
      retryButton(renderer).props.onClick();
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
    expect(renderedText(renderer)).toContain("Question retry-run");
  });

  it("refreshes a running selected detail exactly once when it finishes", async () => {
    agentRunMocks.getAgentRun
      .mockResolvedValueOnce(detail("run-live", { status: "running" }))
      .mockResolvedValueOnce(detail("run-live", { status: "completed" }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-live" });
    await flush();

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-live" finishedRunId="run-live" />);
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
    expect(classNodes(renderer, "agent-run-overview-status")[0].props["data-status"]).toBe("completed");

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-live" finishedRunId="run-live" />);
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
  });

  it("does not let a closed run completion replace a newly opened direct detail", async () => {
    agentRunMocks.getAgentRun
      .mockResolvedValueOnce(detail("run-a", { status: "running" }))
      .mockResolvedValueOnce(detail("run-b", { status: "completed" }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-a" });
    await flush();

    await act(async () => {
      renderer.update(<AgentRunDrawer open={false} {...baseProps} initialRunId="run-a" />);
    });
    await flush();

    await act(async () => {
      renderer.update(
        <AgentRunDrawer
          open
          {...baseProps}
          initialRunId="run-b"
          finishedRunId="run-a"
          finishedRunConversationId="conversation-1"
        />,
      );
    });
    await flush();

    expect(agentRunMocks.getAgentRun).toHaveBeenNthCalledWith(1, "run-a", expect.any(AbortSignal));
    expect(agentRunMocks.getAgentRun).toHaveBeenNthCalledWith(2, "run-b", expect.any(AbortSignal));
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
    expect(renderedText(renderer)).toContain("Question run-b");
  });

  it("refreshes a missing selected detail exactly once when its completion signal arrives", async () => {
    agentRunMocks.getAgentRun
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(detail("run-late", { status: "completed" }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-late" });
    await flush();
    expect(renderedText(renderer)).toContain("本次运行未成功留痕");

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-late" finishedRunId="run-late" />);
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
    expect(classNodes(renderer, "agent-run-overview-status")[0].props["data-status"]).toBe("completed");

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-late" finishedRunId="run-late" />);
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
  });

  it("refreshes a visible running list item when its completion signal arrives", async () => {
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([summary("run-list-live", { status: "running" })])
      .mockResolvedValueOnce([summary("run-list-live", { status: "completed" })]);
    const renderer = await renderDrawer({ open: true });
    await flush();
    expect(classNodes(renderer, "agent-run-status")[0].props["data-status"]).toBe("running");

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} finishedRunId="run-list-live" />);
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
    expect(classNodes(renderer, "agent-run-status")[0].props["data-status"]).toBe("completed");

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} finishedRunId="run-list-live" />);
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
  });

  it("refreshes once when completion arrives before the running list response", async () => {
    const staleList = deferred<AgentRunSummary[]>();
    agentRunMocks.listAgentRuns
      .mockReturnValueOnce(staleList.promise)
      .mockResolvedValueOnce([summary("run-list-race", { status: "completed" })]);
    const renderer = await renderDrawer({ open: true });

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} finishedRunId="run-list-race" />);
    });
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(1);

    await act(async () => {
      staleList.resolve([summary("run-list-race", { status: "running" })]);
      await staleList.promise;
    });
    await flush();

    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
    expect(classNodes(renderer, "agent-run-status")[0].props["data-status"]).toBe("completed");
  });

  it("refreshes once when a completed run was absent from the first list", async () => {
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([summary("run-list-late", { status: "completed" })]);
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} finishedRunId="run-list-late" />);
    });
    await flush();

    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
    expect(renderedText(renderer)).toContain("Question run-list-late");
  });

  it("does not refresh a current-conversation list for another conversation's completion", async () => {
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([]);
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      renderer.update(
        <AgentRunDrawer
          open
          {...baseProps}
          finishedRunId="run-other-conversation"
          finishedRunConversationId="conversation-2"
        />,
      );
    });
    await flush();

    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(1);
  });

  it("allows an explicit retry after a missing detail", async () => {
    agentRunMocks.getAgentRun
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(detail("manual-retry"));
    const renderer = await renderDrawer({ open: true, initialRunId: "manual-retry" });
    await flush();

    expect(retryButton(renderer)).toBeDefined();
    expect(nodeText(renderer.root.findByProps({ role: "status" }))).toBe("本次运行未成功留痕");
    await act(async () => {
      retryButton(renderer).props.onClick();
    });
    await flush();
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(2);
    expect(renderedText(renderer)).toContain("Question manual-retry");
  });

  it("passes signals and aborts detail requests on run changes, close, and unmount", async () => {
    agentRunMocks.getAgentRun.mockImplementation(() => new Promise(() => undefined));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-a" });
    const runASignal = agentRunMocks.getAgentRun.mock.calls[0][1] as AbortSignal;

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-b" />);
    });
    const runBSignal = agentRunMocks.getAgentRun.mock.calls[1][1] as AbortSignal;
    expect(runASignal.aborted).toBe(true);
    expect(runBSignal.aborted).toBe(false);

    await act(async () => {
      renderer.update(<AgentRunDrawer open={false} {...baseProps} />);
    });
    expect(runBSignal.aborted).toBe(true);

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-c" />);
    });
    const runCSignal = agentRunMocks.getAgentRun.mock.calls[2][1] as AbortSignal;
    await act(async () => {
      renderer.unmount();
    });
    renderers.delete(renderer);
    expect(runCSignal.aborted).toBe(true);
  });

  it("does not render an AbortError as a detail failure", async () => {
    agentRunMocks.getAgentRun.mockRejectedValueOnce(abortError());
    const renderer = await renderDrawer({ open: true, initialRunId: "aborted-detail" });
    await flush();

    expect(renderer.root.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(renderedText(renderer)).not.toContain("aborted");
  });

  it("updates cached list summaries after a running detail reaches a terminal state", async () => {
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([summary("cached-live", { status: "running" })]);
    agentRunMocks.getAgentRun
      .mockResolvedValueOnce(detail("cached-live", { status: "running" }))
      .mockResolvedValueOnce(detail("cached-live", {
        status: "completed",
        completedAtEpochMs: 1_700_000_005_000,
        durationMs: 5_000,
      }));
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      buttonWithText(renderer, "Question cached-live").props.onClick();
    });
    await flush();
    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} finishedRunId="cached-live" />);
    });
    await flush();
    await act(async () => {
      backButton(renderer).props.onClick();
    });

    expect(classNodes(renderer, "agent-run-status")[0].props["data-status"]).toBe("completed");
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(1);
  });

  it("returns to the current list when an open drawer clears its initial run", async () => {
    agentRunMocks.getAgentRun.mockResolvedValueOnce(detail("direct-run"));
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([summary("current-run")]);
    const renderer = await renderDrawer({ open: true, initialRunId: "direct-run" });
    await flush();

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} />);
    });
    await flush();

    expect(nodesWithClass(renderer, "agent-run-drawer-back")).toHaveLength(0);
    expect(renderedText(renderer)).toContain("Question current-run");
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      signal: expect.any(AbortSignal),
    });
  });

  it("retains list summaries when returning from a normal selected detail", async () => {
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([summary("list-run")]);
    agentRunMocks.getAgentRun.mockResolvedValueOnce(detail("list-run"));
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      buttonWithText(renderer, "Question list-run").props.onClick();
    });
    await flush();
    await act(async () => {
      backButton(renderer).props.onClick();
    });
    expect(renderedText(renderer)).toContain("Question list-run");
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(1);
  });

  it("retains all-run summaries when returning from loading, missing, and failed detail states", async () => {
    const loadingDetail = deferred<AgentRunDetail | null>();
    agentRunMocks.listAgentRuns
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([summary("all-recovery")]);
    agentRunMocks.getAgentRun
      .mockReturnValueOnce(loadingDetail.promise)
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(new Error("detail failed"));
    const renderer = await renderDrawer({ open: true });
    await flush();

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();

    for (const expectedCall of [1, 2, 3]) {
      await act(async () => {
        buttonWithText(renderer, "Question all-recovery").props.onClick();
      });
      if (expectedCall > 1) await flush();
      expect(nodesWithClass(renderer, "agent-run-drawer-back")).toHaveLength(1);
      if (expectedCall === 1) {
        expect(nodeText(classNodes(renderer, "agent-run-state")[0])).toBe("正在加载运行详情");
      }
      if (expectedCall === 2) {
        expect(nodeText(classNodes(renderer, "agent-run-state")[0])).toContain("本次运行未成功留痕");
      }
      await act(async () => {
        backButton(renderer).props.onClick();
      });
      expect(renderedText(renderer)).toContain("Question all-recovery");
    }
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledTimes(3);
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledTimes(2);
  });

  it("does not let a stale detail response overwrite a newly selected run", async () => {
    const first = deferred<AgentRunDetail | null>();
    agentRunMocks.getAgentRun
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(detail("run-b", { question: "Question run-b" }));
    const renderer = await renderDrawer({ open: true, initialRunId: "run-a" });

    await act(async () => {
      renderer.update(<AgentRunDrawer open {...baseProps} initialRunId="run-b" />);
    });
    await flush();
    expect(renderedText(renderer)).toContain("Question run-b");

    await act(async () => {
      first.resolve(detail("run-a", { question: "Question stale-run-a" }));
      await Promise.resolve();
    });
    expect(renderedText(renderer)).not.toContain("Question stale-run-a");
  });

  it("renders detail sections in overview, timeline, persisted-error, result order", async () => {
    agentRunMocks.getAgentRun.mockResolvedValue(detail("ordered", {
      error: "persisted failure",
      events: [{ type: "status", label: "Started" }],
      result: { reply: "ordered result" },
    }));
    const renderer = await renderDrawer({ open: true, initialRunId: "ordered" });
    await flush();
    const output = renderedText(renderer);
    const detailBody = classNodes(renderer, "agent-run-detail")[0];

    expect((detailBody.children[0] as ReactTestInstance).props.className).toBe("agent-run-overview");
    expect(nodesWithClass(renderer, "agent-run-back")).toHaveLength(0);
    expect(nodesWithClass(renderer, "agent-run-drawer-back")).toHaveLength(1);
    expect(output.indexOf("agent-run-overview")).toBeLessThan(output.indexOf("agent-run-timeline"));
    expect(output.indexOf("agent-run-timeline")).toBeLessThan(output.indexOf("agent-run-persisted-error"));
    expect(output.indexOf("agent-run-persisted-error")).toBeLessThan(output.indexOf("agent-run-result"));
  });

  it("renders an unavailable result status without hiding the rest of the detail", async () => {
    agentRunMocks.getAgentRun.mockResolvedValue(detail("unavailable", {
      error: "persisted detail remains visible",
      events: [{ type: "status", label: "timeline remains visible" }],
      resultUnavailable: true,
    }));
    const renderer = await renderDrawer({ open: true, initialRunId: "unavailable" });
    await flush();
    const output = renderedText(renderer);
    const unavailableStatus = renderer.root.findByProps({ role: "status" });

    expect(nodeText(unavailableStatus)).toBe("结果不可用");
    expect(output).toContain("Question unavailable");
    expect(output).toContain("timeline remains visible");
    expect(output).toContain("persisted detail remains visible");
    expect(output.indexOf("agent-run-overview")).toBeLessThan(output.indexOf("agent-run-timeline"));
    expect(output.indexOf("agent-run-timeline")).toBeLessThan(output.indexOf("agent-run-persisted-error"));
    expect(output.indexOf("agent-run-persisted-error")).toBeLessThan(output.indexOf("agent-run-result"));
  });

  it("does not describe a missing result as unavailable", async () => {
    agentRunMocks.getAgentRun.mockResolvedValue(detail("missing-result", {
      events: [{ type: "status", label: "Still running" }],
    }));
    const renderer = await renderDrawer({ open: true, initialRunId: "missing-result" });
    await flush();

    expect(renderedText(renderer)).not.toContain("结果不可用");
    expect(renderer.root.findAllByProps({ role: "status" })).toHaveLength(0);
  });

  it("does not render direct request or configuration values in drawer-owned timeline and error markup", async () => {
    const secret = "do-not-render-this-api-key";
    agentRunMocks.getAgentRun.mockResolvedValue({
      ...detail("direct-detail", {
        status: "failed",
        error: "safe persisted error",
        events: [{
          type: "error",
          message: "safe event error",
          payload: {
            request: { api_key: secret },
            config: { token: secret },
            api_key: secret,
          },
        }],
      }),
      request: { api_key: secret },
      config: { token: secret },
    });
    const renderer = await renderDrawer({ open: true, initialRunId: "direct-detail" });
    await flush();
    const output = renderedText(renderer);

    expect(output).toContain("safe event error");
    expect(output).toContain("safe persisted error");
    expect(output).not.toContain(secret);
    expect(output).not.toContain("api_key");
  });
});

describe("buildAgentRunTimeline", () => {
  it("normalizes direct events, bounds display text, omits results, and keeps unknown types neutral", () => {
    const events: AgentStreamEvent[] = [
      { type: "status", label: "Preparing", stage: "plan" },
      { type: "tool_start", action: "fallback", payload: { label: "Screen candidates", tool: "screen" } },
      { type: "tool_result", payload: { output_summary: "Found 3 candidates", status: "ok" } },
      { type: "evidence" },
      { type: "final" },
      { type: "error", message: "Provider failed" },
      { type: "result", message: "skip this" },
      { type: "x".repeat(800), label: "ignored" },
    ];
    const timeline = buildAgentRunTimeline(events);

    expect(timeline).toHaveLength(7);
    expect(timeline[0]).toMatchObject({ label: "Preparing", detail: "plan" });
    expect(timeline[1]).toMatchObject({ label: "Screen candidates", tone: "active" });
    expect(timeline[2]).toMatchObject({ label: "Found 3 candidates", tone: "success" });
    expect(timeline.some((item) => item.label === "skip this")).toBe(false);
    expect(timeline.at(-1)).toMatchObject({ type: "x".repeat(500), tone: "neutral" });
    for (const item of timeline) {
      expect(item.label.length).toBeLessThanOrEqual(500);
      expect(item.detail?.length ?? 0).toBeLessThanOrEqual(500);
    }
  });

  it("does not expose raw payload objects or request-like keys", () => {
    const secret = "secret-marker";
    const timeline = buildAgentRunTimeline([{
      type: "tool_start",
      payload: {
        label: "Safe tool label",
        request: { api_key: secret },
        config: { authorization: secret },
        api_key: secret,
      },
    }]);

    expect(timeline).toEqual([expect.objectContaining({ label: "Safe tool label", tone: "active" })]);
    expect(JSON.stringify(timeline)).not.toContain(secret);
    expect(JSON.stringify(timeline)).not.toContain("api_key");
  });
});

describe("AgentRunDrawer accessibility", () => {
  it("exposes dialog metadata and closes on Escape", async () => {
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined });
    const dialog = renderer.root.findByProps({ role: "dialog" });
    const preventDefault = vi.fn();

    expect(dialog.props["aria-modal"]).toBe(true);
    expect(typeof dialog.props["aria-label"]).toBe("string");
    await act(async () => {
      dialog.props.onKeyDown({ key: "Escape", preventDefault });
    });
    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(baseProps.onClose).toHaveBeenCalledTimes(1);
  });

  it("focuses the close button on open and restores supplied focus after close", async () => {
    const closeFocus = vi.fn();
    const returnFocus = { focus: vi.fn() } as unknown as HTMLElement;
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined, returnFocusElement: returnFocus }, {
      createNodeMock: (element) => {
        const props = element.props as { className?: unknown };
        return element.type === "div" && props.className === "agent-run-drawer-close-control"
          ? { querySelector: () => ({ focus: closeFocus }) }
          : null;
      },
    });

    expect(closeFocus).toHaveBeenCalledTimes(1);
    await act(async () => {
      renderer.update(<AgentRunDrawer open={false} {...baseProps} returnFocusElement={returnFocus} />);
    });
    expect(returnFocus.focus).toHaveBeenCalledTimes(1);
  });

  it("keeps focus in the drawer across select and back transitions so Escape still closes", async () => {
    const focus = focusHarness();
    agentRunMocks.listAgentRuns.mockResolvedValueOnce([summary("focus-run")]);
    agentRunMocks.getAgentRun.mockResolvedValueOnce(detail("focus-run"));
    const renderer = await renderDrawer({ open: true }, { createNodeMock: focus.createNodeMock });
    await flush();
    expect(focus.activeElement).toBe(focus.close);

    await act(async () => {
      buttonWithText(renderer, "Question focus-run").props.onClick();
    });
    await flush();
    expect(focus.activeElement).toBe(focus.back);
    expect(focus.drawer.contains(focus.activeElement)).toBe(true);

    await act(async () => {
      backButton(renderer).props.onClick();
    });
    expect(focus.activeElement).toBe(focus.list);
    expect(focus.drawer.contains(focus.activeElement)).toBe(true);

    const dialog = renderer.root.findByProps({ role: "dialog" });
    await act(async () => {
      dialog.props.onKeyDown({ key: "Escape", preventDefault: vi.fn() });
    });
    expect(baseProps.onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps focus in the drawer after list and detail retries", async () => {
    const listFocus = focusHarness();
    agentRunMocks.listAgentRuns
      .mockRejectedValueOnce(new Error("list failed"))
      .mockResolvedValueOnce([]);
    const listRenderer = await renderDrawer({ open: true }, { createNodeMock: listFocus.createNodeMock });
    await flush();
    await act(async () => {
      retryButton(listRenderer).props.onClick();
    });
    expect(listFocus.activeElement).toBe(listFocus.list);
    expect(listFocus.drawer.contains(listFocus.activeElement)).toBe(true);

    const detailFocus = focusHarness();
    agentRunMocks.getAgentRun
      .mockRejectedValueOnce(new Error("detail failed"))
      .mockResolvedValueOnce(detail("retry-focus"));
    const detailRenderer = await renderDrawer(
      { open: true, initialRunId: "retry-focus" },
      { createNodeMock: detailFocus.createNodeMock },
    );
    await flush();
    await act(async () => {
      retryButton(detailRenderer).props.onClick();
    });
    expect(detailFocus.activeElement).toBe(detailFocus.back);
    expect(detailFocus.drawer.contains(detailFocus.activeElement)).toBe(true);

    const dialog = detailRenderer.root.findByProps({ role: "dialog" });
    await act(async () => {
      dialog.props.onKeyDown({ key: "Escape", preventDefault: vi.fn() });
    });
    expect(baseProps.onClose).toHaveBeenCalledTimes(1);
  });

  it("wraps Tab and Shift+Tab between only drawer focusable nodes", async () => {
    const first = { focus: vi.fn(), getAttribute: () => null } as unknown as HTMLElement;
    const last = { focus: vi.fn(), getAttribute: () => null } as unknown as HTMLElement;
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined });
    const dialog = renderer.root.findByProps({ role: "dialog" });
    const currentTarget = { querySelectorAll: vi.fn(() => [first, last]) } as unknown as HTMLElement;
    const preventDefault = vi.fn();
    vi.stubGlobal("document", { activeElement: last });

    await act(async () => {
      dialog.props.onKeyDown({ key: "Tab", currentTarget, preventDefault, shiftKey: false });
    });
    expect(preventDefault).toHaveBeenCalledTimes(1);
    expect(first.focus).toHaveBeenCalledTimes(1);

    vi.stubGlobal("document", { activeElement: first });
    await act(async () => {
      dialog.props.onKeyDown({ key: "Tab", currentTarget, preventDefault, shiftKey: true });
    });
    expect(preventDefault).toHaveBeenCalledTimes(2);
    expect(last.focus).toHaveBeenCalledTimes(1);
    expect(currentTarget.querySelectorAll).toHaveBeenCalledTimes(2);
  });
});
