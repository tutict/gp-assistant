export const NATIVE_VIEWPORT_RESIZE_EVENT = "gp-native-viewport-resize";

/** Android edge-to-edge windows can keep both browser viewports unchanged while the IME overlays them. */
export function readMobileViewport() {
  const viewport = window.visualViewport;
  const visualHeight = viewport?.height || window.innerHeight;
  const style = typeof document === "undefined" ? undefined : document.documentElement?.style;
  const nativeImeHeight = Number.parseFloat(style?.getPropertyValue?.("--native-ime-height") || "");
  const nativeHeight = Number.parseFloat(style?.getPropertyValue?.("--native-viewport-height") || "");
  const nativeBoundsValid = Number.isFinite(nativeHeight) && nativeHeight > 0
    && Number.isFinite(nativeImeHeight) && nativeImeHeight >= 0 && nativeImeHeight <= nativeHeight;
  const nativeKeyboard = nativeBoundsValid && nativeImeHeight > 0;
  const browserKeyboard = Boolean(viewport) && (viewport?.scale || 1) === 1
    && window.innerHeight - visualHeight > Math.max(120, window.innerHeight * 0.15);
  return {
    height: nativeBoundsValid ? Math.min(visualHeight, nativeHeight - nativeImeHeight) : visualHeight,
    offsetTop: viewport?.offsetTop || 0,
    keyboard: nativeKeyboard || browserKeyboard,
    nativeKeyboard,
  };
}

/** Recheck form focus after the native inset has resized its independently scrolling dialog. */
export function installNativeInputVisibility() {
  const reveal = () => {
    if (!readMobileViewport().nativeKeyboard) return;
    window.requestAnimationFrame(() => {
      if (!readMobileViewport().nativeKeyboard) return;
      const focused = document.activeElement as HTMLElement | null;
      if (focused?.isConnected && focused.matches?.("input, textarea, select")) {
        focused.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
      }
    });
  };
  window.addEventListener(NATIVE_VIEWPORT_RESIZE_EVENT, reveal);
  return () => window.removeEventListener(NATIVE_VIEWPORT_RESIZE_EVENT, reveal);
}
