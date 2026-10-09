import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WatchlistItem } from "../types";

const native = vi.hoisted(() => ({ invoke: vi.fn(), available: true }));
vi.mock("./tauri", () => ({ getTauriInvoke: () => native.invoke, isTauriRuntime: () => native.available }));
const a: WatchlistItem = { code: "000001.SZ", name: "A", added_at: "2026-01-01" };
const b: WatchlistItem = { code: "000002.SZ", name: "B", added_at: "2026-01-02" };
const key = "stock-optimizer-watchlist";
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const snapshot = (items: WatchlistItem[], revision = 0, migrationComplete = true) => ({ items, revision, migrationComplete });
const tick = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };

beforeEach(() => {
  vi.resetModules();
  native.invoke.mockReset();
  native.available = true;
  const memory = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => memory.get(k) ?? null,
    setItem: (k: string, v: string) => { memory.set(k, v); },
    removeItem: (k: string) => { memory.delete(k); },
  });
});
afterEach(() => vi.unstubAllGlobals());

describe("watchlist persistence", () => {
  it("does not dispatch a second write before the first backend acknowledgement", async () => {
    const first = deferred<unknown>();
    native.invoke.mockImplementation(async (command: string) => command === "api_watchlist_list" ? [] : snapshot([]));
    const store = await import("./watchlistStore");
    await store.loadPersistentWatchlist([], vi.fn());
    native.invoke.mockClear();
    native.invoke.mockReturnValueOnce(first.promise).mockResolvedValue(snapshot([a, b], 2));
    const set = store.createPersistentWatchlistSetter(vi.fn());
    set([a]);
    await tick();
    set([a, b]);
    await tick();
    expect(native.invoke).toHaveBeenCalledTimes(1);
    first.resolve(snapshot([a], 1));
    await tick();
    expect(native.invoke).toHaveBeenCalledTimes(2);
    expect(native.invoke.mock.calls.map(([command]) => command)).toEqual(["api_watchlist_mutate", "api_watchlist_mutate"]);
    expect(native.invoke.mock.calls[1][1].payload).toMatchObject({ expectedRevision: 1, mutation: { kind: "delta", upserts: [b], removes: [] } });
  });

  it("does not erase cache or queued edits when the initial read resolves late", async () => {
    localStorage.setItem(key, JSON.stringify([a]));
    const read = deferred<unknown>();
    const write = deferred<unknown>();
    native.invoke.mockReturnValueOnce(read.promise).mockReturnValue(write.promise);
    const store = await import("./watchlistStore");
    const changed = vi.fn();
    const loading = store.loadPersistentWatchlist([a], changed);
    store.createPersistentWatchlistSetter(changed)([a, b]);
    await tick();
    read.resolve(snapshot([a], 4));
    await tick();
    expect(JSON.parse(localStorage.getItem(key)!)).toEqual([a, b]);
    expect(changed).toHaveBeenLastCalledWith([a, b]);
    expect(native.invoke.mock.calls[1][1].payload).toMatchObject({ expectedRevision: 4, mutation: { kind: "delta", upserts: [b] } });
    write.resolve(snapshot([a, b], 5));
    await loading;
  });
});


it("keeps failed edits, blocks later writes, and retries the identical operation ID", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([]));
  await store.loadPersistentWatchlist([], vi.fn());
  const changed = vi.fn();
  const listener = vi.fn();
  const unsubscribe = store.subscribeWatchlistPersistence(listener);
  const set = store.createPersistentWatchlistSetter(changed);
  const failed = deferred<unknown>();
  native.invoke.mockReturnValueOnce(failed.promise);
  set([a]);
  await tick();
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saving");
  failed.reject("disk full");
  await tick();
  expect(store.getWatchlistPersistenceSnapshot()).toMatchObject({ status: "error", pendingCount: 1 });
  expect(changed).toHaveBeenLastCalledWith([a]);
  const failedPayload = structuredClone(native.invoke.mock.calls[1][1]);
  set([a, b]);
  await tick();
  expect(native.invoke).toHaveBeenCalledTimes(2);
  native.invoke.mockResolvedValueOnce(snapshot([a], 1)).mockResolvedValueOnce(snapshot([a], 1)).mockResolvedValueOnce(snapshot([a, b], 2));
  await store.retryWatchlistPersistence();
  expect(native.invoke.mock.calls[2][1]).toEqual(failedPayload);
  expect(store.getWatchlistPersistenceSnapshot()).toMatchObject({ status: "saved", pendingCount: 0 });
  expect(listener).toHaveBeenCalled();
  unsubscribe();
});

it("rebases a rejected stale revision only on explicit retry without removing unrelated remote items", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([a], 1));
  const changed = vi.fn();
  await store.loadPersistentWatchlist([], changed);
  native.invoke.mockRejectedValueOnce(JSON.stringify({ code: "WATCHLIST_CONFLICT", revision: 2 }));
  store.createPersistentWatchlistSetter(changed)([]);
  await tick();
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("error");
  native.invoke.mockResolvedValueOnce(snapshot([a, b], 2)).mockResolvedValueOnce(snapshot([b], 3));
  await store.retryWatchlistPersistence();
  expect(native.invoke.mock.calls[3][1].payload).toMatchObject({ expectedRevision: 2, mutation: { kind: "delta", removes: [a.code], upserts: [] } });
  expect(changed).toHaveBeenLastCalledWith([b]);
});

it("does not import stale localStorage into an authoritative empty DB", async () => {
  localStorage.setItem(key, JSON.stringify([a]));
  native.invoke.mockResolvedValue(snapshot([], 9, true));
  const store = await import("./watchlistStore");
  const changed = vi.fn();
  await store.loadPersistentWatchlist([a], changed);
  expect(native.invoke).toHaveBeenCalledTimes(1);
  expect(changed).toHaveBeenLastCalledWith([]);
  expect(JSON.parse(localStorage.getItem(key)!)).toEqual([]);
});

it("serializes one-shot migration before edits made during initial read", async () => {
  localStorage.setItem(key, JSON.stringify([a]));
  const read = deferred<unknown>();
  const migration = deferred<unknown>();
  native.invoke.mockReturnValueOnce(read.promise).mockReturnValueOnce(migration.promise).mockResolvedValue(snapshot([b], 2));
  const store = await import("./watchlistStore");
  const changed = vi.fn();
  const loading = store.loadPersistentWatchlist([a], changed);
  store.createPersistentWatchlistSetter(changed)([b]);
  read.resolve(snapshot([], 0, false));
  await tick();
  expect(native.invoke.mock.calls[1][1].payload.mutation).toEqual({ kind: "migrate", items: [a] });
  expect(changed).toHaveBeenLastCalledWith([b]);
  expect(JSON.parse(localStorage.getItem(key)!)).toEqual([b]);
  migration.resolve(snapshot([a], 1, true));
  await loading;
  expect(native.invoke.mock.calls[2][1].payload).toMatchObject({ expectedRevision: 1, mutation: { kind: "delta", removes: [a.code], upserts: [b] } });
  expect(changed).toHaveBeenLastCalledWith([b]);
});

it("restores an unacknowledged operation after reload using exactly the same ID and payload", async () => {
  let store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([]));
  await store.loadPersistentWatchlist([], vi.fn());
  native.invoke.mockRejectedValueOnce("lost acknowledgement");
  store.createPersistentWatchlistSetter(vi.fn())([a]);
  await tick();
  const payload = structuredClone(native.invoke.mock.calls[1][1]);
  vi.resetModules();
  store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([a], 1)).mockResolvedValueOnce(snapshot([a], 1)).mockResolvedValueOnce(snapshot([a], 1));
  await store.loadPersistentWatchlist(store.loadLocalWatchlistSnapshot(), vi.fn());
  expect(native.invoke.mock.calls[3][1]).toEqual(payload);
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saved");
});

it("preserves edits and allows a retry if the first read fails", async () => {
  const store = await import("./watchlistStore");
  const read = deferred<unknown>();
  native.invoke.mockReturnValueOnce(read.promise);
  const changed = vi.fn();
  const loading = store.loadPersistentWatchlist([], changed);
  store.createPersistentWatchlistSetter(changed)([b]);
  read.reject("database busy");
  await loading;
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("error");
  expect(changed).toHaveBeenLastCalledWith([b]);
  native.invoke.mockResolvedValueOnce(snapshot([a], 2)).mockResolvedValueOnce(snapshot([a, b], 3));
  await store.retryWatchlistPersistence();
  expect(changed).toHaveBeenLastCalledWith([a, b]);
});

it("does not announce native saved when invoke is missing", async () => {
  vi.mocked(native.invoke).mockRejectedValue(new Error("command not found"));
  const store = await import("./watchlistStore");
  await store.loadPersistentWatchlist([], vi.fn());
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("error");
});


it("does not regress to an old duplicate acknowledgement after the DB was cleared elsewhere", async () => {
  let store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([]));
  await store.loadPersistentWatchlist([], vi.fn());
  native.invoke.mockRejectedValueOnce("ack lost");
  store.createPersistentWatchlistSetter(vi.fn())([a]);
  await tick();
  vi.resetModules();
  store = await import("./watchlistStore");
  const changed = vi.fn();
  native.invoke.mockResolvedValueOnce(snapshot([], 2)).mockResolvedValueOnce(snapshot([a], 1)).mockResolvedValueOnce(snapshot([], 2));
  await store.loadPersistentWatchlist(store.loadLocalWatchlistSnapshot(), changed);
  expect(changed).toHaveBeenLastCalledWith([]);
  expect(JSON.parse(localStorage.getItem(key)!)).toEqual([]);
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saved");
});

it("does not strand an edit queued by a subscriber at the saved notification", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([])).mockResolvedValueOnce(snapshot([a], 1));
  let queued = false;
  const set = store.createPersistentWatchlistSetter(vi.fn());
  const unsubscribe = store.subscribeWatchlistPersistence(() => {
    if (store.getWatchlistPersistenceSnapshot().status === "saved" && !queued) { queued = true; set([a]); }
  });
  await store.loadPersistentWatchlist([], vi.fn());
  await tick();
  expect(native.invoke).toHaveBeenCalledTimes(2);
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saved");
  unsubscribe();
});

it("does not dispatch if the durable outbox write fails and preserves the edit for retry", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([])).mockResolvedValueOnce(snapshot([a], 1));
  await store.loadPersistentWatchlist([], vi.fn());
  const original = localStorage.setItem;
  const spy = vi.spyOn(localStorage, "setItem").mockImplementationOnce(() => { throw new Error("quota exceeded"); });
  store.createPersistentWatchlistSetter(vi.fn())([a]);
  await tick();
  expect(native.invoke).toHaveBeenCalledTimes(1);
  expect(store.getWatchlistPersistenceSnapshot()).toMatchObject({ status: "error", pendingCount: 1 });
  spy.mockImplementation(original);
  await store.retryWatchlistPersistence();
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saved");
});

it("marks an optimistic item update unsaved before publishing it to the UI", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([]));
  await store.loadPersistentWatchlist([], vi.fn());
  const ack = deferred<unknown>();
  native.invoke.mockReturnValueOnce(ack.promise);
  const setter = vi.fn(() => { expect(store.getWatchlistPersistenceSnapshot().status).not.toBe("saved"); });
  store.createPersistentWatchlistSetter(setter)([a]);
  await tick();
  ack.resolve(snapshot([a], 1));
  await tick();
});

it("normalizes stock aliases before calculating a delta and preserves the existing added date", async () => {
  const store = await import("./watchlistStore");
  const original = { code: "500001.SH", added_at: "2025-01-01", name: "Fund" };
  native.invoke.mockResolvedValueOnce(snapshot([original]));
  await store.loadPersistentWatchlist([], vi.fn());
  store.createPersistentWatchlistSetter(vi.fn())([{ code: "sh500001", name: "Fund" }]);
  await tick();
  expect(native.invoke).toHaveBeenCalledTimes(1);
});

it("retries outbox cleanup after native acknowledgement without replaying the edit", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([])).mockResolvedValueOnce(snapshot([a], 1));
  await store.loadPersistentWatchlist([], vi.fn());
  const write = localStorage.setItem;
  let failCleanup = true;
  vi.spyOn(localStorage, "setItem").mockImplementation((k, value) => {
    if (failCleanup && k.includes("outbox") && JSON.parse(value).pending.length === 0) throw new Error("cleanup quota failure");
    write(k, value);
  });
  store.createPersistentWatchlistSetter(vi.fn())([a]);
  await tick();
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("error");
  failCleanup = false;
  await store.retryWatchlistPersistence();
  expect(native.invoke).toHaveBeenCalledTimes(2);
  expect(JSON.parse(localStorage.getItem("stock-optimizer-watchlist-outbox-v1")!).pending).toEqual([]);
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saved");
});

it.each([false, true])("R3 reconciles same-renderer lost-ack retry against current authority (queued follow-up=%s)", async (followup) => {
  const store = await import("./watchlistStore");
  const changed = vi.fn();
  native.invoke.mockResolvedValueOnce(snapshot([], 0));
  await store.loadPersistentWatchlist([], changed);
  native.invoke.mockRejectedValueOnce("acknowledgement lost after commit");
  const set = store.createPersistentWatchlistSetter(changed);
  set([a]);
  await tick();
  const attempted = structuredClone(native.invoke.mock.calls[1][1]);
  if (followup) set([a, b]);
  // The add committed at 1, then another supported writer cleared at 2.
  // No module reset: the renderer still remembers revision 0.
  const retryCommands: string[] = [];
  native.invoke.mockImplementation(async (command: string, args?: { payload: { operationId: string; expectedRevision: number } }) => {
    retryCommands.push(command);
    if (command === "api_watchlist_snapshot") return snapshot([], 2);
    if (args?.payload.operationId === attempted.payload.operationId) {
      expect(args).toEqual(attempted);
      return snapshot([a], 1);
    }
    expect(args?.payload.expectedRevision).toBe(2);
    return snapshot([b], 3);
  });
  const savedViews: WatchlistItem[][] = [];
  const unsubscribe = store.subscribeWatchlistPersistence(() => {
    if (store.getWatchlistPersistenceSnapshot().status === "saved") savedViews.push(JSON.parse(localStorage.getItem(key)!));
  });
  await store.retryWatchlistPersistence();
  expect(retryCommands).toContain("api_watchlist_snapshot");
  expect(changed).toHaveBeenLastCalledWith(followup ? [b] : []);
  expect(savedViews).toEqual([followup ? [b] : []]);
  expect(store.getWatchlistPersistenceSnapshot()).toMatchObject({ status: "saved", pendingCount: 0 });
  unsubscribe();
});

it("R2 preserves stale cache and dispatches no migration when the native DB header is damaged", async () => {
  localStorage.setItem(key, JSON.stringify([a]));
  native.invoke.mockRejectedValue("database header is incomplete; original preserved");
  const store = await import("./watchlistStore");
  await store.loadPersistentWatchlist([a], vi.fn());
  await store.retryWatchlistPersistence();
  expect(native.invoke.mock.calls.map(([command]) => command)).toEqual(["api_watchlist_snapshot", "api_watchlist_snapshot"]);
  expect(JSON.parse(localStorage.getItem(key)!)).toEqual([a]);
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("error");
});

it("R3 retains the exact operation and queued follow-up when the authority read after replay fails", async () => {
  const store = await import("./watchlistStore");
  native.invoke.mockResolvedValueOnce(snapshot([]));
  const changed = vi.fn();
  await store.loadPersistentWatchlist([], changed);
  native.invoke.mockRejectedValueOnce("lost ack");
  const set = store.createPersistentWatchlistSetter(changed);
  set([a]);
  await tick();
  const original = structuredClone(native.invoke.mock.calls[1][1]);
  set([a, b]);
  native.invoke.mockResolvedValueOnce(snapshot([a], 1)).mockRejectedValueOnce("authority unavailable");
  await store.retryWatchlistPersistence();
  expect(store.getWatchlistPersistenceSnapshot()).toMatchObject({ status: "error", pendingCount: 2 });
  expect(changed).toHaveBeenLastCalledWith([a, b]);
  expect(native.invoke.mock.calls[2][1]).toEqual(original);
  expect(native.invoke.mock.calls[3][0]).toBe("api_watchlist_snapshot");
  native.invoke.mockResolvedValueOnce(snapshot([a], 1)).mockResolvedValueOnce(snapshot([], 2)).mockResolvedValueOnce(snapshot([b], 3));
  await store.retryWatchlistPersistence();
  expect(native.invoke.mock.calls[4][1]).toEqual(original);
  expect(native.invoke.mock.calls[6][1].payload.expectedRevision).toBe(2);
  expect(changed).toHaveBeenLastCalledWith([b]);
  expect(store.getWatchlistPersistenceSnapshot().status).toBe("saved");
});
