import type { WatchlistItem } from "../types";
import { getTauriInvoke, isTauriRuntime } from "./tauri";

const WATCHLIST_KEY = "stock-optimizer-watchlist";
const OUTBOX_KEY = "stock-optimizer-watchlist-outbox-v1";
type WatchlistSetter = (items: WatchlistItem[]) => void;
type Delta = { kind: "delta"; upserts: WatchlistItem[]; removes: string[] };
type Mutation = Delta | { kind: "migrate"; items: WatchlistItem[] };
type PendingOperation = { operationId: string; expectedRevision?: number; mutation: Mutation };
type NativeSnapshot = { items: WatchlistItem[]; revision: number; migrationComplete: boolean };
export type WatchlistPersistenceSnapshot = {
  status: "saving" | "saved" | "error";
  pendingCount: number;
  error: string | null;
  storage: "sqlite" | "local";
};

let status: WatchlistPersistenceSnapshot = { status: "saving", pendingCount: 0, error: null, storage: "sqlite" };
const listeners = new Set<() => void>();
let setItems: WatchlistSetter | undefined;
let items: WatchlistItem[] = [];
let migrationItems: WatchlistItem[] = [];
let pending: PendingOperation[] = [];
let initialized = false;
let hydrated = false;
let revision = 0;
let running: Promise<void> | undefined;
let needsRebase = false;
let outboxError: string | undefined;

export function subscribeWatchlistPersistence(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

// Stable until publish; suitable for React.useSyncExternalStore.
export function getWatchlistPersistenceSnapshot(): WatchlistPersistenceSnapshot { return status; }

function publish(next: WatchlistPersistenceSnapshot["status"], error: string | null = null): void {
  status = { status: next, pendingCount: pending.length, error, storage: isTauriRuntime() ? "sqlite" : "local" };
  listeners.forEach((listener) => listener());
}

function normalizeWatchlist(values: WatchlistItem[], previous: WatchlistItem[] = []): WatchlistItem[] {
  const seen = new Set<string>();
  const result: WatchlistItem[] = [];
  for (const item of values) {
    let code = String(item?.code || "").trim().toUpperCase();
    const prefixed = /^(SH|SZ|BJ)(\d{6})$/.exec(code);
    if (prefixed) code = `${prefixed[2]}.${prefixed[1]}`;
    if (!/^\d{6}\.(SH|SZ|BJ)$/.test(code)) {
      const digits = code.replace(/\D/g, "").slice(0, 6);
      if (digits.length === 6) code = `${digits}.${/^[659]/.test(digits) ? "SH" : /^[48]/.test(digits) ? "BJ" : "SZ"}`;
    }
    if (!code || seen.has(code)) continue;
    seen.add(code);
    result.push({ code, name: item.name ?? undefined, industry: item.industry ?? undefined,
      added_at: item.added_at || previous.find((entry) => entry.code === code)?.added_at || new Date().toISOString(), source: item.source ?? undefined,
      screenCriteriaSummary: item.screenCriteriaSummary ?? undefined });
  }
  return result;
}

export function loadLocalWatchlistSnapshot(): WatchlistItem[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(WATCHLIST_KEY) || "[]");
    return Array.isArray(parsed) ? normalizeWatchlist(parsed as WatchlistItem[]) : [];
  } catch { return []; }
}

export function persistLocalWatchlistSnapshot(values: WatchlistItem[]): void {
  try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(normalizeWatchlist(values))); }
  catch { /* Cache only. The native outbox below is written strictly before dispatch. */ }
}

function initialize(localSnapshot = loadLocalWatchlistSnapshot()): void {
  if (initialized) return;
  initialized = true;
  items = normalizeWatchlist(localSnapshot);
  migrationItems = items;
  try {
    const raw = localStorage.getItem(OUTBOX_KEY);
    if (!raw) return;
    const saved = JSON.parse(raw);
    if (saved.version !== 1 || !Array.isArray(saved.items) || !Array.isArray(saved.migrationItems) || !Array.isArray(saved.pending)) {
      throw new Error("自选股待保存记录版本无效，未覆盖原记录。");
    }
    for (const entry of saved.pending) {
      if (typeof entry.operationId !== "string" || !entry.operationId ||
          (entry.expectedRevision !== undefined && (!Number.isSafeInteger(entry.expectedRevision) || entry.expectedRevision < 0)) ||
          !entry.mutation || !["delta", "migrate"].includes(entry.mutation.kind) ||
          (entry.mutation.kind === "delta" && (!Array.isArray(entry.mutation.upserts) || !Array.isArray(entry.mutation.removes))) ||
          (entry.mutation.kind === "migrate" && !Array.isArray(entry.mutation.items))) {
        throw new Error("自选股待保存记录损坏，未覆盖原记录。");
      }
    }
    items = normalizeWatchlist(saved.items);
    migrationItems = normalizeWatchlist(saved.migrationItems);
    pending = saved.pending;
  } catch (error) {
    outboxError = String(error);
    publish("error", outboxError);
  }
}

function saveOutbox(): void {
  if (outboxError) throw new Error(outboxError);
  // Persist IDs and exact attempted requests before invoking native code. A
  // process restart can retry a committed operation whose acknowledgement died.
  localStorage.setItem(OUTBOX_KEY, JSON.stringify({ version: 1, items, migrationItems, pending }));
  persistLocalWatchlistSnapshot(items);
}

function operation(mutation: Mutation): PendingOperation {
  return { operationId: crypto.randomUUID(), mutation };
}

function applyPending(base: WatchlistItem[]): WatchlistItem[] {
  return pending.reduce((current, entry) => {
    if (entry.mutation.kind === "migrate") return entry.mutation.items;
    const removed = new Set(entry.mutation.removes);
    const result = new Map(current.filter((item) => !removed.has(item.code)).map((item) => [item.code, item]));
    for (const item of entry.mutation.upserts) result.set(item.code, item);
    return [...result.values()];
  }, base);
}

function acceptSnapshot(value: unknown): NativeSnapshot {
  const state = value as NativeSnapshot;
  if (!state || !Array.isArray(state.items) || !Number.isSafeInteger(state.revision) || state.revision < 0 || typeof state.migrationComplete !== "boolean") {
    throw new Error("自选股存储返回了无效响应，待保存操作已保留。");
  }
  return { ...state, items: normalizeWatchlist(state.items) };
}

function renderOptimistic(base: WatchlistItem[]): void {
  items = applyPending(base);
  saveOutbox();
  setItems?.(items);
}

function conflict(error: unknown): boolean {
  try {
    const parsed = typeof error === "string" ? JSON.parse(error) : error;
    return (parsed as { code?: string })?.code === "WATCHLIST_CONFLICT";
  } catch { return false; }
}

async function drain(): Promise<void> {
  try {
    if (outboxError) throw new Error(outboxError);
    if (!isTauriRuntime()) {
      // Browser preview has no SQLite. Be explicit about the different storage.
      localStorage.setItem(WATCHLIST_KEY, JSON.stringify(items));
      if (pending.length) throw new Error("待保存操作需要桌面存储连接，请返回应用后重试。");
      hydrated = true;
      setItems?.(items);
      publish("saved");
      return;
    }
    const invoke = getTauriInvoke();
    if (!invoke) throw new Error("无法连接自选股存储，请重试。");
    publish("saving");
    if (!hydrated) {
      const remote = acceptSnapshot(await invoke("api_watchlist_snapshot"));
      revision = remote.revision;
      if (needsRebase && pending[0]) {
        // A conflict guarantees this request did not commit. Transport failures
        // do not: those retain the original ID and expectedRevision verbatim.
        pending[0] = operation(pending[0].mutation);
        needsRebase = false;
      }
      if (!remote.migrationComplete && !pending.some((entry) => entry.mutation.kind === "migrate")) {
        pending.unshift(operation({ kind: "migrate", items: migrationItems }));
      }
      renderOptimistic(remote.items);
      hydrated = true;
    }
    while (pending.length) {
      const entry = pending[0];
      const replaying = entry.expectedRevision !== undefined;
      entry.expectedRevision ??= revision;
      saveOutbox();
      let remote = acceptSnapshot(await invoke("api_watchlist_mutate", { payload: entry }));
      if (replaying || remote.revision < revision) {
        // A lost acknowledgement leaves this renderer's revision stale too.
        // Always read current authority after replaying the exact request,
        // before dropping the queued operation, dispatching a follow-up, or
        // publishing saved. Never trust a duplicate's historical rows.
        remote = acceptSnapshot(await invoke("api_watchlist_snapshot"));
      }
      pending.shift();
      revision = remote.revision;
      renderOptimistic(remote.items);
      publish("saving");
    }
    // Also retry a failed post-ack journal cleanup when there is no remaining
    // backend operation. Do not leave an already-committed request stranded.
    saveOutbox();
    setItems?.(items);
    publish(pending.length ? "saving" : "saved");
  } catch (error) {
    if (conflict(error)) {
      needsRebase = true;
      hydrated = false;
    }
    publish("error", error instanceof Error ? error.message : String(error));
  }
}

function start(): Promise<void> {
  if (running) return running;
  // The microtask boundary installs the single-flight guard before drain can
  // synchronously publish to subscribers that may themselves enqueue edits.
  running = Promise.resolve().then(drain).finally(() => {
    running = undefined;
    if (pending.length && status.status === "saving") void start();
  });
  return running;
}

export function retryWatchlistPersistence(): Promise<void> {
  initialize();
  return start();
}

export async function loadPersistentWatchlist(localSnapshot: WatchlistItem[], setter: WatchlistSetter): Promise<void> {
  initialize(localSnapshot);
  setItems = setter;
  setter(items);
  if (status.status === "error") return;
  if (hydrated && !pending.length) return;
  await start();
}

// Compatibility facade for App's existing array setter. Only changed records
// become deltas; unrelated rows fetched during startup/rebase are not deleted.
export function createPersistentWatchlistSetter(setter: WatchlistSetter): WatchlistSetter {
  return (nextItems) => {
    initialize();
    setItems = setter;
    const next = normalizeWatchlist(nextItems, items);
    const before = new Map(items.map((item) => [item.code, item]));
    const after = new Set(next.map((item) => item.code));
    const upserts = next.filter((item) => JSON.stringify(before.get(item.code)) !== JSON.stringify(item));
    const removes = items.filter((item) => !after.has(item.code)).map((item) => item.code);
    if (!upserts.length && !removes.length) return;
    items = next;
    if (!isTauriRuntime() && !pending.length) {
      try { localStorage.setItem(WATCHLIST_KEY, JSON.stringify(items)); publish("saved"); }
      catch (error) { publish("error", String(error)); }
      setter(items);
      return;
    }
    pending.push(operation({ kind: "delta", upserts, removes }));
    try { saveOutbox(); }
    catch (error) { publish("error", String(error)); setter(items); return; }
    if (status.status === "error") {
      publish("error", status.error);
      setter(items);
      return;
    }
    publish("saving");
    setter(items);
    void start();
  };
}
