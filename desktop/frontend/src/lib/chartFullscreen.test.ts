import { afterEach, describe, expect, it, vi } from "vitest";
import { requestChartFullscreen } from "./chartFullscreen";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("chart fullscreen helpers", () => {
  it("does not lock orientation for desktop fullscreen", async () => {
    const lock = vi.fn(async () => undefined);
    const fakeDocument: { fullscreenElement: unknown } = { fullscreenElement: null };
    const element = {
      requestFullscreen: vi.fn(async () => {
        fakeDocument.fullscreenElement = element;
      }),
    };
    vi.stubGlobal("document", fakeDocument);
    vi.stubGlobal("screen", { orientation: { lock } });

    const result = await requestChartFullscreen(element as unknown as HTMLElement);

    expect(result).toEqual({ nativeFullscreen: true, orientationLocked: false });
    expect(lock).not.toHaveBeenCalled();
  });

  it("locks landscape orientation only when requested by mobile", async () => {
    const lock = vi.fn(async () => undefined);
    const fakeDocument: { fullscreenElement: unknown } = { fullscreenElement: null };
    const element = {
      requestFullscreen: vi.fn(async () => {
        fakeDocument.fullscreenElement = element;
      }),
    };
    vi.stubGlobal("document", fakeDocument);
    vi.stubGlobal("screen", { orientation: { lock } });

    const result = await requestChartFullscreen(element as unknown as HTMLElement, { lockLandscape: true });

    expect(result).toEqual({ nativeFullscreen: true, orientationLocked: true });
    expect(lock).toHaveBeenCalledWith("landscape");
  });
});
