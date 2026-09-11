import { useCallback, useSyncExternalStore } from "react";

/** Structural responsiveness, independent of the runtime and all data fetching. */
export function useMediaQuery(query: string): boolean {
  const read = useCallback(() => typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(query).matches : false, [query]);
  const subscribe = useCallback((notify: () => void) => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => {};
    const media = window.matchMedia(query);
    media.addEventListener("change", notify);
    return () => media.removeEventListener("change", notify);
  }, [query]);
  return useSyncExternalStore(subscribe, read, () => false);
}
