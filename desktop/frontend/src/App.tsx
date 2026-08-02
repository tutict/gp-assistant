import { useCallback, useEffect, useRef, useState } from "react";
import { useTheme } from "./hooks/useTheme";
import { useLocalStorage } from "./hooks/useLocalStorage";
import { isMobileTauriRuntime } from "./lib/tauri";
import { createPersistentWatchlistSetter, loadLocalWatchlistSnapshot, loadPersistentWatchlist } from "./lib/watchlistStore";
import { sanitizePersistedLlmSettings } from "./lib/contracts";
import { refreshResearchWatchlist } from "./lib/researchRefresh";
import { Header } from "./components/Header";
import { Sidebar } from "./components/Sidebar";
import { FilterBar } from "./components/FilterBar";
import { ScreenPanel } from "./components/panels/ScreenPanel";
import { ObservePanel } from "./components/panels/ObservePanel";
import { BacktestPanel } from "./components/panels/BacktestPanel";
import { NewsRagPanel } from "./components/panels/NewsRagPanel";
import { AgentPanel } from "./components/panels/AgentPanel";
import { WatchlistPanel } from "./components/panels/WatchlistPanel";
import type { AdaptiveScreenRequest, WatchlistItem, LlmSettings } from "./types";
import {
  consumeBacktestRouteRequest,
  nextBacktestRouteRequest,
  revealActivePanels,
  type BacktestRouteRequest,
} from "./lib/viewNavigation";

type ViewKey = "screen" | "observe" | "backtest" | "news" | "agent";
type LlmSettingsUpdater = LlmSettings | null | ((prev: LlmSettings | null) => LlmSettings | null);
type StockRouteRequest = { code: string; requestId: number };
const RESEARCH_REFRESH_INTERVAL_MS = 15 * 60 * 1000;

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
  const [observeRequest, setObserveRequest] = useState<StockRouteRequest | null>(null);
  const [newsRequest, setNewsRequest] = useState<StockRouteRequest | null>(null);
  const [backtestRouteRequest, setBacktestRouteRequest] = useState<BacktestRouteRequest | null>(null);

  // Persistent state
  const [watchlistLocalSnapshot] = useState<WatchlistItem[]>(() => loadLocalWatchlistSnapshot());
  const [watchlist, setWatchlistState] = useState<WatchlistItem[]>(watchlistLocalSnapshot);
  const researchRefreshStateRef = useRef({ lastRun: 0, running: false, signature: "" });
  const [storedLlmSettings, setStoredLlmSettings] = useLocalStorage<LlmSettings | null>(
    "stock-optimizer-llm-settings",
    null,
    sanitizePersistedLlmSettings,
  );
  const [llmSettings, setLlmSettingsState] = useState<LlmSettings | null>(() => storedLlmSettings);

  const setWatchlist = useCallback(createPersistentWatchlistSetter(setWatchlistState), []);

  useEffect(() => {
    void loadPersistentWatchlist(watchlistLocalSnapshot, setWatchlistState);
  }, [watchlistLocalSnapshot]);

  useEffect(() => {
    let disposed = false;
    const signature = watchlist.map((item) => item.code.trim().toUpperCase()).sort().join("|");
    if (researchRefreshStateRef.current.signature !== signature) {
      researchRefreshStateRef.current.signature = signature;
      researchRefreshStateRef.current.lastRun = 0;
    }
    const maybeRefresh = async () => {
      const state = researchRefreshStateRef.current;
      if (disposed || state.running || !watchlist.length
        || document.visibilityState !== "visible" || !navigator.onLine) return;
      const now = Date.now();
      if (now - state.lastRun < RESEARCH_REFRESH_INTERVAL_MS) return;
      state.running = true;
      state.lastRun = now;
      try {
        const result = await refreshResearchWatchlist(watchlist);
        if (result.failed.length) {
          console.warn("background research refresh partially failed", result.failed);
        }
      } finally {
        state.running = false;
      }
    };
    const handleForeground = () => { void maybeRefresh(); };
    const timer = window.setInterval(handleForeground, 60 * 1000);
    document.addEventListener("visibilitychange", handleForeground);
    window.addEventListener("online", handleForeground);
    void maybeRefresh();
    return () => {
      disposed = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", handleForeground);
      window.removeEventListener("online", handleForeground);
    };
  }, [watchlist]);

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
    const root = document.documentElement;
    root.classList.toggle("mobile-tauri", mobile);
    root.classList.toggle("desktop-runtime", !mobile);
    onMounted?.();
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
    revealActivePanels();
  }, []);

  const observeStock = useCallback((code: string) => {
    setObserveRequest((prev) => ({ code, requestId: (prev?.requestId ?? 0) + 1 }));
    navigate("observe");
  }, [navigate]);

  const runCurrentCriteriaBacktest = useCallback((screenSpec?: AdaptiveScreenRequest) => {
    setBacktestRouteRequest((previous) => nextBacktestRouteRequest(previous, "criteria", screenSpec));
    navigate("backtest");
  }, [navigate]);

  const runWatchlistBacktest = useCallback(() => {
    setBacktestRouteRequest((previous) => nextBacktestRouteRequest(previous, "watchlist"));
    navigate("backtest");
  }, [navigate]);

  const handleBacktestRouteConsumed = useCallback((requestId: number) => {
    setBacktestRouteRequest((current) => consumeBacktestRouteRequest(current, requestId));
  }, []);

  const openNewsForStock = useCallback((code: string) => {
    setNewsRequest((prev) => ({ code, requestId: (prev?.requestId ?? 0) + 1 }));
    navigate("news");
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
              initialCode={observeRequest?.code || ""}
              initialCodeRequestId={observeRequest?.requestId ?? 0}
              mobileRuntime={mobileRuntime}
            />
          )}
          {view === "backtest" && (
            <BacktestPanel
              criteria={criteria}
              watchlist={watchlist}
              preferredSource={backtestRouteRequest}
              onPreferredSourceConsumed={handleBacktestRouteConsumed}
            />
          )}
          {view === "news" && (
            <NewsRagPanel
              llmSettings={llmSettings}
              onLlmSettingsChange={setLlmSettings}
              watchlist={watchlist}
              initialCode={newsRequest?.code || ""}
              initialCodeRequestId={newsRequest?.requestId ?? 0}
            />
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

        {view !== "agent" && view !== "news" && (
          <WatchlistPanel
            items={watchlist}
            onChange={setWatchlist}
            onObserve={observeStock}
            onNews={openNewsForStock}
            onBacktest={runWatchlistBacktest}
          />
        )}

      </main>
    </div>
  );
}
