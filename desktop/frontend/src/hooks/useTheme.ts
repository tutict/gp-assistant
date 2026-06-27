// Theme management hook

import { useCallback, useEffect, useState } from "react";

const THEME_KEY = "stock-optimizer-theme";

export type Theme = "dark" | "light";

export function useTheme(): {
  theme: Theme;
  toggleTheme: () => void;
  setTheme: (theme: Theme) => void;
} {
  const [theme, setThemeState] = useState<Theme>(() => {
    if (typeof window === "undefined") return "dark";
    const requested = new URLSearchParams(window.location.search).get("theme");
    if (requested === "dark" || requested === "light") return requested;
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === "dark" || saved === "light") return saved;
    return "dark";
  });

  const applyTheme = useCallback((t: Theme) => {
    document.documentElement.dataset.theme = t;
    localStorage.setItem(THEME_KEY, t);
  }, []);

  useEffect(() => {
    applyTheme(theme);
  }, [theme, applyTheme]);

  const setTheme = useCallback((t: Theme) => setThemeState(t), []);
  const toggleTheme = useCallback(() => {
    setThemeState((prev) => (prev === "dark" ? "light" : "dark"));
  }, []);

  return { theme, toggleTheme, setTheme };
}
