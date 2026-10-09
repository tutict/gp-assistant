import { describe, expect, it, vi } from "vitest";
import { createWorkspaceStore, type WorkspaceTransport, type WorkspaceSnapshot } from "./workspaceStore";
const empty = (): WorkspaceSnapshot => ({ schemaVersion: 1, revision: 0, values: {} });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(r => { resolve = r; }); return { promise, resolve }; }
function setup(load: Promise<WorkspaceSnapshot> = Promise.resolve(empty())) {
  let saved = empty();
  const transport: WorkspaceTransport = { load: vi.fn(() => load), commit: vi.fn(async (revision, changes) => {
    if (revision !== saved.revision) throw new Error("revision conflict");
    saved = { ...saved, revision: revision + 1, values: { ...saved.values, ...changes } }; return saved.revision;
  }) };
  return { store: createWorkspaceStore(transport), transport, saved: () => saved };
}
describe("durable workspace", () => {
  it("restores offline without overwriting edits made during hydration", async () => {
    const loading = deferred<WorkspaceSnapshot>(); const { store, transport } = setup(loading.promise);
    const init = store.initialize(); store.set("news.question", "typed while loading");
    loading.resolve({ ...empty(), values: { "news.question": "old", "news.thread": "thread-7" } });
    await init; expect(store.get("news.question", "")).toBe("typed while loading"); expect(store.get("news.thread", "")).toBe("thread-7");
    await store.flush(); expect(transport.commit).toHaveBeenCalledTimes(1);
  });
  it("retains dirty drafts on write error and retries explicitly", async () => {
    const { store, transport, saved } = setup(); await store.initialize();
    vi.mocked(transport.commit).mockRejectedValueOnce(new Error("disk full")); store.set("news.question", "unsent"); await store.flush();
    expect(store.getStatus().state).toBe("error"); expect(store.get("news.question", "")).toBe("unsent");
    await store.retry(); expect(store.getStatus().state).toBe("saved"); expect(saved().values["news.question"]).toBe("unsent");
  });
  it("rejects invalid and future snapshots without committing over them", async () => {
    for (const snapshot of [{ ...empty(), schemaVersion: 2 }, { ...empty(), values: { "news.question": 99 } }]) {
      const { store, transport } = setup(Promise.resolve(snapshot)); store.set("news.question", "local"); await store.initialize(); await store.flush();
      expect(store.getStatus().state).toBe("error"); expect(transport.commit).not.toHaveBeenCalled();
    }
  });
  it("does not clear a newer edit when an older transaction completes", async () => {
    const { store, transport } = setup(); await store.initialize(); const writing = deferred<number>(); vi.mocked(transport.commit).mockReturnValueOnce(writing.promise);
    store.set("news.question", "first"); const flush = store.flush(); await Promise.resolve(); store.set("news.question", "second"); writing.resolve(1); await flush;
    expect(store.get("news.question", "")).toBe("second"); expect(store.getStatus().state).not.toBe("saved"); store.dispose();
  });
  it("debounces within 500ms and ignores transient stream payloads", async () => {
    vi.useFakeTimers(); const { store, transport } = setup(); await store.initialize(); store.set("news.question", "a"); store.set("news.question", "ab");
    await vi.advanceTimersByTimeAsync(400); expect(transport.commit).toHaveBeenCalledTimes(1);
    store.set("agent.conversations", [], false); await vi.advanceTimersByTimeAsync(1000); expect(transport.commit).toHaveBeenCalledTimes(1); store.dispose(); vi.useRealTimers();
  });
  it("migrates once, strips heavy results, preserves tombstones and ignores credentials", async () => {
    const legacy = new Map([
      ["stock-optimizer-agent-conversations", JSON.stringify([{id:"gone",title:"deleted",mode:"quick",messages:[]}, {id:"keep",title:"keep",mode:"quick",createdAt:1,updatedAt:1,messages:[{role:"assistant",content:"reply",timestamp:1,result:{huge:"x"},steps:[]}]}])],
      ["stock-optimizer-agent-active-conversation", "keep"], ["stock-optimizer-agent-failed-ledger-deletion:gone", "completed"], ["stock-optimizer-llm-settings", '{"api_key":"secret"}'],
    ]);
    const source = { getItem: (key: string) => legacy.get(key) ?? null }; const { transport, saved } = setup(); const store = createWorkspaceStore(transport, source);
    await store.initialize(); await store.flush(); expect(JSON.stringify(saved())).not.toContain("secret"); expect(JSON.stringify(saved())).not.toContain("huge");
    expect(JSON.stringify(saved())).not.toContain("gone"); expect(saved().values["agent.active"]).toBe("keep"); expect(legacy.get("stock-optimizer-agent-failed-ledger-deletion:gone")).toBe("completed");
    const reload = createWorkspaceStore({ ...transport, load: async () => saved() }, source); await reload.initialize(); await reload.flush(); expect(transport.commit).toHaveBeenCalledTimes(1);
  });
});

it("retry must not merge deleted conversations back from the last persisted snapshot", async () => {
  const conversation={id:"gone",title:"saved",mode:"quick",messages:[],createdAt:1,updatedAt:1};
  const transport:WorkspaceTransport={load:vi.fn(async()=>({...empty(),values:{"agent.conversations":[conversation]}})),commit:vi.fn(async()=>{throw Error("lost acknowledgement");})};
  const store=createWorkspaceStore(transport); await store.initialize(); expect(store.getStatus().ready).toBe(true); store.set("agent.conversations",[]); await store.flush(); await store.retry();
  expect(store.get("agent.conversations",[])).toEqual([]); store.dispose();
});

it("never marks silently truncated conversation text/history as saved", async () => {
  const {store,saved}=setup(); await store.initialize();
  const messages=Array.from({length:25},(_,i)=>({role:"assistant",content:("完整回答".repeat(600))+i,timestamp:i}));
  store.set("agent.conversations",[{id:"long",mode:"quick",title:"history",messages,createdAt:1,updatedAt:1}]); await store.flush();
  const restored=saved().values["agent.conversations"] as Array<{messages:Array<{content:string}>}>;
  expect(restored[0].messages).toHaveLength(25); expect(restored[0].messages[0].content).toBe(messages[0].content); store.dispose();
});

it("restores backtest parameters without running a backtest", async () => {
  const {store,saved,transport}=setup(); await store.initialize();
  store.set("backtest.start","2021-02-03"); store.set("backtest.costBps",25);
  store.set("backtest.adaptiveSpec",{criteria:{limit:10},mode:"range",horizon:"swing_10_30d",primary_limit:10,exploration_limit:10,run_id:"saved-spec"});
  await store.flush(); expect(store.getStatus().state).toBe("saved");
  const next=createWorkspaceStore({...transport,load:async()=>saved()}); await next.initialize();
  expect(next.get("backtest.start","")).toBe("2021-02-03"); expect(next.get("backtest.costBps",0)).toBe(25); store.dispose(); next.dispose();
});
