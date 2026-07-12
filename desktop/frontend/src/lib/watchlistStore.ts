import type { WatchlistItem } from "../types";
import { getTauriInvoke, isTauriRuntime } from "./tauri";

const WATCHLIST_KEY = "stock-optimizer-watchlist";

type WatchlistSetter = (items: WatchlistItem[]) => void;

let watchlistMutationVersion = 0;

function normalizeWatchlist(items: WatchlistItem[]): WatchlistItem[] {
  const seen = new Set<string>();
  const normalized: WatchlistItem[] = [];
  for (const item of items) {
    const code = String(item?.code || "").trim();
    if (!code || seen.has(code)) continue;
    seen.add(code);
    normalized.push({
      code,
      name: item.name,
      industry: item.industry,
      added_at: item.added_at || new Date().toISOString(),
      source: item.source,
      screenCriteriaSummary: item.screenCriteriaSummary,
    });
  }
  return normalized;
}

export function loadLocalWatchlistSnapshot(): WatchlistItem[] {
  try {
    const raw = localStorage.getItem(WATCHLIST_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? normalizeWatchlist(parsed as WatchlistItem[]) : [];
  } catch {
    return [];
  }
}

export function persistLocalWatchlistSnapshot(items: WatchlistItem[]): void {
  try {
    localStorage.setItem(WATCHLIST_KEY, JSON.stringify(normalizeWatchlist(items)));
  } catch {
    // localStorage remains a fallback cache only; database persistence is authoritative in Tauri.
  }
}

export async function loadPersistentWatchlist(localSnapshot: WatchlistItem[], setItems: WatchlistSetter): Promise<void> {
  const loadVersion = watchlistMutationVersion;
  if (!isTauriRuntime()) {
    if (loadVersion === watchlistMutationVersion) setItems(normalizeWatchlist(localSnapshot));
    return;
  }
  const invoke = getTauriInvoke();
  if (!invoke) {
    if (loadVersion === watchlistMutationVersion) setItems(normalizeWatchlist(localSnapshot));
    return;
  }
  try {
    const remoteItems = normalizeWatchlist(await invoke<WatchlistItem[]>("api_watchlist_list"));
    if (remoteItems.length === 0 && localSnapshot.length > 0) {
      const migrated = normalizeWatchlist(await invoke<WatchlistItem[]>("api_watchlist_replace", { payload: { items: localSnapshot } }));
      if (loadVersion === watchlistMutationVersion) setItems(migrated);
      persistLocalWatchlistSnapshot(migrated);
      return;
    }
    if (loadVersion === watchlistMutationVersion) setItems(remoteItems);
    persistLocalWatchlistSnapshot(remoteItems);
  } catch (error) {
    console.warn("watchlist sqlite unavailable, falling back to localStorage", error);
    if (loadVersion === watchlistMutationVersion) setItems(normalizeWatchlist(localSnapshot));
  }
}

export function createPersistentWatchlistSetter(setItems: WatchlistSetter): WatchlistSetter {
  return (nextItems: WatchlistItem[]) => {
    const normalized = normalizeWatchlist(nextItems);
    const mutationVersion = ++watchlistMutationVersion;
    setItems(normalized);
    persistLocalWatchlistSnapshot(normalized);
    const invoke = getTauriInvoke();
    if (!invoke) return;
    invoke<WatchlistItem[]>("api_watchlist_replace", { payload: { items: normalized } })
      .then((storedItems) => {
        if (mutationVersion !== watchlistMutationVersion) return;
        const persisted = normalizeWatchlist(storedItems);
        setItems(persisted);
        persistLocalWatchlistSnapshot(persisted);
      })
      .catch((error) => {
        console.warn("failed to persist watchlist to sqlite", error);
      });
  };
}