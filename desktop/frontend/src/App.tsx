import { useCallback, useEffect, useState } from "react";
import type { PanelKey } from "./types";
import { useTheme } from "./hooks/useTheme";
import { useLocalStorage } from "./hooks/useLocalStorage";
import { isMobileTauriRuntime } from "./lib/tauri";
import { Header } from "./components/Header";
import { Sidebar } from "./components/Sidebar";
import { FilterBar } from "./components/FilterBar";
import { ScreenPanel } from "./components/panels/ScreenPanel";
import { ObservePanel } from "./components/panels/ObservePanel";
import { BacktestPanel } from "./components/panels/BacktestPanel";
import { NewsRagPanel } from "./components/panels/NewsRagPanel";
import { AgentPanel } from "./components/panels/AgentPanel";
import { DataSourceBar } from "./components/DataSourceBar";
import { WatchlistPanel } from "./components/panels/WatchlistPanel";
import { LlmSettingsPanel } from "./components/panels/LlmSettingsPanel";
import type { WatchlistItem, LlmSettings } from "./types";

type ViewKey = "screen" | "observe" | "backtest" | "news" | "agent";

const VIEW_TO_PANELS: Record<ViewKey, PanelKey[]> = {
  screen: ["screen", "sectorScreen", "boardScreen", "graph", "trendAnalyze", "trendScreen"],
  observe: ["observe"],
  backtest: ["backtest"],
  news: ["newsRag", "ragPackBuild", "ragPackQuery", "upstreamScan", "upstreamImport"],
  agent: ["agent"],
};

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

  const [criteriaOpen, setCriteriaOpen] = useState(false);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [mobileRuntime, setMobileRuntime] = useState(false);
  const [observeCode, setObserveCode] = useState("");
  const [backtestPreferredSource, setBacktestPreferredSource] = useState<"criteria" | "watchlist" | null>(null);

  // Persistent state
  const [watchlist, setWatchlist] = useLocalStorage<WatchlistItem[]>("stock-optimizer-watchlist", []);
  const [llmSettings, setLlmSettings] = useLocalStorage<LlmSettings | null>("stock-optimizer-llm-settings", null);

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
  });

  useEffect(() => {
    setMobileRuntime(isMobileTauriRuntime());
    document.documentElement.classList.toggle("mobile-tauri", isMobileTauriRuntime());
    document.documentElement.classList.toggle("desktop-runtime", !isMobileTauriRuntime());
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
        setCriteriaOpen(false);
      }
    };
    document.addEventListener("keydown", onKeydown);
    return () => document.removeEventListener("keydown", onKeydown);
  }, []);

  const navigate = useCallback((v: ViewKey) => {
    setView(v);
    setMobileNavOpen(false);
    setCriteriaOpen(false);
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
        <FilterBar
          criteria={criteria}
          onChange={setCriteria}
          open={criteriaOpen}
          onToggle={() => setCriteriaOpen(!criteriaOpen)}
          onClose={() => setCriteriaOpen(false)}
        />

        <DataSourceBar mobileRuntime={mobileRuntime} />

        <div className="panels">
          {view === "screen" && (
            <ScreenPanel
              criteria={criteria}
              onCriteriaChange={setCriteria}
              watchlist={watchlist}
              onWatchlistChange={setWatchlist}
              onObserveStock={observeStock}
              onRunBacktest={runCurrentCriteriaBacktest}
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
            />
          )}
        </div>

        <WatchlistPanel
          items={watchlist}
          onChange={setWatchlist}
        />

        <LlmSettingsPanel
          settings={llmSettings}
          onChange={setLlmSettings}
        />
      </main>
    </div>
  );
}
