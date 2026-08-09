import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentRunDrawerProps } from "./AgentRunDrawer";

const tauriMocks = vi.hoisted(() => ({
  buildTauriAgentPayload: vi.fn(),
  getTauriInvoke: vi.fn(),
  getTauriListen: vi.fn(),
  isTauriRuntime: vi.fn(),
}));

const drawerMock = vi.hoisted(() => ({
  failure: false,
  props: undefined as AgentRunDrawerProps | undefined,
}));

vi.mock("../../lib/tauri", () => tauriMocks);

vi.mock("./AgentRunDrawer", () => ({
  AgentRunDrawer: (props: AgentRunDrawerProps) => {
    drawerMock.props = props;
    if (!props.open) return null;
    return (
      <div className="agent-run-drawer-mock">
        {drawerMock.failure && <div role="alert">ledger replay failed</div>}
      </div>
    );
  },
}));

import { AgentPanel } from "./AgentPanel";

interface StoredMessage {
  role: "user" | "assistant";
  content: string;
  timestamp: number;
  runId?: string;
}

interface StoredConversation {
  id: string;
  title: string;
  mode: "quick";
  messages: StoredMessage[];
  createdAt: number;
  updatedAt: number;
}

const storage = new Map<string, string>();
const renderers = new Set<ReactTestRenderer>();
const onLlmSettingsChange = vi.fn();
const onWatchlistChange = vi.fn();
const watchlist = [{ code: "000001.SZ", name: "平安银行" }];
const unlistenMock = vi.fn();
const invokeMock = vi.fn();
let streamHandler: ((event: unknown) => void) | undefined;

const baseProps = {
  llmSettings: null,
  onLlmSettingsChange,
  watchlist,
  onWatchlistChange,
};

function conversation(
  id: string,
  title: string,
  messages: StoredMessage[] = [],
  updatedAt = 1,
): StoredConversation {
  return { id, title, mode: "quick", messages, createdAt: 1, updatedAt };
}

function seedConversations(conversations: StoredConversation[], activeConversationId = conversations[0]?.id || "") {
  storage.set("stock-optimizer-agent-conversations", JSON.stringify(conversations));
  storage.set("stock-optimizer-agent-active-conversation", activeConversationId);
}

async function renderPanel() {
  let renderer: ReactTestRenderer | undefined;
  await act(async () => {
    renderer = create(<AgentPanel {...baseProps} />);
  });
  renderers.add(renderer!);
  return renderer!;
}

function hasClass(node: ReactTestInstance, className: string) {
  return typeof node.props.className === "string" && node.props.className.split(" ").includes(className);
}

function buttonWithClass(renderer: ReactTestRenderer, className: string) {
  return renderer.root.find((node) => node.type === "button" && hasClass(node, className));
}

function buttonsWithClass(renderer: ReactTestRenderer, className: string) {
  return renderer.root.findAll((node) => node.type === "button" && hasClass(node, className));
}

function nodeText(node: ReactTestInstance): string {
  return node.children.map((child) => typeof child === "string" ? child : nodeText(child)).join("");
}

async function sendAndWait(renderer: ReactTestRenderer) {
  const textarea = renderer.root.findByType("textarea");
  await act(async () => {
    textarea.props.onChange({ target: { value: "run agent" } });
  });

  await act(async () => {
    await buttonWithClass(renderer, "send-btn").props.onClick();
  });
}

beforeEach(() => {
  storage.clear();
  streamHandler = undefined;
  drawerMock.failure = false;
  drawerMock.props = undefined;
  unlistenMock.mockReset();
  invokeMock.mockReset();
  onLlmSettingsChange.mockReset();
  onWatchlistChange.mockReset();
  tauriMocks.buildTauriAgentPayload.mockReset().mockImplementation((payload) => payload);
  tauriMocks.getTauriInvoke.mockReset().mockReturnValue(invokeMock);
  tauriMocks.getTauriListen.mockReset().mockReturnValue(vi.fn(async (
    _event: string,
    handler: (event: unknown) => void,
  ) => {
    streamHandler = handler;
    return unlistenMock;
  }));
  tauriMocks.isTauriRuntime.mockReset().mockReturnValue(true);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("crypto", { randomUUID: () => "run-generated" });
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  vi.stubGlobal("window", {
    location: { href: "http://localhost/" },
    matchMedia: () => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
    }),
  });
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

describe("AgentPanel run replay interactions", () => {
  it("opens run history without an initial run and passes the drawer contract", async () => {
    seedConversations([conversation("conversation-1", "Current conversation")]);
    const renderer = await renderPanel();
    const trigger = { focus: vi.fn() } as unknown as HTMLElement;

    expect(buttonsWithClass(renderer, "agent-thread-history")).toHaveLength(1);
    expect(buttonsWithClass(renderer, "agent-mobile-history")).toHaveLength(1);
    await act(async () => {
      buttonWithClass(renderer, "agent-thread-history").props.onClick({ currentTarget: trigger });
    });

    expect(drawerMock.props).toMatchObject({
      open: true,
      activeConversationId: "conversation-1",
      initialRunId: undefined,
      returnFocusElement: trigger,
      watchlist,
    });
    expect(drawerMock.props?.finishedRunId).toBeUndefined();
    expect(drawerMock.props?.onClose).toBeTypeOf("function");
    expect(drawerMock.props?.onToggleWatchlist).toBeTypeOf("function");
    const stage = renderer.root.find((node) => hasClass(node, "agent-chat-stage"));
    const finalChild = stage.children.at(-1) as ReactTestInstance;
    expect(finalChild.findAll((node) => hasClass(node, "agent-run-drawer-mock"))).toHaveLength(1);
  });

  it("opens the exact run from an assistant message", async () => {
    seedConversations([conversation("conversation-1", "Replay", [
      { role: "assistant", content: "completed", timestamp: 1, runId: "run-message-1" },
    ])]);
    const renderer = await renderPanel();
    const trigger = { focus: vi.fn() } as unknown as HTMLElement;

    await act(async () => {
      buttonWithClass(renderer, "agent-message-replay").props.onClick({ currentTarget: trigger });
    });

    expect(drawerMock.props).toMatchObject({
      open: true,
      activeConversationId: "conversation-1",
      initialRunId: "run-message-1",
      returnFocusElement: trigger,
    });
  });

  it("does not render replay controls for legacy assistant messages", async () => {
    seedConversations([conversation("conversation-1", "Legacy", [
      { role: "assistant", content: "legacy answer", timestamp: 1 },
    ])]);
    const renderer = await renderPanel();

    expect(buttonsWithClass(renderer, "agent-message-replay")).toHaveLength(0);
  });

  it("closes the drawer and clears direct replay when the conversation changes", async () => {
    seedConversations([
      conversation("conversation-1", "First", [
        { role: "assistant", content: "completed", timestamp: 1, runId: "run-message-1" },
      ], 2),
      conversation("conversation-2", "Second", [], 1),
    ], "conversation-1");
    const renderer = await renderPanel();

    await act(async () => {
      buttonWithClass(renderer, "agent-message-replay").props.onClick({ currentTarget: {} });
    });
    expect(drawerMock.props?.initialRunId).toBe("run-message-1");

    const secondConversation = renderer.root.findAll((node) => node.type === "button" && hasClass(node, "agent-history-main"))
      .find((node) => nodeText(node).includes("Second"));
    expect(secondConversation).toBeDefined();
    await act(async () => {
      secondConversation!.props.onClick();
      await Promise.resolve();
    });

    expect(drawerMock.props).toMatchObject({
      open: false,
      activeConversationId: "conversation-2",
      initialRunId: undefined,
    });
  });

  it.each([
    { type: "result", response: { reply: "done" } },
    { type: "error", message: "stream failed" },
  ])("publishes the active run after a terminal $type event", async (terminalEvent) => {
    seedConversations([conversation("conversation-1", "Terminal")]);
    tauriMocks.getTauriListen.mockReturnValueOnce(vi.fn(async (
      _event: string,
      handler: (event: unknown) => void,
    ) => {
      streamHandler = handler;
      handler({ payload: { run_id: "run-generated", ...terminalEvent } });
      return unlistenMock;
    }));
    invokeMock.mockResolvedValueOnce(undefined);
    const renderer = await renderPanel();
    await sendAndWait(renderer);

    expect(drawerMock.props?.finishedRunId).toBe("run-generated");
    expect(streamHandler).toBeTypeOf("function");
  });

  it("publishes the active run when the stream request throws", async () => {
    seedConversations([conversation("conversation-1", "Catch")]);
    invokeMock.mockRejectedValueOnce(new Error("invoke failed"));
    const renderer = await renderPanel();
    const textarea = renderer.root.findByType("textarea");
    await act(async () => {
      textarea.props.onChange({ target: { value: "run agent" } });
    });
    await act(async () => {
      await buttonWithClass(renderer, "send-btn").props.onClick();
    });

    expect(drawerMock.props?.finishedRunId).toBe("run-generated");
    const replies = renderer.root.findAll((node) => hasClass(node, "agent-final-reply")).map(nodeText);
    expect(replies).toContain("错误：invoke failed");
    expect(renderer.root.findByType("textarea").props.disabled).toBe(false);
  });

  it("keeps composer and messages independent from a drawer ledger failure", async () => {
    seedConversations([conversation("conversation-1", "Isolation", [
      { role: "assistant", content: "stable answer", timestamp: 1, runId: "run-stable" },
    ])]);
    drawerMock.failure = true;
    const renderer = await renderPanel();
    const textarea = renderer.root.findByType("textarea");
    await act(async () => {
      textarea.props.onChange({ target: { value: "draft question" } });
    });

    await act(async () => {
      buttonWithClass(renderer, "agent-thread-history").props.onClick({ currentTarget: {} });
    });

    expect(nodeText(renderer.root.findByProps({ role: "alert" }))).toContain("ledger replay failed");
    expect(renderer.root.findByType("textarea").props).toMatchObject({
      disabled: false,
      value: "draft question",
    });
    expect(buttonWithClass(renderer, "send-btn").props.disabled).toBe(false);
    expect(nodeText(renderer.root.find((node) => hasClass(node, "agent-final-reply")))).toBe("stable answer");
  });
});
