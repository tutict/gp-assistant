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
