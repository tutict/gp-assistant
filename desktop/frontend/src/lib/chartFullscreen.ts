type LockableScreenOrientation = ScreenOrientation & {
  lock?: (orientation: "landscape") => Promise<void>;
};

export async function requestChartFullscreen(
  element: HTMLElement,
  { lockLandscape = false }: { lockLandscape?: boolean } = {},
) {
  let nativeFullscreen = false;
  let orientationLocked = false;

  try {
    if (typeof element.requestFullscreen === "function") {
      await element.requestFullscreen({ navigationUI: "hide" });
      nativeFullscreen = document.fullscreenElement === element;
    }
  } catch {
    nativeFullscreen = false;
  }

  try {
    if (!lockLandscape) return { nativeFullscreen, orientationLocked };
    const orientation = screen.orientation as LockableScreenOrientation | undefined;
    if (orientation?.lock) {
      await orientation.lock("landscape");
      orientationLocked = true;
    }
  } catch {
    // Orientation locking is optional in WebView/browser runtimes. CSS provides a fallback.
  }

  return { nativeFullscreen, orientationLocked };
}

export async function exitChartFullscreen(element?: HTMLElement | null) {
  try {
    const activeElement = document.fullscreenElement;
    if (activeElement && (!element || activeElement === element)) {
      await document.exitFullscreen();
    }
  } catch {
    // The application-level fullscreen fallback can still be dismissed below.
  } finally {
    unlockChartOrientation();
  }
}

export function unlockChartOrientation() {
  try {
    screen.orientation?.unlock?.();
  } catch {
    // Some Android WebViews expose Screen Orientation without supporting unlock.
  }
}
