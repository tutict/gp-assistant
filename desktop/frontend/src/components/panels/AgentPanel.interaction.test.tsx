import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentRunDrawerProps } from "./AgentRunDrawer";

const tauriMocks = vi.hoisted(() => ({
  buildTauriAgentPayload: vi.fn(),
  getTauriInvoke: vi.fn(),
  getTauriListen: vi.fn(),
  isTauriRuntime: vi.fn(),
  postJson: vi.fn(),
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

import { AgentPanel, resetAgentConversationDeletionCoordinatorForTests } from "./AgentPanel";

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
const ledgerDeletionPrefix = "stock-optimizer-agent-failed-ledger-deletion:";
const renderers = new Set<ReactTestRenderer>();
const onLlmSettingsChange = vi.fn();
const onWatchlistChange = vi.fn();
const watchlist = [{ code: "000001.SZ", name: "平安银行" }];
const unlistenMock = vi.fn();
const invokeMock = vi.fn();
const storageHandlers = new Set<(event: { key: string | null }) => void>();
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

function deferred<T>() {
  let resolve: (value: T) => void;
  let reject: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, reject: reject!, resolve: resolve! };
}

function localStorageDouble(shouldThrow?: (key: string) => boolean) {
  return {
    get length() {
      return storage.size;
    },
    key: (index: number) => Array.from(storage.keys())[index] ?? null,
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => {
      if (shouldThrow?.(key)) throw new Error("quota exceeded");
      storage.set(key, value);
    },
    removeItem: (key: string) => storage.delete(key),
  };
}

function storedLedgerDeletionIds() {
  return Array.from(storage.keys())
    .filter((key) => key.startsWith(ledgerDeletionPrefix) && storage.get(key) === "pending")
    .map((key) => decodeURIComponent(key.slice(ledgerDeletionPrefix.length)))
    .sort();
}

async function beginDeferredSend(renderer: ReactTestRenderer, request: Promise<unknown>) {
  invokeMock.mockReturnValueOnce(request);
  const textarea = renderer.root.findByType("textarea");
  await act(async () => {
    textarea.props.onChange({ target: { value: "run agent" } });
  });

  let sendPromise: Promise<void> | undefined;
  act(() => {
    sendPromise = buttonWithClass(renderer, "send-btn").props.onClick();
  });
  await Promise.resolve();
  expect(streamHandler).toBeTypeOf("function");
  expect(invokeMock).toHaveBeenCalledTimes(1);
  return { sendPromise: sendPromise! };
}

beforeEach(() => {
  resetAgentConversationDeletionCoordinatorForTests();
  storage.clear();
  storageHandlers.clear();
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
  tauriMocks.postJson.mockReset().mockResolvedValue({ deleted: 0 });
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("crypto", { randomUUID: () => "run-generated" });
  vi.stubGlobal("localStorage", localStorageDouble());
  vi.stubGlobal("window", {
    location: { href: "http://localhost/" },
    addEventListener: (type: string, handler: (event: { key: string | null }) => void) => {
      if (type === "storage") storageHandlers.add(handler);
    },
    removeEventListener: (type: string, handler: (event: { key: string | null }) => void) => {
      if (type === "storage") storageHandlers.delete(handler);
    },
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

    const desktopHistory = buttonWithClass(renderer, "agent-thread-history");
    const mobileHistory = buttonWithClass(renderer, "agent-mobile-history");
    expect(desktopHistory.props).toMatchObject({ "aria-label": "运行复盘", title: "运行复盘" });
    expect(nodeText(desktopHistory)).toBe("运行复盘");
    expect(mobileHistory.props).toMatchObject({ "aria-label": "运行历史", title: "运行历史" });
    await act(async () => {
      desktopHistory.props.onClick({ currentTarget: trigger });
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
    const replayButton = buttonWithClass(renderer, "agent-message-replay");

    expect(replayButton.props).toMatchObject({
      "aria-label": "查看本次运行复盘",
      title: "查看本次运行复盘",
    });

    await act(async () => {
      replayButton.props.onClick({ currentTarget: trigger });
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
      returnFocusElement: null,
    });
  });

  it("removes local history immediately and invalidates replay after ledger cleanup", async () => {
    seedConversations([
      conversation("conversation-delete", "Delete me", [], 2),
      conversation("conversation-keep", "Keep me", [], 1),
    ]);
    const cleanup = deferred<unknown>();
    tauriMocks.postJson.mockReturnValueOnce(cleanup.promise);
    const renderer = await renderPanel();
    const removeButton = buttonsWithClass(renderer, "agent-history-remove")
      .find((button) => nodeText(button.parent as ReactTestInstance).includes("Delete me"));
    expect(removeButton).toBeDefined();

    act(() => {
      removeButton!.props.onClick({ stopPropagation: vi.fn() });
    });

    expect(tauriMocks.postJson).toHaveBeenCalledWith("/api/agent/runs/delete-conversation", {
      conversation_id: "conversation-delete",
    }, { timeoutMs: 10_000 });
    expect(drawerMock.props?.ledgerRevision).toBe(0);
    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-keep" })]);
    expect(storedLedgerDeletionIds()).toEqual(["conversation-delete"]);

    await act(async () => {
      cleanup.resolve({ deleted: 1 });
      await cleanup.promise;
    });
    expect(drawerMock.props?.ledgerRevision).toBe(1);
    expect(storedLedgerDeletionIds()).toEqual([]);
    expect(storage.get(`${ledgerDeletionPrefix}${encodeURIComponent("conversation-delete")}`))
      .toBe("completed");
  });

  it("keeps normal deletion terminal state across unmount and remount", async () => {
    seedConversations([
      conversation("conversation-delete", "Delete me", [], 2),
      conversation("conversation-keep", "Keep me", [], 1),
    ]);
    const cleanup = deferred<unknown>();
    tauriMocks.postJson.mockReturnValueOnce(cleanup.promise);
    const renderer = await renderPanel();

    act(() => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
      renderer.unmount();
      renderers.delete(renderer);
    });
    const remounted = await renderPanel();
    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-keep" })]);
    expect(storedLedgerDeletionIds()).toEqual(["conversation-delete"]);

    await act(async () => {
      cleanup.resolve({ deleted: 1 });
      await cleanup.promise;
    });

    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-keep" })]);
    expect(storedLedgerDeletionIds()).toEqual([]);
    expect(nodeText(remounted.root)).not.toContain("Delete me");
  });

  it("does not touch the ledger when a durable deletion marker cannot be stored", async () => {
    seedConversations([conversation("conversation-delete", "Delete me")]);
    const renderer = await renderPanel();
    vi.stubGlobal("localStorage", localStorageDouble((key) => key.startsWith(ledgerDeletionPrefix)));

    act(() => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
    });

    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-delete" })]);
    expect(nodeText(renderer.root.findByProps({ role: "alert" }))).toContain("对话未删除");
    expect(buttonsWithClass(renderer, "agent-history-remove")[0].props.disabled).toBe(false);

    act(() => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
    });
    expect(tauriMocks.postJson).not.toHaveBeenCalled();
  });

  it("recovers on retry after a one-time tombstone quota failure", async () => {
    seedConversations([conversation("conversation-delete", "Delete me")]);
    const cleanup = deferred<unknown>();
    tauriMocks.postJson.mockReturnValueOnce(cleanup.promise);
    const renderer = await renderPanel();
    let tombstoneWrites = 0;
    vi.stubGlobal("localStorage", localStorageDouble((key) => (
      key.startsWith(ledgerDeletionPrefix) && tombstoneWrites++ === 0
    )));

    await act(async () => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
      await Promise.resolve();
    });
    expect(tauriMocks.postJson).not.toHaveBeenCalled();
    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-delete" })]);

    act(() => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
      renderer.unmount();
      renderers.delete(renderer);
    });
    const remounted = await renderPanel();
    expect(nodeText(remounted.root)).not.toContain("Delete me");
    expect(tauriMocks.postJson).toHaveBeenCalledTimes(1);

    await act(async () => {
      cleanup.resolve({ deleted: 1 });
      await cleanup.promise;
    });

    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .not.toEqual([expect.objectContaining({ id: "conversation-delete" })]);
    expect(remounted.root.findByType("textarea").props.disabled).toBe(false);
  });

  it("keeps the conversation when deletion preparation fails", async () => {
    seedConversations([conversation("conversation-delete", "Delete me")]);
    const renderer = await renderPanel();
    vi.stubGlobal("localStorage", localStorageDouble((key) => key.startsWith(ledgerDeletionPrefix)));

    await act(async () => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
      await Promise.resolve();
    });

    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-delete" })]);
    expect(nodeText(renderer.root.findByProps({ role: "alert" }))).toContain("对话未删除");
    expect(drawerMock.props?.ledgerRevision).toBe(0);
    expect(tauriMocks.postJson).not.toHaveBeenCalled();
  });

  it("does not start concurrent ledger cleanup without durable tombstones", async () => {
    seedConversations([
      conversation("conversation-a", "Conversation A", [], 2),
      conversation("conversation-b", "Conversation B", [], 1),
    ]);
    const renderer = await renderPanel();
    vi.stubGlobal("localStorage", localStorageDouble((key) => key.startsWith(ledgerDeletionPrefix)));
    const removeButtons = buttonsWithClass(renderer, "agent-history-remove");
    const removeA = removeButtons.find((button) => nodeText(button.parent as ReactTestInstance).includes("Conversation A"));
    const removeB = removeButtons.find((button) => nodeText(button.parent as ReactTestInstance).includes("Conversation B"));

    act(() => {
      removeA!.props.onClick({ stopPropagation: vi.fn() });
      removeB!.props.onClick({ stopPropagation: vi.fn() });
    });

    expect(nodeText(renderer.root.findByProps({ role: "alert" }))).toContain("对话未删除");
    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([
        expect.objectContaining({ id: "conversation-a" }),
        expect.objectContaining({ id: "conversation-b" }),
      ]);
    expect(tauriMocks.postJson).not.toHaveBeenCalled();
  });

  it("keeps concurrent deletion tombstones independent", async () => {
    seedConversations([
      conversation("conversation-a", "Conversation A", [], 2),
      conversation("conversation-b", "Conversation B", [], 1),
    ]);
    const cleanupA = deferred<unknown>();
    const cleanupB = deferred<unknown>();
    tauriMocks.postJson
      .mockReturnValueOnce(cleanupA.promise)
      .mockReturnValueOnce(cleanupB.promise);
    const renderer = await renderPanel();
    const removeButtons = buttonsWithClass(renderer, "agent-history-remove");
    const removeA = removeButtons.find((button) => nodeText(button.parent as ReactTestInstance).includes("Conversation A"));
    const removeB = removeButtons.find((button) => nodeText(button.parent as ReactTestInstance).includes("Conversation B"));

    act(() => {
      removeA!.props.onClick({ stopPropagation: vi.fn() });
      removeB!.props.onClick({ stopPropagation: vi.fn() });
    });
    expect(storedLedgerDeletionIds()).toEqual(["conversation-a", "conversation-b"]);

    await act(async () => {
      cleanupA.reject(new Error("ledger unavailable"));
      await Promise.resolve();
    });
    await act(async () => {
      cleanupB.resolve({ deleted: 1 });
      await cleanupB.promise;
    });

    expect(storedLedgerDeletionIds()).toEqual(["conversation-a"]);
    expect(nodeText(renderer.root.findByProps({ role: "alert" }))).toContain("1 个已删除对话");
  });

  it("applies a completed deletion marker from another browser context", async () => {
    seedConversations([
      conversation("conversation-a", "Conversation A", [], 2),
      conversation("conversation-b", "Conversation B", [], 1),
    ]);
    const renderer = await renderPanel();
    const tombstoneKey = `${ledgerDeletionPrefix}${encodeURIComponent("conversation-a")}`;
    storage.set(tombstoneKey, "completed");
    const textarea = renderer.root.findByType("textarea");
    await act(async () => {
      textarea.props.onChange({ target: { value: "must not run" } });
    });
    await act(async () => {
      buttonWithClass(renderer, "send-btn").props.onClick();
      await Promise.resolve();
    });
    expect(invokeMock).not.toHaveBeenCalled();

    await act(async () => {
      for (const handler of storageHandlers) handler({ key: tombstoneKey });
      await Promise.resolve();
    });

    expect(nodeText(renderer.root)).not.toContain("Conversation A");
    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-b" })]);
  });

  it("keeps failed ledger cleanup retryable without blocking local conversation deletion", async () => {
    seedConversations([conversation("conversation-delete", "Delete me")]);
    tauriMocks.postJson
      .mockRejectedValueOnce(new Error("ledger unavailable"))
      .mockResolvedValueOnce({ deleted: 1 });
    const renderer = await renderPanel();

    await act(async () => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
      await Promise.resolve();
    });

    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .not.toEqual([expect.objectContaining({ id: "conversation-delete" })]);
    expect(storedLedgerDeletionIds()).toEqual(["conversation-delete"]);
    expect(nodeText(renderer.root.findByProps({ role: "alert" }))).toContain("运行记录仍待清理");
    expect(drawerMock.props?.ledgerRevision).toBe(0);

    await act(async () => {
      renderer.root.findByProps({ "aria-label": "重试清理运行记录" }).props.onClick();
      await Promise.resolve();
    });

    expect(tauriMocks.postJson).toHaveBeenCalledTimes(2);
    expect(storedLedgerDeletionIds()).toEqual([]);
    expect(drawerMock.props?.ledgerRevision).toBe(1);
  });

  it("uses the same best-effort ledger cleanup in browser mode", async () => {
    tauriMocks.isTauriRuntime.mockReturnValue(false);
    seedConversations([
      conversation("conversation-delete", "Delete me", [], 2),
      conversation("conversation-keep", "Keep me", [], 1),
    ]);
    const renderer = await renderPanel();

    await act(async () => {
      buttonsWithClass(renderer, "agent-history-remove")[0].props.onClick({ stopPropagation: vi.fn() });
      await Promise.resolve();
    });

    expect(tauriMocks.postJson).toHaveBeenCalledWith("/api/agent/runs/delete-conversation", {
      conversation_id: "conversation-delete",
    }, { timeoutMs: 10_000 });
    expect(JSON.parse(storage.get("stock-optimizer-agent-conversations") || "[]"))
      .toEqual([expect.objectContaining({ id: "conversation-keep" })]);
  });

  it("disables conversation deletion while an Agent run is active", async () => {
    seedConversations([conversation("conversation-running", "Running")]);
    const request = deferred<unknown>();
    const renderer = await renderPanel();
    const { sendPromise } = await beginDeferredSend(renderer, request.promise);

    expect(buttonsWithClass(renderer, "agent-history-remove")[0].props.disabled).toBe(true);
    expect(tauriMocks.postJson).not.toHaveBeenCalled();

    await act(async () => {
      request.resolve(undefined);
      await sendPromise;
    });
  });

  it("waits for the stream invoke to resolve before publishing a result completion", async () => {
    seedConversations([conversation("conversation-1", "Terminal")]);
    const request = deferred<unknown>();
    const renderer = await renderPanel();
    const { sendPromise } = await beginDeferredSend(renderer, request.promise);

    await act(async () => {
      streamHandler!({ payload: { run_id: "run-generated", type: "result", response: { reply: "done" } } });
    });

    expect(drawerMock.props?.finishedRunId).toBeUndefined();
    const repliesBeforePersistence = renderer.root.findAll((node) => hasClass(node, "agent-final-reply")).map(nodeText);
    expect(repliesBeforePersistence).toContain("done");
    await act(async () => {
      request.resolve(undefined);
      await sendPromise;
    });
    expect(drawerMock.props?.finishedRunId).toBe("run-generated");
  });

  it("publishes completion only after an early error event reaches the catch path", async () => {
    seedConversations([conversation("conversation-1", "Terminal error")]);
    const request = deferred<unknown>();
    const renderer = await renderPanel();
    const { sendPromise } = await beginDeferredSend(renderer, request.promise);

    await act(async () => {
      streamHandler!({ payload: { run_id: "run-generated", type: "error", message: "stream failed" } });
    });

    expect(drawerMock.props?.finishedRunId).toBeUndefined();
    const repliesBeforePersistence = renderer.root.findAll((node) => hasClass(node, "agent-final-reply")).map(nodeText);
    expect(repliesBeforePersistence).toContain("stream failed");
    await act(async () => {
      request.reject(new Error("invoke failed"));
      await sendPromise;
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
