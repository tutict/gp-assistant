import { installDesktopCloseGuard } from "../lib/closeGuard";
import { createContext, useCallback, useContext, useEffect, useRef, useSyncExternalStore, type ReactNode } from "react";
import { createNativeWorkspaceStore, type WorkspaceStore } from "../lib/workspaceStore";
const WorkspaceContext = createContext<WorkspaceStore | null>(null);
export const WorkspaceCredentialsReady = createContext(true);
let shared: WorkspaceStore | undefined;
export function useWorkspaceStore() { return useContext(WorkspaceContext) ?? (shared ??= createNativeWorkspaceStore()); }
export function useWorkspaceStatus() { const store = useWorkspaceStore(); return useSyncExternalStore(store.subscribe, store.getStatus, store.getStatus); }
export function useWorkspaceCredentialsReady() { return useContext(WorkspaceCredentialsReady); }
export function useWorkspaceState<T>(key: string, initial: T): [T, (value: T | ((previous: T) => T), persist?: boolean) => void] {
  const store = useWorkspaceStore();
  const fallback = useRef(initial);
  const getSnapshot = useCallback(() => store.get(key, fallback.current), [key, store]);
  const value = useSyncExternalStore(store.subscribe, getSnapshot, getSnapshot);
  useEffect(() => { void store.initialize(); }, [store]);
  const setValue = useCallback((next: T | ((previous: T) => T), persist = true) => store.set(key, next, persist, fallback.current), [key, store]);
  return [value, setValue];
}
export function WorkspaceProvider({ store, children }: { store?: WorkspaceStore; children: ReactNode }) {
  const current = useRef(store ?? (shared ??= createNativeWorkspaceStore())).current;
  useEffect(() => {
    void current.initialize();
    const stopCloseGuard = installDesktopCloseGuard(current);
    const hidden = () => { if (typeof document !== "undefined" && document.visibilityState === "hidden") void current.flush(); };
    const flush = () => { void current.flush(); };
    if (typeof document !== "undefined") document.addEventListener?.("visibilitychange", hidden);
    if (typeof window !== "undefined") window.addEventListener?.("pagehide", flush);
    return () => {
      stopCloseGuard();
      if (typeof document !== "undefined") document.removeEventListener?.("visibilitychange", hidden);
      if (typeof window !== "undefined") window.removeEventListener?.("pagehide", flush);
      void current.flush();
    };
  }, [current]);
  return <WorkspaceContext.Provider value={current}>{children}</WorkspaceContext.Provider>;
}
export function WorkspaceSaveStatus() {
  const store = useWorkspaceStore(); const status = useWorkspaceStatus();
  return <div className="workspace-save-status" role={status.state === "error" ? "alert" : "status"} aria-live="polite">
    {status.state === "loading" ? "正在恢复本地工作区，可继续编辑…" : status.state === "saving" ? "正在保存…" : status.state === "saved" ? "已保存到本机" : "本地保存失败，当前编辑仍保留在内存中。"}
    {status.state === "error" && <><span>{status.error}</span><button type="button" onClick={() => void store.retry()}>重试保存</button></>}
  </div>;
}
