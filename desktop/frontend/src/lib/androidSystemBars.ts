export interface AndroidSystemBarTheme {
  theme: "dark" | "light";
  background: string;
  lightStatusBar: boolean;
  lightNavigationBar: boolean;
}

declare global {
  interface Window {
    AndroidSystemBars?: {
      setTheme: (theme: string) => void;
    };
  }
}

const THEMES: Record<"dark" | "light", AndroidSystemBarTheme> = {
  dark: {
    theme: "dark",
    background: "#0D1014",
    lightStatusBar: false,
    lightNavigationBar: false,
  },
  light: {
    theme: "light",
    background: "#F4F6F8",
    lightStatusBar: true,
    lightNavigationBar: true,
  },
};

export function syncAndroidSystemBars(theme: "dark" | "light"): void {
  if (typeof window === "undefined") return;
  const bridge = window.AndroidSystemBars;
  if (!bridge) return;
  const config = THEMES[theme];
  try {
    bridge.setTheme(config.theme);
  } catch (error) {
    console.warn("Android system-bar theme sync failed", error);
  }
}
