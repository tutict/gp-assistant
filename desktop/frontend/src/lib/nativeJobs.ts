import { newSecureId } from "./secureId";
/** Opt-in bridge. Do not wrap agent commands or layer the old abort handler over this one. */
export type NativeJobInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
export interface NativeJobOptions {
  /** Only reuse for retransmitting the SAME invocation. A user retry must omit this. */
  jobId?: string;
  signal?: AbortSignal | null;
  /** Delivery failed: cancellation is not confirmed; status/restart may be needed. */
  onCancelError?: (error: unknown, jobId: string) => void;
}

const kinds: Readonly<Record<string, string>> = Object.freeze({
  api_market_refresh: "market_refresh",
  api_backtest: "backtest",
  api_research_pack_import: "research_pack_import",
  api_research_rebuild_index: "research_rebuild_index",
});

export function isNativeJobCommand(command: string): boolean { return Object.hasOwn(kinds, command); }
export function newNativeJobId(): string { return newSecureId(); }

export function nativeJobInvocation(
  command: string,
  args: Record<string, unknown> | undefined,
  jobId: string,
): { command: "api_job_run"; args: { jobId: string; kind: string; payload: unknown } } | null {
  if (!Object.hasOwn(kinds, command)) return null;
  if (!/^[A-Za-z0-9_-]{1,80}$/.test(jobId)) throw new Error("Invalid native job ID");
  return { command: "api_job_run", args: { jobId, kind: kinds[command]!, payload: args?.payload ?? {} } };
}

export async function invokeNativeJob<T = unknown>(
  invoke: NativeJobInvoke,
  command: string,
  args?: Record<string, unknown>,
  options: NativeJobOptions = {},
): Promise<T> {
  if (!Object.hasOwn(kinds, command)) throw new Error(`Unsupported native job command: ${command}`);
  const jobId = options.jobId ?? newNativeJobId();
  const mapped = nativeJobInvocation(command, args, jobId)!;
  const { signal } = options;
  let cancelSent = false;
  const cancel = () => {
    if (cancelSent) return;
    cancelSent = true;
    // Never await cancellation before rejecting the caller: a blocked store must not
    // freeze UI abort. Native also accepts cancel-before-submit as a durable tombstone.
    void Promise.resolve().then(() => invoke("api_job_cancel", { jobId })).catch(error => {
      if (options.onCancelError) {
        try { options.onCancelError(error, jobId); } catch { /* Consumer callback cannot create an unhandled rejection. */ }
      } else {
        console.warn("Native job cancellation was not confirmed; inspect job status", jobId);
      }
    });
  };
  const abortError = () => new DOMException("Native job invocation aborted", "AbortError");
  if (signal?.aborted) {
    if (options.jobId) cancel();
    throw abortError();
  }
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const cleanup = () => signal?.removeEventListener("abort", abort);
    const abort = () => {
      if (settled) return;
      settled = true;
      cleanup();
      cancel();
      reject(abortError());
    };
    signal?.addEventListener("abort", abort, { once: true });
    // Close the listener-registration race without submitting work after an abort.
    if (signal?.aborted) { abort(); return; }
    try {
      Promise.resolve(invoke(mapped.command, mapped.args)).then(value => {
        if (!settled) { settled = true; cleanup(); resolve(value as T); }
      }, error => {
        if (!settled) { settled = true; cleanup(); reject(error); }
      });
    } catch (error) {
      if (!settled) { settled = true; cleanup(); reject(error); }
    }
  });
}
