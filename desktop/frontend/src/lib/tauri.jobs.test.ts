import { afterEach, expect, it, vi } from "vitest";
afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
it("routes normal backtest IPC through native jobs and propagates cancellation", async () => {
  const invoke=vi.fn((command:string)=>command==="api_job_cancel"?Promise.resolve({state:"cancelled"}):new Promise(()=>{}));
  vi.stubGlobal("window",{location:{href:"http://tauri.localhost/"},__TAURI__:{core:{invoke}},setTimeout,clearTimeout});
  vi.stubGlobal("navigator",{userAgent:"Windows"});
  vi.stubGlobal("localStorage",{getItem:()=>null});
  const {postJson}=await import("./tauri");
  const cancel=new AbortController();
  const pending=postJson("/api/backtest",{}, {signal:cancel.signal});
  await Promise.resolve();
  expect(invoke.mock.calls[0]?.[0]).toBe("api_job_run");
  const rejection=expect(pending).rejects.toBeDefined();
  cancel.abort(); await rejection; await Promise.resolve();
  expect(invoke.mock.calls.some(call=>call[0]==="api_job_cancel")).toBe(true);
});

it("shares an in-flight market refresh across toolbar and screening callers", async () => {
  let finishRefresh: (value: unknown) => void = () => undefined;
  const invoke = vi.fn((command: string) => {
    if (command === "api_job_run") return new Promise(resolve => { finishRefresh = resolve; });
    return Promise.resolve({});
  });
  vi.stubGlobal("window", {
    location: { href: "http://tauri.localhost/" },
    __TAURI__: { core: { invoke } },
    setTimeout,
    clearTimeout,
  });
  vi.stubGlobal("navigator", { userAgent: "Windows" });
  vi.stubGlobal("localStorage", { getItem: () => null });
  vi.stubGlobal("document", {
    visibilityState: "visible",
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: false, status: 404 })));

  const { refreshTauriMarketData } = await import("./tauri");
  const invokeFn = invoke as unknown as <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  const first = refreshTauriMarketData(invokeFn, { validate_after_write: false });
  const second = refreshTauriMarketData(invokeFn, { validate_after_write: false });
  expect(second).toBe(first);

  await vi.waitFor(() => expect(invoke.mock.calls.filter(([command]) => command === "api_job_run")).toHaveLength(1));
  finishRefresh({ done: true, next_batch_start: 1, total_batches: 1, fetched: 1, requested: 1 });
  await Promise.all([first, second]);
  expect(invoke.mock.calls.filter(([command]) => command === "api_job_run")).toHaveLength(1);
});
