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

async function renderDrawer(
  overrides: Partial<React.ComponentProps<typeof AgentRunDrawer>> = {},
  options?: Parameters<typeof create>[1],
) {
  let renderer: ReactTestRenderer | undefined;
  await act(async () => {
    renderer = create(<AgentRunDrawer open={false} {...baseProps} {...overrides} />, options);
  });
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
    expect(agentRunMocks.listAgentRuns).toHaveBeenCalledWith({ conversationId: "conversation-1" });
    expect(renderedText(renderer)).toContain("Question current-run");

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();
    expect(agentRunMocks.listAgentRuns).toHaveBeenLastCalledWith({});
    expect(renderedText(renderer)).toContain("Question all-run");
  });

  it("does not widen a current scope without a conversation id", async () => {
    const renderer = await renderDrawer({ open: true, activeConversationId: undefined });
    await flush();

    expect(agentRunMocks.listAgentRuns).not.toHaveBeenCalled();
    expect(classNodes(renderer, "agent-run-state")).toHaveLength(1);
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

    expect(classNodes(renderer, "agent-run-state")).toHaveLength(1);
    await act(async () => {
      current.resolve([]);
      await Promise.resolve();
    });
    expect(classNodes(renderer, "agent-run-state")).toHaveLength(1);

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();
    expect(classNodes(renderer, "agent-run-state")).toHaveLength(1);

    await act(async () => {
      scopeButton(renderer, "all").props.onClick();
    });
    await flush();
    expect(classNodes(renderer, "agent-run-status-icon")).toHaveLength(4);
    expect(classNodes(renderer, "agent-run-status-label").map(nodeText)).toEqual(
      expect.arrayContaining([expect.any(String), expect.any(String), expect.any(String), expect.any(String)]),
    );
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
    expect(agentRunMocks.getAgentRun).toHaveBeenCalledWith("run-1");
    expect(renderedText(renderer)).toContain("Replay result is visible");
    expect(classNodes(renderer, "agent-run-overview-status")).toHaveLength(1);
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

    expect(output.indexOf("agent-run-overview")).toBeLessThan(output.indexOf("agent-run-timeline"));
    expect(output.indexOf("agent-run-timeline")).toBeLessThan(output.indexOf("agent-run-persisted-error"));
    expect(output.indexOf("agent-run-persisted-error")).toBeLessThan(output.indexOf("agent-run-result"));
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
