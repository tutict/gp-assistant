import { useCallback, useEffect, useState } from "react";

export type FontScale = "small" | "standard" | "large";

const FONT_SCALE_KEY = "stock-optimizer-font-scale";

function isFontScale(value: string | null): value is FontScale {
  return value === "small" || value === "standard" || value === "large";
}

function readFontScale(): FontScale {
  if (typeof window === "undefined") return "standard";
  const requested = new URLSearchParams(window.location.search).get("fontScale");
  if (isFontScale(requested)) return requested;
  const saved = localStorage.getItem(FONT_SCALE_KEY);
  return isFontScale(saved) ? saved : "standard";
}

export function useFontScale(): {
  fontScale: FontScale;
  setFontScale: (fontScale: FontScale) => void;
} {
  const [fontScale, setFontScaleState] = useState<FontScale>(readFontScale);

  const applyFontScale = useCallback((next: FontScale) => {
    document.documentElement.dataset.fontScale = next;
    localStorage.setItem(FONT_SCALE_KEY, next);
  }, []);

  useEffect(() => {
    applyFontScale(fontScale);
  }, [applyFontScale, fontScale]);

  const setFontScale = useCallback((next: FontScale) => setFontScaleState(next), []);
  return { fontScale, setFontScale };
}
