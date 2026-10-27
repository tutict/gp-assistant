import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTheme } from "./hooks/useTheme";
import { useDensity } from "./hooks/useDensity";
import { useFontScale } from "./hooks/useFontScale";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { useWorkspaceState, useWorkspaceStore, useWorkspaceStatus, WorkspaceCredentialsReady } from "./hooks/useWorkspace";
import { WorkspaceErrorBoundary } from "./components/ui/ErrorBoundary";
import { getJson, isMobileTauriRuntime, isTauriRuntime, postJson } from "./lib/tauri";
import { createPersistentWatchlistSetter, filterWatchlistByMarket, loadLocalWatchlistSnapshot, loadPersistentWatchlist } from "./lib/watchlistStore";
import { useLlmCredentials } from "./hooks/useLlmCredentials";
import { buildScreenCriteria } from "./lib/contracts";
import { createSettingsRegistry } from "./lib/settingsRegistry";
import { DEFAULT_FILTER_CRITERIA } from "./lib/screenCriteria";
import { Header } from "./components/Header";
import { Sidebar } from "./components/Sidebar";
import { FilterBar, type FilterCriteria } from "./components/FilterBar";
import { ScreenPanel } from "./components/panels/ScreenPanel";
import { WatchlistPanel } from "./components/panels/WatchlistPanel";
import { HkMarketPanel } from "./components/panels/HkMarketPanel";
import { HkObservePanel } from "./components/panels/HkObservePanel";
import { HkUnavailablePanel } from "./components/panels/HkUnavailablePanel";
import { PanelFeedback } from "./components/ui/PanelFeedback";
import type { AdaptiveScreenRequest, DataStatus, ScreenCriteria, WatchlistItem } from "./types";
import type { CapabilityId, MarketCapability, MarketId } from "./lib/marketDomain";
import { marketTransition, shouldApplyMarketCapabilities } from "./lib/marketCapabilities";
import {
  consumeBacktestRouteRequest,
  nextBacktestRouteRequest,
  revealActivePanels,
  type BacktestRouteRequest,
} from "./lib/viewNavigation";

type ViewKey = "screen" | "observe" | "backtest" | "news" | "agent";

function viewFromHash(hash: string): ViewKey {
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
}

type ResearchView = "screen" | "observe" | "backtest";

function isResearchView(view: ViewKey): view is ResearchView {
  return view === "screen" || view === "observe" || view === "backtest";
}
type StockRouteRequest = { code: string; requestId: number };
type NewsRouteRequest = StockRouteRequest & { view?: "sources" | "sentiment" };
type AgentDraftRequest = { prompt: string; requestId: number };

const loadObservePanel = () => import("./components/panels/ObservePanel");
const loadBacktestPanel = () => import("./components/panels/BacktestPanel");
const loadNewsRagPanel = () => import("./components/panels/SentimentPanel");
const loadAgentPanel = () => import("./components/panels/AgentPanel");
const ObservePanel = lazy(async () => ({ default: (await loadObservePanel()).ObservePanel }));
const BacktestPanel = lazy(async () => ({ default: (await loadBacktestPanel()).BacktestPanel }));
const NewsRagPanel = lazy(async () => ({ default: (await loadNewsRagPanel()).SentimentPanel }));
const AgentPanel = lazy(async () => ({ default: (await loadAgentPanel()).AgentPanel }));
const adjacentPanelLoaders: Record<ViewKey, Array<() => Promise<unknown>>> = {
  screen: [loadObservePanel],
  observe: [loadBacktestPanel],
  backtest: [loadObservePanel, loadNewsRagPanel],
  news: [loadBacktestPanel, loadAgentPanel],
  agent: [loadNewsRagPanel],
};

interface AppProps {
  onMounted?: () => void;
}

export default function App({ onMounted }: AppProps) {
  const { theme, setTheme, toggleTheme } = useTheme();
  const { density, setDensity } = useDensity();
  const { fontScale, setFontScale } = useFontScale();
  const workspace = useWorkspaceStore();
  const workspaceStatus = useWorkspaceStatus();
  const [backtestDefaultApplied, setBacktestDefaultApplied] = useState(false);
  const [view, setView] = useWorkspaceState<ViewKey>("app.view", viewFromHash(window.location.hash));
  const [selectedStock, setSelectedStock] = useWorkspaceState("app.stock", "");
  const [market, setMarket] = useWorkspaceState<MarketId>("app.market", "cn");
  const marketRef = useRef(market);
  marketRef.current = market;
  const [backtestSource, setBacktestSource] = useWorkspaceState<"criteria" | "watchlist">("app.backtest.source", "criteria");
  const [visitedResearch, setVisitedResearch] = useState<Record<ResearchView, boolean>>(() => {
    const initial = viewFromHash(window.location.hash);
    return {
      screen: initial === "screen",
      observe: initial === "observe",
      backtest: initial === "backtest",
    };
  });
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [mobileRuntime, setMobileRuntime] = useState(() => isMobileTauriRuntime());
  const [observeRequest, setObserveRequest] = useState<StockRouteRequest | null>(null);
  const [newsRequest, setNewsRequest] = useState<NewsRouteRequest | null>(null);
  const [agentDraftRequest, setAgentDraftRequest] = useState<AgentDraftRequest | null>(null);
  const [backtestRouteRequest, setBacktestRouteRequest] = useState<BacktestRouteRequest | null>(null);
  const [searchCode, setSearchCode] = useState("");
  const [shortcutHelpOpen, setShortcutHelpOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [marketStatus, setMarketStatus] = useState<DataStatus | null>(null);
  const [marketCapabilities, setMarketCapabilities] = useState<MarketCapability[] | null>(null);
  const [marketCapabilityError, setMarketCapabilityError] = useState<string | null>(null);
  const marketCapabilitiesRequestIdRef = useRef(0);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // Persistent state
  const [watchlistLocalSnapshot] = useState<WatchlistItem[]>(() => loadLocalWatchlistSnapshot());
  const [watchlist, setWatchlistState] = useState<WatchlistItem[]>(watchlistLocalSnapshot);
  const { llmSettings, setLlmSettings: saveLlmSettings, credentialsReady, credentialError, credentialMigrationPending, retryCredentialMigration } = useLlmCredentials();
  const setLlmSettings = useCallback<typeof saveLlmSettings>(async (value) => {
    if (!credentialsReady) throw new Error("密钥存储仍在初始化，请稍后再试。");
    await saveLlmSettings(value);
  }, [credentialsReady, saveLlmSettings]);

  const setWatchlist = useCallback(createPersistentWatchlistSetter(setWatchlistState), []);
  const visibleWatchlist = useMemo(() => filterWatchlistByMarket(watchlist, market), [market, watchlist]);

  useEffect(() => {
    void loadPersistentWatchlist(watchlistLocalSnapshot, setWatchlistState);
  }, [watchlistLocalSnapshot]);

  useEffect(() => {
    let disposed = false;
    let resuming = false;
    const resumeExplicitRuns = () => {
      if (!isTauriRuntime() || disposed || resuming || document.visibilityState !== "visible" || !navigator.onLine) return;
      resuming = true;
      void postJson("/api/data/resume-explicit", {})
        .catch((error) => console.warn("explicit external data recovery failed", error))
        .finally(() => { resuming = false; });
    };
    document.addEventListener("visibilitychange", resumeExplicitRuns);
    window.addEventListener("online", resumeExplicitRuns);
    resumeExplicitRuns();
    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", resumeExplicitRuns);
      window.removeEventListener("online", resumeExplicitRuns);
    };
  }, []);

  // Filter criteria state
  const [criteria, setCriteria] = useWorkspaceState("app.criteria", DEFAULT_FILTER_CRITERIA);

  useEffect(() => {
    const mobile = isMobileTauriRuntime();
    setMobileRuntime(mobile);
    const root = document.documentElement;
    root.classList.toggle("mobile-tauri", mobile);
    root.classList.toggle("desktop-runtime", !mobile);
    onMounted?.();
  }, [onMounted]);

  useEffect(() => {
    let cancelled = false;
    if (market === "hk") {
      setMarketStatus(null);
      return () => { cancelled = true; };
    }
    void getJson<DataStatus>("/api/data-sources/status")
      .then((status) => { if (!cancelled) setMarketStatus(status); })
      .catch(() => { if (!cancelled) setMarketStatus(null); });
    return () => {
      cancelled = true;
    };
  }, [market]);

  useEffect(() => {
    const requestId = ++marketCapabilitiesRequestIdRef.current;
    let cancelled = false;
    setMarketCapabilities(null);
    setMarketCapabilityError(null);
    void getJson<MarketCapability[]>(`/api/market/capabilities?market=${market}`)
      .then((capabilities) => {
        if (cancelled || !shouldApplyMarketCapabilities(market, marketRef.current, requestId, marketCapabilitiesRequestIdRef.current)) return;
        setMarketCapabilities(capabilities);
      })
      .catch((error) => {
        if (cancelled || !shouldApplyMarketCapabilities(market, marketRef.current, requestId, marketCapabilitiesRequestIdRef.current)) return;
        setMarketCapabilityError(error instanceof Error ? error.message : String(error));
      });
    return () => { cancelled = true; };
  }, [market]);

  // Hash routing
  useEffect(() => {
    const onHashChange = () => {
      const next = viewFromHash(window.location.hash);
      setView(next);
      void workspace.flush();
      if (isResearchView(next)) {
        setVisitedResearch((current) => current[next] ? current : { ...current, [next]: true });
      }
    };
    window.addEventListener("hashchange", onHashChange);
    if (window.location.hash) onHashChange();
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [setView, workspace]);


  const navigate = useCallback((v: ViewKey) => {
    setView(v);
    void workspace.flush();
    if (isResearchView(v)) {
      setVisitedResearch((current) => current[v] ? current : { ...current, [v]: true });
    }
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
  }, [setView, workspace]);

  useEffect(() => {
    if (isResearchView(view)) setVisitedResearch(current => current[view] ? current : { ...current, [view]: true });
  }, [view]);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      window.scrollTo({ top: 0, left: 0, behavior: "auto" });
      document.querySelector<HTMLElement>(".workbench")?.scrollTo({ top: 0, left: 0, behavior: "auto" });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [view]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      for (const loadPanel of adjacentPanelLoaders[view]) void loadPanel();
    }, 240);
    return () => window.clearTimeout(timer);
  }, [view]);

  const observeStock = useCallback((code: string) => {
    setSelectedStock(code);
    setObserveRequest((prev) => ({ code, requestId: (prev?.requestId ?? 0) + 1 }));
    navigate("observe");
  }, [navigate, setSelectedStock]);

  const changeMarket = useCallback((next: MarketId) => {
    const transition = marketTransition(market, next);
    if (!transition) return;
    marketRef.current = transition.market;
    if (transition.clearCapabilities) {
      marketCapabilitiesRequestIdRef.current += 1;
      setMarketCapabilities(null);
      setMarketCapabilityError(null);
    }
    setMarket(transition.market);
    if (transition.clearSelection) setSelectedStock("");
    if (transition.clearObserveRequest) setObserveRequest(null);
    if (transition.clearNewsRequest) setNewsRequest(null);
    if (transition.clearBacktestRequest) setBacktestRouteRequest(null);
    setMarketStatus(null);
  }, [market, setMarket, setSelectedStock]);

  const marketCapability = useCallback((id: CapabilityId) =>
    marketCapabilities?.find((item) => item.id === id) || null,
  [marketCapabilities]);

  const runCurrentCriteriaBacktest = useCallback((screenSpec?: AdaptiveScreenRequest, criteriaSnapshot?: FilterCriteria) => {
    setBacktestSource("criteria");
    setBacktestRouteRequest((previous) => nextBacktestRouteRequest(
      previous,
      "criteria",
      screenSpec,
      criteriaSnapshot,
    ));
    navigate("backtest");
  }, [navigate, setBacktestSource]);

  const runWatchlistBacktest = useCallback(() => {
    setBacktestSource("watchlist");
    setBacktestRouteRequest((previous) => nextBacktestRouteRequest(previous, "watchlist"));
    navigate("backtest");
  }, [navigate, setBacktestSource]);

  const handleBacktestRouteConsumed = useCallback((requestId: number) => {
    setBacktestDefaultApplied(true);
    setBacktestRouteRequest((current) => consumeBacktestRouteRequest(current, requestId));
  }, []);

  const openNewsForStock = useCallback((code: string) => {
    setSelectedStock(code);
    setNewsRequest((prev) => ({ code, requestId: (prev?.requestId ?? 0) + 1, view: "sources" }));
    navigate("news");
  }, [navigate, setSelectedStock]);
  const applyEvolutionCriteria = useCallback((next: ScreenCriteria) => {
    setCriteria((previous) => ({
      ...previous,
      minRoe: next.min_roe == null ? previous.minRoe : String(next.min_roe * 100),
      maxPe: next.max_pe == null ? previous.maxPe : String(next.max_pe),
      maxPb: next.max_pb == null ? previous.maxPb : String(next.max_pb),
      minMcap: next.min_market_cap_billion == null ? previous.minMcap : String(next.min_market_cap_billion),
      industry: next.industry ?? previous.industry,
      marketScope: next.market_scope ?? previous.marketScope,
      resultLimit: next.limit ?? previous.resultLimit,
      scoreProfile: next.score_profile ?? previous.scoreProfile,
    }));
  }, []);
  const handoffToAgent = useCallback((prompt: string) => {
    setAgentDraftRequest((prev) => ({ prompt, requestId: (prev?.requestId ?? 0) + 1 }));
    navigate("agent");
  }, [navigate]);


  const focusGlobalSearch = useCallback(() => {
    searchInputRef.current?.focus();
    searchInputRef.current?.select();
  }, []);

  const handleGlobalSearchCommit = useCallback((code: string) => {
    const normalizedCode = code.trim().toUpperCase();
    if (!normalizedCode) return;
    setSearchCode(normalizedCode);
    observeStock(normalizedCode);
  }, [observeStock]);

  const toggleShortcutHelp = useCallback(() => {
    setSettingsOpen(false);
    setShortcutHelpOpen((open) => !open);
  }, []);

  const toggleSettings = useCallback(() => {
    setShortcutHelpOpen(false);
    setSettingsOpen((open) => !open);
  }, []);

  const settings = useMemo(() => createSettingsRegistry({
    density,
    setDensity,
    theme,
    setTheme,
    fontScale,
    setFontScale,
    market,
    setMarket: changeMarket,
  }), [changeMarket, density, fontScale, market, setDensity, setFontScale, setTheme, theme]);

  const closeOverlays = useCallback(() => {
    setShortcutHelpOpen(false);
    setSettingsOpen(false);
    setMobileNavOpen(false);
    if (document.activeElement === searchInputRef.current) searchInputRef.current?.blur();
  }, []);

  useGlobalShortcuts({
    onFocusSearch: focusGlobalSearch,
    onNavigate: navigate,
    onToggleHelp: toggleShortcutHelp,
    onEscape: closeOverlays,
  });

  return (
    <WorkspaceCredentialsReady.Provider value={credentialsReady}>
    <div className={`app ${mobileNavOpen ? "mobile-nav-open" : ""}`} data-active-view={view}>
      <Header
        theme={theme}
        searchCode={searchCode}
        searchInputRef={searchInputRef}
        watchlistCount={watchlist.length}
        dataStatus={marketStatus}
        shortcutHelpOpen={shortcutHelpOpen}
        settingsOpen={settingsOpen}
        settings={settings}
        onSearchCodeChange={setSearchCode}
        onSearchCommit={handleGlobalSearchCommit}
        onToggleHelp={toggleShortcutHelp}
        onToggleSettings={toggleSettings}
        onToggleTheme={toggleTheme}
        onToggleMobileNav={() => setMobileNavOpen(true)}
        llmSettings={llmSettings}
        onLlmSettingsChange={(settings) => setLlmSettings(settings)}
      />

      <Sidebar
        view={view}
        onNavigate={navigate}
        open={mobileNavOpen}
        onClose={() => setMobileNavOpen(false)}
      />

      <main className="workbench">
        <div className="workbench-notices">
      {!credentialsReady && <p role="status">密钥正在初始化，可继续本地编辑；模型提交与设置保存暂不可用。</p>}
      {credentialError && <PanelFeedback kind="error" title="密钥保存状态" description={credentialError} action={credentialMigrationPending ? <button type="button" onClick={() => void retryCredentialMigration()}>重试安全迁移</button> : undefined} />}
        </div>
        {view === "screen" && (
          market === "hk" ? null : <FilterBar status={marketStatus} onStatusChange={setMarketStatus} />
        )}

        <div className="panels">
          {visitedResearch.screen && (
            <div className="persistent-panel" hidden={view !== "screen"}>
              {market === "hk" ? <div className="hk-market-workspace">
                <HkMarketPanel onObserveStock={observeStock} capability={marketCapability("security_directory")} capabilityLoading={marketCapabilities === null && !marketCapabilityError} capabilityError={marketCapabilityError} />
                <HkUnavailablePanel feature="港股筛选" capability={marketCapability("screening")} loading={marketCapabilities === null && !marketCapabilityError} error={marketCapabilityError} onSwitchMarket={() => changeMarket("cn")} />
              </div> : <WorkspaceErrorBoundary label="选股"><ScreenPanel
                criteria={criteria} onCriteriaChange={setCriteria} watchlist={visibleWatchlist} onWatchlistChange={setWatchlist}
                onObserveStock={observeStock} onNewsStock={openNewsForStock} onRunBacktest={runCurrentCriteriaBacktest} mobileRuntime={mobileRuntime} marketStatus={marketStatus}
              /></WorkspaceErrorBoundary>}
            </div>
          )}
          {visitedResearch.observe && (
            <Suspense fallback={view === "observe" ? <PanelFeedback kind="loading" description="正在加载工作区..." /> : null}>
              <div className="persistent-panel" hidden={view !== "observe"}>
                {market === "hk" ? marketCapabilities === null || marketCapability("observe")?.status === "unavailable"
                  ? <HkUnavailablePanel feature="港股普通股观察" capability={marketCapability("observe")} additionalCapabilities={[marketCapability("financial_snapshot")].filter((item): item is MarketCapability => Boolean(item))} loading={marketCapabilities === null && !marketCapabilityError} error={marketCapabilityError} onSwitchMarket={() => changeMarket("cn")} />
                  : <HkObservePanel code={observeRequest?.code || selectedStock} watchlist={visibleWatchlist} onWatchlistChange={(items) => setWatchlist([...items, ...watchlist.filter((item) => !visibleWatchlist.some((visible) => visible.code === item.code))])} /> : <WorkspaceErrorBoundary label="观察"><ObservePanel
                  watchlist={visibleWatchlist} onWatchlistChange={setWatchlist} initialCode={observeRequest?.code || selectedStock} initialCodeRequestId={observeRequest?.requestId ?? 0}
                  mobileRuntime={mobileRuntime} onOpenNews={openNewsForStock} onAskAgent={handoffToAgent}
                  capability={marketCapability("observe") || undefined}
                /></WorkspaceErrorBoundary>}
              </div>
            </Suspense>
          )}
          {visitedResearch.backtest && (
            <Suspense fallback={view === "backtest" ? <PanelFeedback kind="loading" description="正在加载工作区..." /> : null}>
              <div className="persistent-panel" hidden={view !== "backtest"}>
                {market === "hk" ? <HkUnavailablePanel feature="港股回测" capability={marketCapability("backtest")} loading={marketCapabilities === null && !marketCapabilityError} error={marketCapabilityError} onSwitchMarket={() => changeMarket("cn")} /> : <WorkspaceErrorBoundary label="回测"><BacktestPanel
                  criteria={criteria} watchlist={visibleWatchlist} preferredSource={backtestRouteRequest ?? (workspaceStatus.ready && !backtestDefaultApplied ? { source: backtestSource, requestId: -1 } : null)} onPreferredSourceConsumed={handleBacktestRouteConsumed}
                /></WorkspaceErrorBoundary>}
              </div>
            </Suspense>
          )}
          {view === "news" && market === "hk" && <HkUnavailablePanel feature="港股新闻、RAG 与情绪研究" capability={marketCapability("news_research")} additionalCapabilities={[marketCapability("sentiment")].filter((item): item is MarketCapability => Boolean(item))} loading={marketCapabilities === null && !marketCapabilityError} error={marketCapabilityError} onSwitchMarket={() => changeMarket("cn")} />}
          {view === "news" && market === "cn" && (
            <Suspense fallback={<PanelFeedback kind="loading" description="正在加载工作区..." />}>
              <WorkspaceErrorBoundary label="消息研究"><NewsRagPanel
                llmSettings={llmSettings}
                onLlmSettingsChange={setLlmSettings}
                watchlist={visibleWatchlist}
                initialCode={newsRequest?.code || selectedStock}
                initialCodeRequestId={newsRequest?.requestId ?? 0}
                initialView={newsRequest?.view}
                onAskAgent={handoffToAgent}
                onGoToScreen={() => navigate("screen")}
                criteria={buildScreenCriteria(criteria)}
                onApplyScreenCriteria={applyEvolutionCriteria}
              /></WorkspaceErrorBoundary>
            </Suspense>
          )}
          {view === "agent" && market === "hk" && <HkUnavailablePanel feature="港股研究 Agent" capability={marketCapability("research_agent")} loading={marketCapabilities === null && !marketCapabilityError} error={marketCapabilityError} onSwitchMarket={() => changeMarket("cn")} />}
          {view === "agent" && market === "cn" && (
            <Suspense fallback={<PanelFeedback kind="loading" description="正在加载工作区..." />}>
              <WorkspaceErrorBoundary label="智能体"><AgentPanel
                llmSettings={llmSettings}
                onLlmSettingsChange={setLlmSettings}
                watchlist={visibleWatchlist}
                onWatchlistChange={setWatchlist}
                draftPrompt={agentDraftRequest?.prompt || ""}
                draftRequestId={agentDraftRequest?.requestId ?? 0}
              /></WorkspaceErrorBoundary>
            </Suspense>
          )}
        </div>

        {market === "hk" && view === "screen" && <WorkspaceErrorBoundary label="港股自选"><WatchlistPanel items={visibleWatchlist} onChange={(items) => setWatchlist([...items, ...watchlist.filter((item) => !visibleWatchlist.some((visible) => visible.code === item.code))])} onObserve={observeStock} /></WorkspaceErrorBoundary>}
        {market === "cn" && view !== "agent" && view !== "news" && (
          <WorkspaceErrorBoundary label="自选股"><WatchlistPanel
            items={visibleWatchlist}
            onChange={setWatchlist}
            onObserve={observeStock}
            onNews={openNewsForStock}
            onBacktest={runWatchlistBacktest}
          /></WorkspaceErrorBoundary>
        )}

      </main>
    </div>
    </WorkspaceCredentialsReady.Provider>
  );
}
