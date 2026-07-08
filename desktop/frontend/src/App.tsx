import { useCallback, useEffect, useState } from "react";
import { useTheme } from "./hooks/useTheme";
import { useLocalStorage } from "./hooks/useLocalStorage";
import { isMobileTauriRuntime } from "./lib/tauri";
import { sanitizePersistedLlmSettings } from "./lib/contracts";
import { Header } from "./components/Header";
import { Sidebar } from "./components/Sidebar";
import { FilterBar } from "./components/FilterBar";
import { ScreenPanel } from "./components/panels/ScreenPanel";
import { ObservePanel } from "./components/panels/ObservePanel";
import { BacktestPanel } from "./components/panels/BacktestPanel";
import { NewsRagPanel } from "./components/panels/NewsRagPanel";
import { AgentPanel } from "./components/panels/AgentPanel";
import { WatchlistPanel } from "./components/panels/WatchlistPanel";
import { LlmSettingsPanel } from "./components/panels/LlmSettingsPanel";
import type { WatchlistItem, LlmSettings } from "./types";

type ViewKey = "screen" | "observe" | "backtest" | "news" | "agent";
type LlmSettingsUpdater = LlmSettings | null | ((prev: LlmSettings | null) => LlmSettings | null);

interface AppProps {
  onMounted?: () => void;
}

export default function App({ onMounted }: AppProps) {
  const { theme, toggleTheme } = useTheme();
  const [view, setView] = useState<ViewKey>(() => {
    const hash = window.location.hash;
    const map: Record<string, ViewKey> = {
      "#sectionScreen": "screen",
      "#sectionGraph": "screen",
      "#sectionTrend": "screen",
      "#sectionObserve": "observe",
      "#sectionBacktest": "backtest",
      "#sectionNewsRag": "news",
      "#sectionAgent": "agent",
    };
    return map[hash] || "screen";
  });
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [mobileRuntime, setMobileRuntime] = useState(false);
  const [observeCode, setObserveCode] = useState("");
  const [backtestPreferredSource, setBacktestPreferredSource] = useState<"criteria" | "watchlist" | null>(null);

  // Persistent state
  const [watchlist, setWatchlist] = useLocalStorage<WatchlistItem[]>("stock-optimizer-watchlist", []);
  const [storedLlmSettings, setStoredLlmSettings] = useLocalStorage<LlmSettings | null>(
    "stock-optimizer-llm-settings",
    null,
    sanitizePersistedLlmSettings,
  );
  const [llmSettings, setLlmSettingsState] = useState<LlmSettings | null>(() => storedLlmSettings);

  // Filter criteria state
  const [criteria, setCriteria] = useLocalStorage("stock-optimizer-criteria", {
    includeSt: false,
    requireInstitutionBuyRatio: false,
    minRoe: "",
    maxPe: "",
    maxPb: "",
    minMcap: "",
    industry: "",
    resultLimit: 10,
    sortBy: "score",
    sortDir: "desc",
    scoreProfile: "balanced",
  });

  useEffect(() => {
    const mobile = isMobileTauriRuntime();
    setMobileRuntime(mobile);

    const updateRuntimeClasses = () => {
      const root = document.documentElement;
      const viewport = window.visualViewport;
      const width = viewport?.width || window.innerWidth;
      const height = viewport?.height || window.innerHeight;
      const screenWidth = window.screen?.width || width;
      const screenHeight = window.screen?.height || height;
      const viewportShortSide = Math.min(width, height);
      const viewportLongSide = Math.max(width, height);
      const screenShortSide = Math.min(screenWidth, screenHeight);
      const shortSide = Math.min(viewportShortSide, screenShortSide);
      const compact = mobile && (shortSide <= 560 || (viewportShortSide <= 600 && viewportLongSide < 960));
      const tablet = mobile && !compact;
      root.classList.toggle("mobile-tauri", mobile);
      root.classList.toggle("desktop-runtime", !mobile);
      root.classList.toggle("android-tablet", tablet);
      root.classList.toggle("android-phone", compact);
      root.classList.toggle("android-compact", compact);
      root.classList.toggle("android-bottom-nav", compact);
      root.classList.toggle("android-landscape", compact && width >= height);
      root.classList.toggle("android-portrait", compact && width < height);
    };

    updateRuntimeClasses();
    window.addEventListener("resize", updateRuntimeClasses);
    window.visualViewport?.addEventListener("resize", updateRuntimeClasses);
    onMounted?.();
    return () => {
      window.removeEventListener("resize", updateRuntimeClasses);
      window.visualViewport?.removeEventListener("resize", updateRuntimeClasses);
    };
  }, [onMounted]);

  // Hash routing
  useEffect(() => {
    const onHashChange = () => {
      const hash = window.location.hash;
      const map: Record<string, ViewKey> = {
        "#sectionScreen": "screen",
        "#sectionGraph": "screen",
        "#sectionTrend": "screen",
        "#sectionObserve": "observe",
        "#sectionBacktest": "backtest",
        "#sectionNewsRag": "news",
        "#sectionAgent": "agent",
      };
      setView(map[hash] || "screen");
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  // Escape key
  useEffect(() => {
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMobileNavOpen(false);
      }
    };
    document.addEventListener("keydown", onKeydown);
    return () => document.removeEventListener("keydown", onKeydown);
  }, []);

  const navigate = useCallback((v: ViewKey) => {
    setView(v);
    setMobileNavOpen(false);
    const hrefMap: Record<ViewKey, string> = {
      screen: "#sectionScreen",
      observe: "#sectionObserve",
      backtest: "#sectionBacktest",
      news: "#sectionNewsRag",
      agent: "#sectionAgent",
    };
    if (window.location.hash !== hrefMap[v]) {
      history.replaceState(null, "", hrefMap[v]);
    }
  }, []);

  const observeStock = useCallback((code: string) => {
    setObserveCode(code);
    navigate("observe");
  }, [navigate]);

  const runCurrentCriteriaBacktest = useCallback(() => {
    setBacktestPreferredSource("criteria");
    navigate("backtest");
  }, [navigate]);

  const setLlmSettings = useCallback((value: LlmSettingsUpdater) => {
    setLlmSettingsState((prev) => {
      const next = typeof value === "function" ? value(prev) : value;
      setStoredLlmSettings(sanitizePersistedLlmSettings(next));
      return next;
    });
  }, [setStoredLlmSettings]);

  return (
    <div className={`app ${mobileNavOpen ? "mobile-nav-open" : ""}`} data-active-view={view}>
      <Header
        theme={theme}
        onToggleTheme={toggleTheme}
        onToggleMobileNav={() => setMobileNavOpen(true)}
      />

      <Sidebar
        view={view}
        onNavigate={navigate}
        open={mobileNavOpen}
        onClose={() => setMobileNavOpen(false)}
      />

      <main className="workbench">
        {view === "screen" && (
          <FilterBar mobileRuntime={mobileRuntime} />
        )}

        <div className="panels">
          {view === "screen" && (
            <ScreenPanel
              criteria={criteria}
              onCriteriaChange={setCriteria}
              watchlist={watchlist}
              onWatchlistChange={setWatchlist}
              onObserveStock={observeStock}
              onRunBacktest={runCurrentCriteriaBacktest}
              mobileRuntime={mobileRuntime}
            />
          )}
          {view === "observe" && (
            <ObservePanel
              watchlist={watchlist}
              onWatchlistChange={setWatchlist}
              initialCode={observeCode}
            />
          )}
          {view === "backtest" && (
            <BacktestPanel
              criteria={criteria}
              watchlist={watchlist}
              preferredSource={backtestPreferredSource}
            />
          )}
          {view === "news" && (
            <NewsRagPanel llmSettings={llmSettings} />
          )}
          {view === "agent" && (
            <AgentPanel
              llmSettings={llmSettings}
              onLlmSettingsChange={setLlmSettings}
              watchlist={watchlist}
              onWatchlistChange={setWatchlist}
            />
          )}
        </div>

        {view !== "agent" && (
          <WatchlistPanel
            items={watchlist}
            onChange={setWatchlist}
          />
        )}

        {view === "news" && (
          <LlmSettingsPanel
            settings={llmSettings}
            onChange={setLlmSettings}
          />
        )}
      </main>
    </div>
  );
}
