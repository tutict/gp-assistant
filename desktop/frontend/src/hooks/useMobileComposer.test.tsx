import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import { useMobileComposer } from "./useMobileComposer";

describe("composer input method protection", () => {
  it.each([
    { name: "overlays the WebView", innerHeight: 640, visualHeight: 640, nativeHeight: "640px", ime: "240px", mobile: true, keyboard: true, available: 400, scale: 1 },
    { name: "already resizes the WebView", innerHeight: 400, visualHeight: 400, nativeHeight: "640px", ime: "240px", mobile: true, keyboard: true, available: 400, scale: 1 },
    { name: "covers wider Android landscape", innerHeight: 390, visualHeight: 390, nativeHeight: "390px", ime: "180px", mobile: false, keyboard: true, available: 210, scale: 1 },
    { name: "leaves a short landscape editing area", innerHeight: 390, visualHeight: 390, nativeHeight: "390px", ime: "240px", mobile: false, keyboard: true, available: 150, scale: 1 },
    { name: "has no complete native viewport", innerHeight: 400, visualHeight: 400, nativeHeight: "", ime: "240px", mobile: true, keyboard: false, available: 400, scale: 1 },
    { name: "has a negative native height", innerHeight: 400, visualHeight: 400, nativeHeight: "-100px", ime: "240px", mobile: true, keyboard: false, available: 400, scale: 1 },
    { name: "has non-finite native dimensions", innerHeight: 400, visualHeight: 400, nativeHeight: "Infinitypx", ime: "Infinitypx", mobile: true, keyboard: false, available: 400, scale: 1 },
    { name: "is a pinch gesture without native insets", innerHeight: 640, visualHeight: 320, nativeHeight: "", ime: "", mobile: true, keyboard: false, available: 320, scale: 2 },
  ])("handles a viewport that $name and restores its styles on Back", async ({ innerHeight, visualHeight, nativeHeight, ime, mobile, keyboard, available, scale }) => {
    const properties = new Map([
      ["--native-ime-height", ime],
      ["--native-viewport-height", nativeHeight],
      ["--composer-viewport-height", "500px"],
    ]);
    const listeners = new Map<string, () => void>();
    const root = {
      dataset: {} as Record<string, string>,
      style: {
        getPropertyValue: (name: string) => properties.get(name) || "",
        setProperty: (name: string, value: string) => properties.set(name, value),
        removeProperty: (name: string) => properties.delete(name),
      },
    };
    const textarea = { style: {} as Record<string, string>, scrollHeight: 200 };
    const listen = (type: string, callback: () => void) => listeners.set(type, callback);
    const unlisten = (type: string) => listeners.delete(type);
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", { documentElement: root, activeElement: textarea });
    vi.stubGlobal("window", {
      innerHeight,
      matchMedia: () => ({ matches: mobile, addEventListener: vi.fn(), removeEventListener: vi.fn() }),
      visualViewport: { height: visualHeight, scale, offsetTop: 0, addEventListener: vi.fn(), removeEventListener: vi.fn() },
      addEventListener: listen,
      removeEventListener: unlisten,
    });
    function Probe() {
      const composer = useMobileComposer("保留的研究草稿");
      return <textarea ref={composer.textareaRef} onFocus={composer.onFocus} />;
    }
    let renderer: ReactTestRenderer | undefined;
    try {
      await act(async () => { renderer = create(<Probe />, { createNodeMock: () => textarea }); });
      expect(root.dataset.composerKeyboard).toBe(keyboard ? "open" : undefined);
      expect(properties.get("--composer-viewport-height")).toBe(keyboard ? `${available}px` : "500px");
      expect(textarea.style.maxHeight).toBe(`${Math.max(48, Math.floor(available * 0.3))}px`);
      expect(textarea.style.minHeight).toBe("48px");
      properties.set("--native-ime-height", "0px");
      await act(async () => { listeners.get("gp-native-viewport-resize")?.(); });
      expect(root.dataset.composerKeyboard).toBeUndefined();
      expect(properties.get("--composer-viewport-height")).toBe("500px");
    } finally {
      if (renderer) await act(async () => renderer?.unmount());
      vi.unstubAllGlobals();
    }
  });

  it("does not submit during Chinese composition or the browser composition-end Enter", async () => {
    const submit = vi.fn();
    function Probe() {
      const composer = useMobileComposer("");
      return <textarea onCompositionStart={composer.onCompositionStart} onCompositionEnd={composer.onCompositionEnd}
        onKeyDown={event => { if (event.key === "Enter" && !composer.isComposing(event)) submit(); }} />;
    }
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<Probe />); });
    const input = renderer.root.findByType("textarea");
    await act(async () => {
      input.props.onCompositionStart();
      input.props.onKeyDown({ key: "Enter", nativeEvent: {} });
      input.props.onCompositionEnd();
      input.props.onKeyDown({ key: "Enter", nativeEvent: { isComposing: true } });
      input.props.onKeyDown({ key: "Enter", keyCode: 229, nativeEvent: {} });
    });
    expect(submit).not.toHaveBeenCalled();
    await act(async () => input.props.onKeyDown({key:"Enter",nativeEvent:{}}));
    expect(submit).toHaveBeenCalledOnce();
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });
});
