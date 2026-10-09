import { describe, expect, it, vi } from "vitest";
import { invokeNativeJob, nativeJobInvocation, newNativeJobId } from "./nativeJobs";

describe("native job bridge", () => {
  it("maps only the explicit allowlist and preserves backtest payload", () => {
    const payload = { strategy_mode: "adaptive_swing_v1", as_of_date: "20250101" };
    expect(nativeJobInvocation("api_backtest", { payload }, "j1")).toEqual({ command: "api_job_run", args: { jobId: "j1", kind: "backtest", payload } });
    expect(nativeJobInvocation("api_research_rebuild_index", undefined, "j2")?.args.payload).toEqual({});
    expect(nativeJobInvocation("api_agent_start", { payload }, "j3")).toBeNull();
    expect(nativeJobInvocation("api_research_import_url", { payload }, "j3")).toBeNull();
  });

  it("allocates fresh retry IDs and keeps an explicit retransmission ID", async () => {
    const invoke = vi.fn().mockResolvedValue({ ok: true });
    await invokeNativeJob(invoke, "api_backtest", { payload: {} });
    await invokeNativeJob(invoke, "api_backtest", { payload: {} });
    expect(invoke.mock.calls[0]?.[1].jobId).not.toBe(invoke.mock.calls[1]?.[1].jobId);
    await invokeNativeJob(invoke, "api_backtest", { payload: {} }, { jobId: "retransmit" });
    expect(invoke.mock.calls[2]?.[1].jobId).toBe("retransmit");
    expect(newNativeJobId()).toMatch(/^[A-Za-z0-9_-]{1,80}$/);
  });

  it("aborts once, calls native cancellation, and ignores late results", async () => {
    const ac = new AbortController();
    let finish!: (value: unknown) => void;
    const invoke = vi.fn((command: string, _args?: Record<string, unknown>) => command === "api_job_run" ? new Promise(resolve => { finish = resolve; }) : Promise.resolve({ state: "cancelled" }));
    const pending = invokeNativeJob(invoke, "api_backtest", { payload: {} }, { jobId: "abort", signal: ac.signal });
    const rejected = expect(pending).rejects.toMatchObject({ name: "AbortError" });
    ac.abort(); ac.abort();
    await rejected;
    expect(invoke.mock.calls.map(c => c[0])).toEqual(["api_job_run", "api_job_cancel"]);
    expect(invoke.mock.calls[1]?.[1]).toEqual({ jobId: "abort" });
    finish({ late: true });
  });

  it("does not schedule work for a pre-aborted request, but cancels explicit retransmission", async () => {
    const invoke = vi.fn().mockResolvedValue({});
    const ac = new AbortController(); ac.abort();
    await expect(invokeNativeJob(invoke, "api_backtest", { payload: {} }, { signal: ac.signal })).rejects.toMatchObject({ name: "AbortError" });
    expect(invoke).not.toHaveBeenCalled();
    await expect(invokeNativeJob(invoke, "api_backtest", { payload: {} }, { signal: ac.signal, jobId: "existing" })).rejects.toMatchObject({ name: "AbortError" });
    expect(invoke).toHaveBeenCalledExactlyOnceWith("api_job_cancel", { jobId: "existing" });
  });

  it("never double-cancels agents or bypasses unknown commands", async () => {
    const invoke = vi.fn();
    await expect(invokeNativeJob(invoke, "api_agent_start", {})).rejects.toThrow("Unsupported native job command");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("removes abort listeners on completion and surfaces cancellation-delivery failure", async () => {
    const ac = new AbortController();
    const invoke = vi.fn().mockResolvedValue({ done: true });
    expect(await invokeNativeJob(invoke, "api_backtest", { payload: {} }, { signal: ac.signal })).toEqual({ done: true });
    ac.abort();
    expect(invoke).toHaveBeenCalledTimes(1);
    const failed = vi.fn();
    const ac2 = new AbortController();
    const broken = vi.fn((cmd: string) => cmd === "api_job_cancel" ? Promise.reject(new Error("offline")) : new Promise(() => {}));
    const pending = invokeNativeJob(broken, "api_backtest", { payload: {} }, { signal: ac2.signal, onCancelError: failed });
    const rejected = expect(pending).rejects.toMatchObject({ name: "AbortError" });
    ac2.abort(); await rejected;
    await Promise.resolve();
    expect(failed).toHaveBeenCalledTimes(1);
  });
});

it("uses secure getRandomValues on supported older Android WebViews without randomUUID", () => {
  const original=globalThis.crypto;
  vi.stubGlobal("crypto",{getRandomValues:original.getRandomValues.bind(original)});
  try { expect(newNativeJobId()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/); } finally { vi.unstubAllGlobals(); }
});
