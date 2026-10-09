import { invoke as nativeInvoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./tauri";
import { migrateWorkspaceLegacy, validateWorkspaceValues, workspaceValue, WORKSPACE_DEBOUNCE_MS, WORKSPACE_SCHEMA, type LegacyStorage } from "./workspaceSchema";
export interface WorkspaceSnapshot { schemaVersion: number; revision: number; values: Record<string, unknown>; }
export interface WorkspaceTransport { load(): Promise<WorkspaceSnapshot>; commit(revision: number, changes: Record<string, unknown>): Promise<number>; }
export interface WorkspaceStatus { state: "loading" | "saving" | "saved" | "error"; ready: boolean; error?: string; }
export type WorkspaceStore = ReturnType<typeof createWorkspaceStore>;
export function createWorkspaceStore(transport: WorkspaceTransport, legacy?: LegacyStorage) {
  let values: Record<string, unknown> = {};
  let durable: Record<string, unknown> = {};
  const dirty = new Map<string, { version: number; value: unknown }>();
  const touched = new Set<string>();
  const listeners = new Set<() => void>();
  let version = 0, revision = 0, ready = false, hasHydrated = false;
  let status: WorkspaceStatus = { state: "loading", ready: false };
  let hydration: Promise<void> | undefined, writing: Promise<void> | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const emit = () => { for (const listener of listeners) listener(); };
  const report = (state: WorkspaceStatus["state"], error?: string) => { status = { state, ready, ...(error ? { error } : {}) }; emit(); };
  const fail = (error: unknown) => report("error", error instanceof Error ? error.message : String(error));
  const clearTimer = () => { if (timer) clearTimeout(timer); timer = undefined; };
  const schedule = () => {
    clearTimer();
    if (ready && dirty.size && status.state !== "error") timer = setTimeout(() => { void flush(); }, WORKSPACE_DEBOUNCE_MS);
  };
  async function initialize() {
    if (ready) return;
    return hydration ??= (async () => {
      try {
        const snapshot = await transport.load();
        if (snapshot.schemaVersion !== WORKSPACE_SCHEMA || !Number.isSafeInteger(snapshot.revision) || snapshot.revision < 0) throw new Error("Unsupported or invalid workspace schema/revision; original preserved");
        const restored = validateWorkspaceValues(snapshot.values);
        const migration = legacy && restored["migration.localStorage.v1"] !== true ? migrateWorkspaceLegacy(legacy) : {};
        durable = restored; revision = snapshot.revision;
        const incoming = { ...migration, ...restored };
        for (const [key, value] of Object.entries(incoming)) {
          if (!touched.has(key)) values[key] = value;
          else if (!hasHydrated && key === "agent.conversations" && Array.isArray(value) && Array.isArray(values[key])) {
            const local = values[key] as { id: string }[];
            values[key] = [...local, ...value.filter(item => !local.some(entry => entry.id === item.id))].slice(0, 40);
            if (dirty.has(key)) dirty.set(key, { version: ++version, value: workspaceValue(key, values[key]) });
          }
        }
        for (const [key, value] of Object.entries(migration)) if (!(key in restored) && !dirty.has(key)) dirty.set(key, { version: ++version, value });
        ready = true; hasHydrated = true; report(dirty.size ? "saving" : "saved"); schedule();
      } catch (error) { fail(error); }
    })().finally(() => { hydration = undefined; });
  }
  function set<T>(key: string, update: T | ((previous: T) => T), persist = true, fallback?: T) {
    const previous = (Object.hasOwn(values, key) ? values[key] : fallback) as T;
    const next = typeof update === "function" ? (update as (value: T) => T)(previous) : update;
    // Keep edits in memory even if the durable size/type guard rejects them.
    values = { ...values, [key]: next }; touched.add(key);
    if (persist) {
      try {
        const projected = workspaceValue(key, next);
        dirty.set(key, { version: ++version, value: projected });
        if (status.state !== "error") report("saving"); else emit();
        schedule();
      } catch (error) { dirty.set(key, { version: ++version, value: next }); fail(error); }
    } else emit();
  }
  async function flush() {
    clearTimer();
    if (writing) { await writing; return flush(); }
    if (!ready) { await initialize(); clearTimer(); }
    if (!ready || status.state === "error" || !dirty.size) return;
    const batch = new Map(dirty);
    const changes = Object.fromEntries([...batch].map(([key, item]) => [key, item.value]));
    writing = (async () => {
      try {
        const checked = validateWorkspaceValues(changes);
        validateWorkspaceValues({ ...durable, ...checked });
        report("saving");
        const nextRevision = await transport.commit(revision, checked);
        if (nextRevision !== revision + 1) throw new Error("Invalid workspace commit acknowledgement; retry required");
        revision = nextRevision; durable = { ...durable, ...checked };
        for (const [key, item] of batch) if (dirty.get(key)?.version === item.version) dirty.delete(key);
        report(dirty.size ? "saving" : "saved");
      } catch (error) { fail(error); }
    })();
    await writing; writing = undefined; schedule();
  }
  async function retry() {
    if (writing) await writing;
    // Re-read after ambiguous commit/conflict; preserve local dirty edits and don't retry forever.
    ready = false; report("loading"); await initialize();
    if (ready) await flush();
  }
  return { initialize, set, flush, retry,
    get: <T>(key: string, fallback: T): T => (Object.hasOwn(values, key) ? values[key] : fallback) as T,
    getStatus: () => status,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    persist: (key: string) => { if (Object.hasOwn(values, key)) set(key, values[key]); },
    dispose: () => { clearTimer(); listeners.clear(); },
  };
}
const WEB_WORKSPACE_KEY = "stock-optimizer-workspace-v1";
const emptyWorkspaceSnapshot = (): WorkspaceSnapshot => ({ schemaVersion: WORKSPACE_SCHEMA, revision: 0, values: {} });
function readWebWorkspaceSnapshot(): WorkspaceSnapshot {
  if (typeof localStorage === "undefined") return emptyWorkspaceSnapshot();
  const raw = localStorage.getItem(WEB_WORKSPACE_KEY);
  if (!raw) return emptyWorkspaceSnapshot();
  const parsed = JSON.parse(raw) as WorkspaceSnapshot;
  if (!parsed || parsed.schemaVersion !== WORKSPACE_SCHEMA || !Number.isSafeInteger(parsed.revision) || parsed.revision < 0 || !parsed.values || typeof parsed.values !== "object") {
    throw new Error("Invalid browser workspace snapshot; original preserved");
  }
  return parsed;
}

/** SQLite is authoritative in Tauri. The browser preview uses a bounded local adapter so UI harnesses and web previews remain editable without pretending to be native storage. */
export function createNativeWorkspaceStore() {
  const legacy: LegacyStorage = { getItem: key => typeof localStorage === "undefined" ? null : localStorage.getItem(key) };
  if (!isTauriRuntime()) {
    return createWorkspaceStore({
      load: async () => readWebWorkspaceSnapshot(),
      commit: async (expectedRevision, changes) => {
        const current = readWebWorkspaceSnapshot();
        if (current.revision !== expectedRevision) throw new Error("browser workspace revision conflict; reload and retry");
        const next: WorkspaceSnapshot = { schemaVersion: WORKSPACE_SCHEMA, revision: expectedRevision + 1, values: { ...current.values, ...changes } };
        if (typeof localStorage === "undefined") throw new Error("browser workspace storage unavailable");
        localStorage.setItem(WEB_WORKSPACE_KEY, JSON.stringify(next));
        return next.revision;
      },
    }, legacy);
  }
  const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => nativeInvoke<T>(command, args);
  return createWorkspaceStore({ load: () => invoke("api_workspace_load"), commit: (expectedRevision, changes) => invoke("api_workspace_commit", { payload: { expectedRevision, changes } }) }, legacy);
}
