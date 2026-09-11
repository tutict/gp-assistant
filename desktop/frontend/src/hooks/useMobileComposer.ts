import { useCallback, useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { useMediaQuery } from "./useMediaQuery";

/** Keeps a focused composer within the visible viewport, including mobile keyboards. */
export function useMobileComposer(value: string) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const composingRef = useRef(false);
  const mobile = useMediaQuery("(max-width: 768px)");
  const syncRef = useRef<() => void>(() => {});

  const resize = useCallback(() => {
    const textarea = textareaRef.current;
    if (!textarea || typeof window === "undefined") return;
    const available = window.visualViewport?.height || window.innerHeight;
    if (!available) return;
    const limit = Math.floor(available * 0.3);
    textarea.style.height = "auto";
    textarea.style.maxHeight = `${limit}px`;
    textarea.style.minHeight = `${Math.min(44, limit)}px`;
    textarea.style.height = `${Math.min(Math.max(44, textarea.scrollHeight), limit)}px`;
    textarea.style.overflowY = textarea.scrollHeight > limit ? "auto" : "hidden";
  }, []);

  // Run after every render as the panel can finish loading without changing its draft.
  useLayoutEffect(() => { resize(); });

  useEffect(() => {
    if (typeof window === "undefined" || typeof document === "undefined") return;
    const root = document.documentElement;
    if (!root?.style) return;
    const viewport = window.visualViewport;
    const previousKeyboard = root.dataset.composerKeyboard;
    const previousHeight = root.style.getPropertyValue("--composer-viewport-height");
    const previousOffset = root.style.getPropertyValue("--composer-viewport-offset");
    let ownsViewport = false;
    const restore = () => {
      if (!ownsViewport) return;
      if (previousKeyboard === undefined) delete root.dataset.composerKeyboard;
      else root.dataset.composerKeyboard = previousKeyboard;
      if (previousHeight) root.style.setProperty("--composer-viewport-height", previousHeight);
      else root.style.removeProperty("--composer-viewport-height");
      if (previousOffset) root.style.setProperty("--composer-viewport-offset", previousOffset);
      else root.style.removeProperty("--composer-viewport-offset");
      ownsViewport = false;
    };
    const sync = () => {
      resize();
      const focused = document.activeElement === textareaRef.current && textareaRef.current !== null;
      const height = viewport?.height || window.innerHeight;
      // Pinch zoom also shrinks visualViewport; it must not be treated as a keyboard.
      const keyboard = mobile && focused && Boolean(viewport) && (viewport?.scale || 1) === 1
        && window.innerHeight - height > Math.max(120, window.innerHeight * 0.15);
      if (!keyboard) { restore(); return; }
      ownsViewport = true;
      root.dataset.composerKeyboard = "open";
      root.style.setProperty("--composer-viewport-height", `${height}px`);
      root.style.setProperty("--composer-viewport-offset", `${viewport?.offsetTop || 0}px`);
    };
    syncRef.current = sync;
    viewport?.addEventListener("resize", sync);
    viewport?.addEventListener("scroll", sync);
    window.addEventListener?.("resize", sync);
    sync();
    return () => {
      viewport?.removeEventListener("resize", sync);
      viewport?.removeEventListener("scroll", sync);
      window.removeEventListener?.("resize", sync);
      syncRef.current = () => {};
      restore();
    };
  }, [mobile, resize]);

  // Reading value records the draft dependency explicitly for callers and development tools.
  void value;
  return {
    textareaRef,
    mobile,
    onFocus: () => syncRef.current(),
    onBlur: () => { queueMicrotask(() => syncRef.current()); },
    onCompositionStart: () => { composingRef.current = true; },
    onCompositionEnd: () => { composingRef.current = false; },
    isComposing: (event?: KeyboardEvent<HTMLTextAreaElement>) => composingRef.current
      || Boolean(event?.nativeEvent?.isComposing) || event?.keyCode === 229,
  };
}
