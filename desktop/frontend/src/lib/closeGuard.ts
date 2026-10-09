import type { WorkspaceStore } from "./workspaceStore";
import { invoke as nativeInvoke } from "@tauri-apps/api/core";
import { listen as nativeListen } from "@tauri-apps/api/event";
import { getWatchlistPersistenceSnapshot, retryWatchlistPersistence } from "./watchlistStore";
export interface CloseDependencies { flush(): Promise<boolean>; confirmDiscard(): boolean; exit(saved: boolean): Promise<void>; }
export async function prepareUserClose(dependencies: CloseDependencies): Promise<boolean> {
  let saved = false;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    saved = await Promise.race([dependencies.flush(), new Promise<boolean>(resolve => { timeout = setTimeout(() => resolve(false), 5000); })]);
  } catch { /* A failed save never authorizes clean exit. */ }
  finally { if (timeout !== undefined) clearTimeout(timeout); }
  if (!saved && !dependencies.confirmDiscard()) return false;
  await dependencies.exit(saved);
  return true;
}
export function installDesktopCloseGuard(store: WorkspaceStore): () => void {
  if (typeof window === "undefined" || (!window.__TAURI_INTERNALS__ && !window.__TAURI__?.core?.invoke) || /Android|iPhone|iPad|iPod/i.test(typeof navigator === "undefined" ? "" : navigator.userAgent)) return () => {};
  const invoke = window.__TAURI__?.core?.invoke ?? nativeInvoke;
  const listen = window.__TAURI__?.event?.listen ?? nativeListen;
  let disposed = false, busy = false; let unlisten: (() => void) | undefined;
  void listen("client-close-requested", () => {
    if (disposed || busy) return; busy = true;
    void prepareUserClose({
      flush: async () => {
        await store.flush(); await retryWatchlistPersistence();
        return store.getStatus().state === "saved" && getWatchlistPersistenceSnapshot().status === "saved";
      },
      confirmDiscard: () => window.confirm("草稿或自选股尚未保存成功。仍要关闭并放弃未保存的修改吗？"),
      exit: async (saved) => { await invoke("api_app_confirm_close", { saved }); },
    }).catch(() => { /* Window remains open if native confirmation fails. */ }).finally(() => { busy = false; });
  }).then(stop => {
    if (disposed) { stop(); return; } unlisten = stop;
    void invoke("api_app_close_handler_ready", { ready: true }).catch(() => {});
  }).catch(() => {});
  return () => { disposed = true; unlisten?.(); }; // Native page-load start resets readiness; stale unload IPC must not disable a new page.
}
