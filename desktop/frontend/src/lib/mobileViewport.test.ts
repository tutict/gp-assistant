import { describe, expect, it, vi } from "vitest";
import { installNativeInputVisibility, NATIVE_VIEWPORT_RESIZE_EVENT } from "./mobileViewport";

describe("native focused input visibility", () => {
  it.each([true, false])("scrolls the active input after native layout only when a valid IME exists (%s)", (native) => {
    const properties = new Map(native ? [["--native-ime-height", "240px"], ["--native-viewport-height", "640px"]] : []);
    const callbacks = new Map<string, () => void>();
    const frames: FrameRequestCallback[] = [];
    const focused = { isConnected: true, matches: () => true, scrollIntoView: vi.fn() };
    const doc = { documentElement: { style: { getPropertyValue: (key: string) => properties.get(key) || "" } }, activeElement: focused };
    vi.stubGlobal("document", doc);
    vi.stubGlobal("window", {
      innerHeight: 640,
      visualViewport: { height: 400, scale: 1, offsetTop: 0 },
      addEventListener: (name: string, callback: () => void) => callbacks.set(name, callback),
      removeEventListener: (name: string) => callbacks.delete(name),
      requestAnimationFrame: (callback: FrameRequestCallback) => frames.push(callback),
    });
    const stop = installNativeInputVisibility();
    try {
      callbacks.get(NATIVE_VIEWPORT_RESIZE_EVENT)?.();
      expect(focused.scrollIntoView).not.toHaveBeenCalled();
      frames.shift()?.(0);
      if (native) {
        expect(focused.scrollIntoView).toHaveBeenCalledWith({ block: "nearest", inline: "nearest", behavior: "instant" });
        expect(doc.activeElement).toBe(focused);
        focused.scrollIntoView.mockClear();
        callbacks.get(NATIVE_VIEWPORT_RESIZE_EVENT)?.();
        properties.set("--native-ime-height", "0px");
        frames.shift()?.(0);
      }
      expect(focused.scrollIntoView).not.toHaveBeenCalled();
    } finally {
      stop();
      expect(callbacks.has(NATIVE_VIEWPORT_RESIZE_EVENT)).toBe(false);
      vi.unstubAllGlobals();
    }
  });
});
