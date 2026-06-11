const $ = (selector) => document.querySelector(selector);

const buttons = {
  screen: $("#screenBtn"),
  sectorScreen: $("#sectorScreenBtn"),
  graph: $("#graphBtn"),
  trendAnalyze: $("#trendAnalyzeBtn"),
  trendScreen: $("#trendScreenBtn"),
  backtest: $("#backtestBtn"),
  newsRag: $("#newsRagBtn"),
  ragPackBuild: $("#ragPackBuildBtn"),
  ragPackQuery: $("#ragPackQueryBtn"),
  upstreamScan: $("#upstreamScanBtn"),
  upstreamImport: $("#upstreamImportBtn"),
  agent: $("#agentBtn"),
  observe: $("#observeBtn"),
};

const panels = {
  screen: $("#screenResult"),
  graph: $("#graphResult"),
  trend: $("#trendResult"),
  backtest: $("#backtestResult"),
  newsRag: $("#newsRagResult"),
  agent: $("#agentResult"),
  observe: $("#observeResult"),
};

const themeToggle = $("#themeToggle");
const themeText = $("#themeText");
const THEME_KEY = "gp-assistant-theme";
const DATA_SOURCE_KEY = "gp-assistant-data-source";
const DATA_REFRESH_KEY = "gp-assistant-source-refresh";
const DATA_PROXY_KEY = "gp-assistant-proxy-mode";
const AUTO_REFRESH_CHECK_KEY = "gp-assistant-auto-refresh-last-check";
const AUTO_REFRESH_CHECK_INTERVAL_MS = 30 * 60 * 1000;
const LLM_SETTINGS_KEY = "gp-assistant-llm-settings";
const WATCHLIST_KEY = "gp-assistant-watchlist";
const DEFAULT_RESULT_LIMIT = 10;
const DEFAULT_SECTOR_GROUP_LIMIT = 12;
const DEFAULT_PER_SECTOR_LIMIT = 3;
const SECTOR_SCREEN_POOL_MULTIPLIER = 8;
const STOCK_SEARCH_LIMIT = 3;
const DEFAULT_DATA_SOURCE = "tdx";
const MOBILE_TENCENT_MAX_CANDIDATES = 16000;
const MOBILE_TENCENT_MAX_FAILED_BATCHES = 4;
const MOBILE_TENCENT_MAX_REFRESH_SECS = 45;
const DEFAULT_TODAY_DATE_INPUT_IDS = new Set(["trendEnd", "btEnd", "observeEnd"]);
let mobileMarketDataPromise = null;
let mobileMarketDataSummary = null;
let mobileMarketDataMeta = null;
let autoRefreshInFlight = false;
const dataSource = {
  select: $("#dataSourceSelect"),
  refresh: $("#refreshSource"),
  proxy: $("#proxyModeSelect"),
  status: $("#sourceStatus"),
  universe: $("#universeCount"),
  cacheLabel: $("#cacheMetricLabel"),
  cache: $("#cacheBytes"),
  updated: $("#universeUpdated"),
  policy: $("#cachePolicy"),
  note: $("#maintenanceNote"),
  refreshUniverse: $("#refreshUniverseBtn"),
  pruneCache: $("#pruneCacheBtn"),
  progress: $("#refreshProgress"),
  progressLabel: $("#refreshProgressLabel"),
  progressValue: $("#refreshProgressValue"),
  progressBar: $("#refreshProgressBar"),
};
const mobileNav = {
  toggle: $("#mobileNavToggle"),
  close: $("#mobileNavClose"),
  panel: $("#mobileNav"),
  overlay: $("#mobileNavOverlay"),
  links: document.querySelectorAll("[data-mobile-nav-link]"),
  bottomLinks: document.querySelectorAll("[data-bottom-nav-link]"),
};
const workbench = {
  root: $(".workbench"),
  navLinks: document.querySelectorAll("[data-workbench-nav]"),
  viewLinks: document.querySelectorAll("[data-view-link]"),
  criteriaOpen: $("#openCriteriaBtn"),
  criteriaClose: $("#closeCriteriaBtn"),
  criteriaOverlay: $("#criteriaOverlay"),
  criteriaSummary: $("#criteriaSummary"),
};
const watchlistUi = {
  source: $("#btSource"),
  sourceOptions: document.querySelectorAll("[data-backtest-source-option]"),
  panel: $("#watchlistPanel"),
  count: $("#watchlistCount"),
  items: $("#watchlistItems"),
  empty: $("#watchlistEmpty"),
  clear: $("#clearWatchlistBtn"),
};
const llmSettings = {
  apiKey: $("#llmApiKey"),
  baseUrl: $("#llmBaseUrl"),
  model: $("#llmModel"),
  temperature: $("#llmTemperature"),
  timeout: $("#llmTimeout"),
  jsonMode: $("#llmJsonMode"),
  rememberKey: $("#llmRememberKey"),
  status: $("#llmStatus"),
  save: $("#llmSaveBtn"),
  clear: $("#llmClearBtn"),
};

const TAURI_MOBILE_GET_ROUTES = {
  "/api/data-sources/status": async ({ invoke }) => mobileDataStatus(invoke),
  "/api/upstream-rag/mobile/list": async ({ invoke }) => invoke("core_upstream_rag_list"),
  "/api/upstream-rag/mobile/detail": async ({ invoke, parsed }) =>
    invoke("core_upstream_rag_detail", {
      payload: {
        stock_code: parsed.searchParams.get("stock_code") || "",
        pack_version: parsed.searchParams.get("pack_version") || "",
      },
    }),
  "/api/stock-search": async ({ invoke, parsed }) => searchTauriStocks(invoke, parsed.searchParams),
};

const TAURI_MOBILE_GET_PREFIX_ROUTES = [
  {
    prefix: "/api/observe/",
    handler: async ({ invoke, path, parsed }) => {
      const code = decodeURIComponent(path.slice("/api/observe/".length));
      return observeTauriStock(invoke, code, parsed.searchParams);
    },
  },
];

const TAURI_MOBILE_POST_ROUTES = {
  "/api/screen": async ({ invoke, payload }) =>
    invokeCoreWithMobileData(invoke, "core_screen_with_data", "criteria", payload),
  "/api/data-sources/auto-refresh-universe": async ({ invoke }) => {
    const cache = await readMobileMarketDataCache(invoke).catch(() => null);
    const tradingDay = isLikelyTradingDay();
    if (!cache) {
      const status = await refreshMobileMarketData(invoke, { seed: null });
      return {
        source: DEFAULT_DATA_SOURCE,
        checked_at: new Date().toISOString(),
        trading_day: tradingDay,
        after_close: !shouldUsePreviousCloseForMobileRefresh(),
        due: true,
        refreshed: true,
        initial_refresh: true,
        status,
        notes: mobileRefreshNotes(status, "首次安装已联网生成手机本地股票池。"),
      };
    }
    if (!tradingDay) {
      return {
        source: DEFAULT_DATA_SOURCE,
        checked_at: new Date().toISOString(),
        trading_day: false,
        after_close: false,
        due: false,
        refreshed: false,
        status: await mobileDataStatus(invoke),
        notes: ["今天不是交易日，移动端不自动刷新股票池。"],
      };
    }
    const status = await refreshMobileMarketData(invoke);
    return {
      source: DEFAULT_DATA_SOURCE,
      checked_at: new Date().toISOString(),
      trading_day: true,
      after_close: !shouldUsePreviousCloseForMobileRefresh(),
      due: true,
      refreshed: true,
      status,
      notes: mobileRefreshNotes(status, "移动端已按交易日策略联网更新股票池。"),
    };
  },
  "/api/data-sources/refresh-universe": async ({ invoke }) => {
    const status = await refreshMobileMarketData(invoke);
    return {
      source: DEFAULT_DATA_SOURCE,
      refreshed: true,
      status,
      notes: mobileRefreshNotes(status, "已通过腾讯行情联网更新股票池，并写入手机本地缓存。"),
    };
  },
  "/api/data-sources/prune-cache": async ({ invoke }) => pruneMobileMarketData(invoke),
  "/api/sector-screen": async ({ invoke, payload }) => buildTauriSectorScreen(invoke, payload),
  "/api/graph-screen": async ({ invoke, payload }) =>
    invokeCoreWithMobileData(invoke, "core_graph_screen_with_data", "request", payload),
  "/api/trend": async ({ invoke, payload }) =>
    invokeCoreWithMobileData(invoke, "core_trend_with_data", "request", payload),
  "/api/trend-screen": async ({ invoke, payload }) =>
    invokeCoreWithMobileData(invoke, "core_trend_screen_with_data", "request", payload),
  "/api/backtest": async ({ invoke, payload }) =>
    invokeCoreWithMobileData(invoke, "core_backtest_with_data", "request", payload),
  "/api/agent": async ({ invoke, payload }) =>
    invokeCoreWithMobileData(invoke, "core_agent_with_data", "message", payload?.message || ""),
  "/api/news-rag": async ({ invoke, payload }) => analyzeMobileNewsRag(invoke, payload),
  "/api/upstream-rag/mobile/import": async ({ invoke, payload }) => invoke("core_upstream_rag_import", { payload }),
  "/api/upstream-rag/mobile/detail": async ({ invoke, payload }) => invoke("core_upstream_rag_detail", { payload }),
  "/api/upstream-rag/mobile/rollback": async ({ invoke, payload }) => invoke("core_upstream_rag_rollback", { payload }),
};

async function invokeCoreWithMobileData(invoke, command, payloadKey, payloadValue) {
  return invoke(command, {
    payload: {
      data: await loadMobileMarketData(invoke),
      [payloadKey]: payloadValue,
    },
  });
}

async function analyzeMobileNewsRag(invoke, payload = {}) {
  const requestedCode = normalizeStockCode(payload?.code || payload?.seed_codes?.[0] || "");
  const detail = await loadMobileNewsRagManifest(invoke, requestedCode);
  const manifest = detail?.manifest || detail || {};
  const manifestCode = normalizeStockCode(manifest.target_stock_code || "");

  if (requestedCode && manifestCode && stockCodeDigits(requestedCode) !== stockCodeDigits(manifestCode)) {
    throw new Error(`当前手机 RAG 包是 ${manifestCode}，不是 ${requestedCode}。请先导入目标股票的同步包。`);
  }

  const sources = mobileNewsRagSourcesFromManifest(manifest, payload?.max_items || 24);
  const skill = await invoke("core_mobile_stock_skill", {
    payload: {
      stock_code: manifestCode || requestedCode,
      stock_name: manifest.target_stock_name || "",
      question: "分析上下游消息利好利空",
      sources,
    },
  });
  return mobileNewsSkillToNewsRagResult(skill, manifest, sources, detail?.notes || []);
}

async function loadMobileNewsRagManifest(invoke, requestedCode) {
  try {
    return await invoke("core_upstream_rag_detail", {
      payload: {
        stock_code: requestedCode || "",
        pack_version: "",
      },
    });
  } catch (error) {
    if (requestedCode) {
      throw new Error(`手机端没有 ${requestedCode} 的上下游 RAG 包。请先在桌面端构建同步包，再扫码导入手机。`);
    }
    throw new Error("手机端尚未导入上下游 RAG 包。请先在桌面端构建同步包，再扫码导入手机。");
  }
}

function mobileNewsRagSourcesFromManifest(manifest, maxItems = 24) {
  const sources = [];
  const relationEdges = Array.isArray(manifest.relation_edges) ? manifest.relation_edges : [];
  const evidenceChunks = Array.isArray(manifest.evidence_chunks) ? manifest.evidence_chunks : [];

  relationEdges.forEach((edge) => {
    const evidence = String(edge.evidence_text || edge.source_ref || "").trim();
    if (!evidence) return;
    sources.push({
      title: mobileRelationSourceTitle(edge),
      summary: evidence,
      source_tier: edge.source_tier || "manual_url",
      source_name: edge.source_name || "RAG 关系边",
      published_at: edge.published_at || null,
      source_url: edge.source_url || null,
      evidence,
    });
  });

  evidenceChunks.forEach((chunk) => {
    const evidence = String(chunk.evidence_text || chunk.title || "").trim();
    if (!evidence) return;
    sources.push({
      title: chunk.title || "RAG 证据片段",
      summary: evidence,
      source_tier: chunk.source_tier || "manual_url",
      source_name: chunk.source_name || "RAG 证据",
      published_at: chunk.published_at || null,
      source_url: chunk.source_url || null,
      evidence,
    });
  });

  return dedupeMobileNewsRagSources(sources).slice(0, clampInt(maxItems, 1, 80, 24));
}

function mobileRelationSourceTitle(edge) {
  const source = edge.source_entity?.entity_name || edge.source_entity?.stock_code || "上游/关联方";
  const target = edge.target_entity?.entity_name || edge.target_entity?.stock_code || "目标股票";
  const relation = relationTypeLabel(edge.relation_type);
  const status = relationStatusLabel(edge.status);
  return `${source} ${relation} ${target}（${status}）`;
}

function dedupeMobileNewsRagSources(sources) {
  const seen = new Set();
  return sources.filter((source) => {
    const key = [source.title, source.source_name, source.evidence].join("\n");
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function mobileNewsSkillToNewsRagResult(skill, manifest, sources, detailNotes) {
  const overview = skill?.overview || {};
  const code = normalizeStockCode(overview.stock_code || manifest.target_stock_code || "");
  const name = overview.stock_name || manifest.target_stock_name || "";
  const target = `${name || code || "目标股票"}${code ? `（${code}）` : ""}`;
  const findings = [
    ...mobileSkillFindingsToNewsFindings(skill?.positive_factors, "利好", target, code),
    ...mobileSkillFindingsToNewsFindings(skill?.negative_factors, "利空", target, code),
    ...mobileSkillFindingsToNewsFindings(skill?.neutral_information, "中性", target, code),
    ...mobileSkillFindingsToNewsFindings(skill?.unverified_leads, "不确定", target, code),
  ];
  const notes = uniqueCompactStrings([
    overview.summary,
    "移动端使用已导入本机 RAG 包离线分析，不会在手机端抓取公告或新闻。",
    sources.length ? "" : "当前 RAG 包没有可分析证据，请在桌面端重建包含证据片段的同步包。",
    ...(Array.isArray(skill?.notes) ? skill.notes : []),
    ...(Array.isArray(manifest.notes) ? manifest.notes : []),
    ...(Array.isArray(detailNotes) ? detailNotes : []),
  ]);
  return {
    scope_codes: code ? [code] : [],
    relation_count: manifest.relation_edge_count ?? (manifest.relation_edges || []).length ?? 0,
    message_count: sources.length,
    findings,
    notes,
  };
}

function mobileSkillFindingsToNewsFindings(items, direction, target, code) {
  return (Array.isArray(items) ? items : []).map((item) => ({
    target,
    direction,
    confidence: mobileConfidenceLabel(item.confidence),
    impact_chain: item.summary || item.risk_note || item.title || "",
    evidence: [
      {
        title: item.title || "-",
        source: item.source_name || "-",
        source_tier: item.source_tier || "manual_url",
        published_at: item.published_at || null,
        url: item.source_url || null,
        stock_codes: code ? [code] : [],
        relation_types: [],
        sentiment: mobileSentimentFromDirection(direction),
      },
    ],
    pending_checks: uniqueCompactStrings([item.risk_note]),
  }));
}

function mobileConfidenceLabel(value) {
  const score = Number(value);
  if (!Number.isFinite(score)) return "低";
  if (score >= 0.75) return "高";
  if (score >= 0.5) return "中";
  return "低";
}

function mobileSentimentFromDirection(direction) {
  if (direction === "利好") return "positive";
  if (direction === "利空") return "negative";
  if (direction === "中性") return "mixed";
  return "uncertain";
}

function uniqueCompactStrings(values) {
  return [...new Set(values.map((value) => String(value || "").trim()).filter(Boolean))];
}

initTheme();
initRuntimeSurface();
initMobileNav();
initDataSource();
initAutoRefresh();
initWatchlist();
initLlmSettings();
initFormControls();
initStickyOffsets();
bindActions();
dismissBootSplash();

function bindActions() {
  buttons.screen.addEventListener("click", () => runTask(buttons.screen, panels.screen, runScreen));
  buttons.sectorScreen?.addEventListener("click", () =>
    runTask(buttons.sectorScreen, panels.screen, runSectorScreen),
  );
  buttons.graph.addEventListener("click", () => runTask(buttons.graph, panels.graph, runGraph));
  buttons.trendAnalyze.addEventListener("click", () => runTask(buttons.trendAnalyze, panels.trend, runTrendAnalysis));
  buttons.trendScreen.addEventListener("click", () => runTask(buttons.trendScreen, panels.trend, runTrendScreen));
  buttons.backtest.addEventListener("click", () => runTask(buttons.backtest, panels.backtest, runBacktest));
  buttons.newsRag?.addEventListener("click", () => runTask(buttons.newsRag, panels.newsRag, runNewsRag));
  buttons.ragPackBuild?.addEventListener("click", () =>
    runTask(buttons.ragPackBuild, panels.newsRag, runUpstreamRagBuildAndTransfer),
  );
  buttons.ragPackQuery?.addEventListener("click", () => runTask(buttons.ragPackQuery, panels.newsRag, runUpstreamRagList));
  buttons.upstreamScan?.addEventListener("click", () => runTask(buttons.upstreamScan, panels.newsRag, runUpstreamQrScan));
  buttons.upstreamImport?.addEventListener("click", () => runTask(buttons.upstreamImport, panels.newsRag, runUpstreamRagImport));
  buttons.agent.addEventListener("click", () => runTask(buttons.agent, panels.agent, runAgent));
  buttons.observe?.addEventListener("click", () => runTask(buttons.observe, panels.observe, () => runObserve()));
  dataSource.select?.addEventListener("change", () => {
    localStorage.setItem(DATA_SOURCE_KEY, getSelectedDataSource());
    updateSourceStatus();
    loadDataStatus();
  });
  dataSource.refresh?.addEventListener("change", () => {
    localStorage.setItem(DATA_REFRESH_KEY, dataSource.refresh.checked ? "true" : "false");
    updateSourceStatus();
  });
  dataSource.proxy?.addEventListener("change", () => {
    localStorage.setItem(DATA_PROXY_KEY, getSelectedProxyMode());
    updateSourceStatus();
    loadDataStatus();
  });
  dataSource.refreshUniverse?.addEventListener("click", () => runDataTask(dataSource.refreshUniverse, refreshUniverse));
  dataSource.pruneCache?.addEventListener("click", () => runDataTask(dataSource.pruneCache, pruneCache));
  watchlistUi.source?.addEventListener("change", () => {
    setBacktestSource(watchlistUi.source.value);
  });
  watchlistUi.sourceOptions.forEach((option) => {
    option.addEventListener("click", () => setBacktestSource(option.dataset.backtestSourceOption));
  });
  watchlistUi.clear?.addEventListener("click", clearWatchlist);
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const watch = target?.closest("[data-watchlist-code]");
    if (watch) {
      event.preventDefault();
      toggleWatchlistFromElement(watch);
      return;
    }
    const action = target?.closest("[data-observe-code]");
    if (!action) return;
    event.preventDefault();
    runObserve(action.dataset.observeCode);
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const remove = target?.closest("[data-watchlist-remove]");
    if (!remove) return;
    event.preventDefault();
    removeWatchlistStock(remove.dataset.watchlistRemove);
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const action = target?.closest("[data-run-backtest]");
    if (!action) return;
    event.preventDefault();
    runBacktestFromScreen();
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const action = target?.closest("[data-empty-action]");
    if (!action) return;
    event.preventDefault();
    handlePanelEmptyAction(action.dataset.emptyAction, action);
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const detail = target?.closest("[data-upstream-detail]");
    if (!detail) return;
    event.preventDefault();
    runTask(detail, panels.newsRag, () => runUpstreamRagDetail(detail.dataset.stockCode, detail.dataset.packVersion));
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const rollback = target?.closest("[data-upstream-rollback]");
    if (!rollback) return;
    event.preventDefault();
    runTask(rollback, panels.newsRag, () => runUpstreamRagRollback(rollback.dataset.stockCode));
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const cancel = target?.closest("[data-upstream-scan-cancel]");
    if (!cancel) return;
    event.preventDefault();
    window.__gpUpstreamQrCancel?.();
  });
  llmSettings.save?.addEventListener("click", saveLlmSettings);
  llmSettings.clear?.addEventListener("click", clearLlmSettings);
  workbench.criteriaOpen?.addEventListener("click", () => setCriteriaPanelOpen(true));
  workbench.criteriaClose?.addEventListener("click", () => setCriteriaPanelOpen(false));
  workbench.criteriaOverlay?.addEventListener("click", () => setCriteriaPanelOpen(false));
  [
    llmSettings.apiKey,
    llmSettings.baseUrl,
    llmSettings.model,
    llmSettings.temperature,
    llmSettings.timeout,
    llmSettings.jsonMode,
  ].forEach((input) => input?.addEventListener("input", updateLlmStatus));
}

function dismissBootSplash() {
  window.requestAnimationFrame(() => {
    document.documentElement.classList.add("app-ready");
    $("#bootSplash")?.remove();
  });
}

function initRuntimeSurface() {
  const mobileRuntime = isMobileTauriRuntime();
  document.body.classList.toggle("mobile-tauri", mobileRuntime);
  document.body.classList.toggle("desktop-runtime", !mobileRuntime);
  if (buttons.ragPackQuery) {
    buttons.ragPackQuery.textContent = mobileRuntime ? "查看本机包" : "查看同步包";
  }
  if (mobileRuntime && dataSource.refreshUniverse) {
    dataSource.refreshUniverse.textContent = "联网更新股票池";
  }
  if (mobileRuntime && dataSource.pruneCache) {
    dataSource.pruneCache.textContent = "清理缓存";
  }
}

function initMobileNav() {
  if (!mobileNav.toggle || !mobileNav.panel || !mobileNav.overlay) return;

  mobileNav.toggle.addEventListener("click", () => setMobileNavOpen(true));
  mobileNav.close?.addEventListener("click", () => setMobileNavOpen(false));
  mobileNav.overlay.addEventListener("click", () => setMobileNavOpen(false));
  workbench.viewLinks.forEach((link) => {
    link.addEventListener("click", (event) => {
      const href = link.getAttribute("href") || "#sectionScreen";
      event.preventDefault();
      activateWorkbenchView(viewFromHref(href), { href, updateHash: true });
      setMobileNavOpen(false);
    });
  });
  window.addEventListener("hashchange", () => activateWorkbenchView(viewFromHref(window.location.hash), { updateHash: false }));
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      setMobileNavOpen(false);
      setCriteriaPanelOpen(false);
    }
  });
  activateWorkbenchView(viewFromHref(window.location.hash), { updateHash: false });
}

function setMobileNavOpen(isOpen) {
  document.body.classList.toggle("mobile-nav-open", isOpen);
  mobileNav.toggle?.setAttribute("aria-expanded", String(isOpen));
  mobileNav.panel?.setAttribute("aria-hidden", String(!isOpen));
}

function activateWorkbenchView(view = "screen", options = {}) {
  const normalized = ["screen", "observe", "backtest", "news", "agent"].includes(view) ? view : "screen";
  const href = options.href || hrefForView(normalized);
  workbench.root?.setAttribute("data-active-view", normalized);
  document.body.dataset.activeView = normalized;
  setActiveNavLink(normalized);
  if (options.updateHash && window.location.hash !== href) {
    history.replaceState(null, "", href);
  }
  setCriteriaPanelOpen(false);
  workbench.root?.scrollIntoView({ block: "start" });
}

function viewFromHref(href) {
  const map = {
    "#sectionScreen": "screen",
    "#sectionGraph": "screen",
    "#sectionTrend": "screen",
    "#sectionObserve": "observe",
    "#sectionBacktest": "backtest",
    "#sectionNewsRag": "news",
    "#sectionAgent": "agent",
  };
  return map[href] || "screen";
}

function hrefForView(view) {
  const map = {
    screen: "#sectionScreen",
    observe: "#sectionObserve",
    backtest: "#sectionBacktest",
    news: "#sectionNewsRag",
    agent: "#sectionAgent",
  };
  return map[view] || "#sectionScreen";
}

function setActiveNavLink(activeView) {
  workbench.viewLinks.forEach((link) => {
    link.classList.toggle("active", (link.dataset.viewLink || "") === activeView);
  });
}

function setCriteriaPanelOpen(isOpen) {
  document.body.classList.toggle("criteria-open", Boolean(isOpen));
  if (workbench.criteriaOverlay) {
    workbench.criteriaOverlay.hidden = !isOpen;
  }
}

function initStickyOffsets() {
  const shell = $(".app-shell");
  const header = $(".app-header");
  const contextBar = $(".research-context-bar");
  if (!shell || !header) return;

  const update = () => {
    const headerHeight = Math.ceil(header.getBoundingClientRect().height);
    const contextHeight = contextBar ? Math.ceil(contextBar.getBoundingClientRect().height) : 0;
    shell.style.setProperty("--app-header-height", `${headerHeight}px`);
    shell.style.setProperty("--research-context-height", `${contextHeight}px`);
  };

  update();
  window.addEventListener("resize", update);
  if ("ResizeObserver" in window) {
    const observer = new ResizeObserver(update);
    observer.observe(header);
    if (contextBar) observer.observe(contextBar);
  }
}

async function runTask(button, panel, task) {
  const original = button.textContent;
  button.disabled = true;
  button.textContent = "运行中";
  try {
    await task();
  } finally {
    button.disabled = false;
    button.textContent = original;
    updateCriteriaSummary();
  }
}

async function runScreen() {
  if ($("#sectorMode")?.checked) {
    await runSectorScreen();
    return;
  }

  setLoading(panels.screen, "筛选中");
  const payload = buildCriteria();
  const data = await postJson("/api/screen", payload, panels.screen);
  if (data) renderScreenResult(panels.screen, data);
}

async function runSectorScreen() {
  setLoading(panels.screen, "按板块筛选中");
  const maxSectors = clampInt($("#maxSectors")?.value, 1, 50, DEFAULT_SECTOR_GROUP_LIMIT);
  const perSectorLimit = clampInt($("#perSectorLimit")?.value, 1, 50, DEFAULT_PER_SECTOR_LIMIT);
  const payload = {
    criteria: buildCriteria(),
    max_sectors: maxSectors,
    per_sector_limit: perSectorLimit,
    min_sector_candidates: perSectorLimit,
  };
  const data = await postJson("/api/sector-screen", payload, panels.screen);
  if (data) renderSectorScreenResult(panels.screen, data);
}

async function runGraph() {
  setLoading(panels.graph, "关系传播中");
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    seed_codes: parseCodes($("#seedCodes").value),
    relation_depth: clampInt($("#relationDepth").value, 1, 3, 1),
    relation_weight: clampFloat($("#relationWeight").value, 0, 1, 0.4),
    limit: Math.min(readInt("resultLimit", DEFAULT_RESULT_LIMIT), 100),
  };
  const data = await postJson("/api/graph-screen", payload, panels.graph);
  if (data) renderGraphResult(panels.graph, data);
}

async function runTrendAnalysis() {
  const code = readStockCode("trendCode");
  if (!code) {
    setError(panels.trend, "请输入股票代码", "例如：300750.SZ");
    return;
  }
  $("#trendCode").value = code;
  setLoading(panels.trend, "趋势指标计算中");
  const payload = {
    code,
    start_date: readDateParam("trendStart", "20200101"),
    end_date: readDateParam("trendEnd", currentSystemDateCompact()),
    series_limit: 180,
  };
  const data = await postJson("/api/trend", payload, panels.trend);
  if (data) renderTrendAnalysis(panels.trend, data);
}

async function runTrendScreen() {
  setLoading(panels.trend, "趋势选股中");
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    start_date: readDateParam("trendStart", "20200101"),
    end_date: readDateParam("trendEnd", currentSystemDateCompact()),
    limit: Math.min(readInt("resultLimit", DEFAULT_RESULT_LIMIT), 100),
  };
  const data = await postJson("/api/trend-screen", payload, panels.trend);
  if (data) renderTrendScreenResult(panels.trend, data);
}

async function runBacktest() {
  setLoading(panels.backtest, "回测中");
  updateBacktestScope();
  const source = getBacktestSource();
  const watchlistItems = readWatchlist();
  if (source === "watchlist" && !watchlistItems.length) {
    setError(panels.backtest, "自选观察池为空", "请先从筛选结果收藏股票。");
    return;
  }
  const payload = {
    source,
    criteria: buildCriteria({ limit: 100 }),
    stock_codes: source === "watchlist" ? watchlistItems.map((item) => item.code) : [],
    start_date: readDateParam("btStart", "20200101"),
    end_date: readDateParam("btEnd", currentSystemDateCompact()),
    top_n: clampInt($("#btTopN").value, 1, 100, 10),
    rebalance_frequency: $("#btRebalance")?.value || "monthly",
    transaction_cost_bps: clampFloat($("#btCostBps")?.value, 0, 500, 10),
    benchmark: $("#btBenchmark")?.value || "candidate_equal_weight",
  };
  const data = await postJson("/api/backtest", payload, panels.backtest);
  if (data) renderBacktestResult(panels.backtest, data);
}

async function runBacktestFromScreen() {
  activateWorkbenchView("backtest", { href: "#sectionBacktest", updateHash: true });
  await runTask(buttons.backtest, panels.backtest, runBacktest);
}

async function handlePanelEmptyAction(action, trigger) {
  if (action === "run-screen") {
    activateWorkbenchView("screen", { href: "#sectionScreen", updateHash: true });
    await runTask(buttons.screen, panels.screen, runScreen);
    return;
  }
  if (action === "watchlist-backtest") {
    setBacktestSource("watchlist");
    activateWorkbenchView("backtest", { href: "#sectionBacktest", updateHash: true });
    await runTask(buttons.backtest, panels.backtest, runBacktest);
    return;
  }
  if (action === "go-observe-screen") {
    activateWorkbenchView("screen", { href: "#sectionScreen", updateHash: true });
    panels.screen?.scrollIntoView({ block: "start", behavior: "smooth" });
    return;
  }
  activateWorkbenchView("screen", { href: "#sectionScreen", updateHash: true });
  trigger?.blur?.();
}

async function runNewsRag() {
  const code = readStockCode("newsCode");
  if (!code) {
    setError(panels.newsRag, "请输入目标股票代码", "上下游消息分析需要明确的单只目标股票，例如：300750.SZ。");
    return;
  }
  $("#newsCode").value = code;
  const mobileRuntime = isMobileTauriRuntime();
  const timer = startPanelProgress(
    panels.newsRag,
    mobileRuntime ? "手机端 RAG 分析中" : "上下游消息分析中",
    mobileRuntime
      ? [
          [18, "读取本机 RAG 包"],
          [42, "抽取离线证据"],
          [66, "运行本地分析"],
          [86, "生成影响判断"],
        ]
      : [
          [18, "读取已有关系图"],
          [38, "更新本地消息缓存"],
          [62, "检索证据"],
          [82, "生成影响判断"],
        ],
  );
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    code,
    seed_codes: [code],
    days: clampInt($("#newsDays")?.value, 1, 365, 30),
    max_items: 24,
  };
  const llm = buildLlmConfig();
  if (llm) payload.llm = llm;
  try {
    const data = await postJson("/api/news-rag", payload, panels.newsRag);
    if (data) renderNewsRagResult(panels.newsRag, data);
  } finally {
    if (timer) window.clearInterval(timer);
  }
}

async function runRagPackBuildFromNewsCache() {
  setLoading(panels.newsRag, "构建离线 RAG pack");
  const code = readStockCode("newsCode");
  if (code) $("#newsCode").value = code;
  const payload = {
    pack_version: `local-news-${new Date().toISOString().slice(0, 10)}`,
    days: clampInt($("#newsDays")?.value, 1, 3650, 30),
    stock_codes: code ? [code] : parseCodes($("#seedCodes")?.value || ""),
    relation_types: [],
    source_tiers: ["filing", "news", "community"],
    limit: 1000,
    target_chars: 500,
    overlap_chars: 80,
  };
  const data = await postJson("/api/rag-pack/build-from-news-cache", payload, panels.newsRag);
  if (data) renderRagPackBuildResult(panels.newsRag, data);
}

async function runRagPackQuery() {
  const code = readStockCode("newsCode");
  if (code) $("#newsCode").value = code;
  const query =
    $("#ragPackQuery")?.value.trim() ||
    (code ? `${code} 上下游 供应链 订单 证据` : "上下游 供应链 订单 证据");
  setLoading(panels.newsRag, "查询本地离线 RAG pack");
  const payload = {
    query,
    stock_codes: code ? [code] : parseCodes($("#seedCodes")?.value || ""),
    relation_types: [],
    source_tiers: ["filing", "news", "community"],
    top_k: 8,
  };
  const data = await postJson("/api/rag-pack/query", payload, panels.newsRag);
  if (data) renderRagPackQueryResult(panels.newsRag, data);
}

async function runUpstreamRagBuildAndTransfer() {
  const timer = startPanelProgress(panels.newsRag, "构建上下游 RAG 同步包", [
    [16, "采集 CNINFO 公告"],
    [36, "读取通达信 F10"],
    [58, "抓取公开 URL"],
    [76, "抽取关系和证据"],
    [90, "生成 manifest 和二维码"],
  ]);
  const code = readStockCode("newsCode");
  if (!code) {
    if (timer) window.clearInterval(timer);
    setError(panels.newsRag, "请输入目标股票代码", "构建手机同步包需要明确的单只股票，例如：300750.SZ。");
    return;
  }
  $("#newsCode").value = code;
  const buildPayload = {
    code,
    data_until: currentSystemDateInputValue(),
    filing_days: 1095,
    news_days: clampInt($("#newsDays")?.value, 1, 3650, 180),
    manual_urls: parseUpstreamManualUrls(),
  };
  try {
    const build = await postJson("/api/upstream-rag/build", buildPayload, panels.newsRag);
    if (!build) return;
    const transfer = build.manifest?.valid
      ? await postJson("/api/upstream-rag/transfer/start", { ttl_minutes: 15 }, panels.newsRag)
      : null;
    renderUpstreamRagBuildResult(panels.newsRag, build, transfer);
  } finally {
    if (timer) window.clearInterval(timer);
  }
}

async function runUpstreamRagList() {
  const mobileRuntime = isMobileTauriRuntime();
  setLoading(panels.newsRag, mobileRuntime ? "读取手机端 RAG 包" : "读取桌面端同步包状态");
  const data = mobileRuntime
    ? await getJson("/api/upstream-rag/mobile/list", panels.newsRag)
    : await getJson("/api/upstream-rag/status", panels.newsRag);
  if (!data) return;
  if (mobileRuntime) {
    renderUpstreamRagMobileList(panels.newsRag, data);
  } else {
    renderUpstreamRagDesktopStatus(panels.newsRag, data);
  }
}

async function runUpstreamRagImport() {
  if (!isMobileTauriRuntime()) {
    setError(panels.newsRag, "导入仅在安卓端执行", "桌面端负责构建和开启局域网临时传输服务。");
    return;
  }
  const descriptor = parseUpstreamImportDescriptor($("#upstreamImportPayload")?.value || "");
  if (!descriptor.manifest_url) {
    setError(panels.newsRag, "缺少扫码内容", "请扫码或粘贴 manifest_url / 二维码 JSON。");
    return;
  }
  setLoading(panels.newsRag, "下载并校验上下游 RAG 包");
  try {
    const payload = await fetchUpstreamImportPayload(descriptor);
    const data = await postJson("/api/upstream-rag/mobile/import", payload, panels.newsRag);
    if (data) renderUpstreamRagImportResult(panels.newsRag, data);
  } catch (err) {
    setError(panels.newsRag, "导入失败", err.message);
  }
}

async function runUpstreamQrScan() {
  if (!navigator.mediaDevices?.getUserMedia) {
    setError(panels.newsRag, "当前 WebView 不支持打开相机", "请用系统相机扫描桌面端二维码后，把二维码 JSON 或 manifest_url 粘贴到输入框。");
    return;
  }
  const detector = createBarcodeDetector();
  const jsQrAvailable = typeof window.jsQR === "function";
  if (!detector && !jsQrAvailable) {
    setError(panels.newsRag, "当前 WebView 不支持扫码", "缺少二维码解码器。请把二维码 JSON 或 manifest_url 粘贴到输入框。");
    return;
  }
  let stream;
  try {
    stream = await openQrCameraStream();
  } catch (err) {
    setError(panels.newsRag, "无法打开相机", cameraAccessMessage(err));
    return;
  }

  panels.newsRag.className = basePanelClass(panels.newsRag);
  panels.newsRag.innerHTML = `
    <div class="upstream-scan">
      <video id="upstreamScanVideo" autoplay playsinline muted></video>
      <div>
        <strong>对准桌面端二维码</strong>
        <span id="upstreamScanStatus" class="upstream-scan-status">${detector ? "使用原生识别" : "使用兼容识别"}</span>
        <button type="button" data-upstream-scan-cancel>取消</button>
      </div>
    </div>
  `;
  const video = $("#upstreamScanVideo");
  video.srcObject = stream;
  try {
    await video.play();
  } catch (err) {
    stream.getTracks().forEach((track) => track.stop());
    setError(panels.newsRag, "无法播放相机画面", err.message || "相机已授权但视频预览启动失败。");
    return;
  }

  let cancelled = false;
  window.__gpUpstreamQrCancel = () => {
    cancelled = true;
  };
  const canvas = document.createElement("canvas");
  try {
    while (!cancelled) {
      const raw = detector ? await detectWithBarcodeDetector(detector, video) : detectWithJsQr(video, canvas);
      if (raw) {
        $("#upstreamImportPayload").value = raw;
        break;
      }
      await delay(240);
    }
  } finally {
    stream.getTracks().forEach((track) => track.stop());
    window.__gpUpstreamQrCancel = null;
  }
  if (cancelled) {
    panels.newsRag.className = `${basePanelClass(panels.newsRag)} empty`;
    panels.newsRag.innerHTML = renderEmpty("已取消扫码");
    return;
  }
  await runUpstreamRagImport();
}

function createBarcodeDetector() {
  if (!("BarcodeDetector" in window)) return null;
  try {
    return new BarcodeDetector({ formats: ["qr_code"] });
  } catch {
    return null;
  }
}

async function openQrCameraStream() {
  const constraints = [
    { video: { facingMode: { ideal: "environment" } } },
    { video: true },
  ];
  let lastError = null;
  for (const constraint of constraints) {
    try {
      return await navigator.mediaDevices.getUserMedia(constraint);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError || new Error("相机不可用。");
}

async function detectWithBarcodeDetector(detector, video) {
  const codes = await detector.detect(video).catch(() => []);
  return codes?.[0]?.rawValue || "";
}

function detectWithJsQr(video, canvas) {
  const sourceWidth = video.videoWidth;
  const sourceHeight = video.videoHeight;
  if (!sourceWidth || !sourceHeight || typeof window.jsQR !== "function") return "";

  const maxDimension = 900;
  const scale = Math.min(1, maxDimension / Math.max(sourceWidth, sourceHeight));
  const width = Math.max(1, Math.round(sourceWidth * scale));
  const height = Math.max(1, Math.round(sourceHeight * scale));
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) return "";
  context.drawImage(video, 0, 0, width, height);
  const image = context.getImageData(0, 0, width, height);
  const code = window.jsQR(image.data, width, height, { inversionAttempts: "attemptBoth" });
  return code?.data || "";
}

function cameraAccessMessage(error) {
  const name = error?.name || "";
  const message = error?.message || "";
  if (name === "NotAllowedError" || name === "PermissionDeniedError") {
    return "相机权限被拒绝。请在系统设置里允许本应用使用相机，然后重新扫码。";
  }
  if (name === "NotFoundError" || name === "DevicesNotFoundError") {
    return "没有找到可用摄像头。";
  }
  if (name === "NotReadableError" || name === "TrackStartError") {
    return "摄像头被其他应用占用，或 Android WebView 无法启动摄像头。请关闭占用相机的应用后重试。";
  }
  return message || "Android WebView 未返回具体原因。请确认已授予相机权限。";
}

async function runUpstreamRagDetail(stockCode, packVersion) {
  setLoading(panels.newsRag, "读取 RAG 包详情");
  const data = isMobileTauriRuntime()
    ? await postJson("/api/upstream-rag/mobile/detail", { stock_code: stockCode, pack_version: packVersion }, panels.newsRag)
    : await getJson("/api/upstream-rag/status", panels.newsRag);
  if (data) renderUpstreamRagDetailResult(panels.newsRag, data);
}

async function runUpstreamRagRollback(stockCode) {
  if (!isMobileTauriRuntime()) {
    setError(panels.newsRag, "回滚仅在安卓端执行", "桌面端可以重新构建并开启传输。");
    return;
  }
  setLoading(panels.newsRag, "回滚 RAG 包");
  const data = await postJson("/api/upstream-rag/mobile/rollback", { stock_code: stockCode }, panels.newsRag);
  if (data) renderUpstreamRagDetailResult(panels.newsRag, data);
}

async function runAgent() {
  const message = $("#agentMsg").value.trim();
  if (!message) {
    setError(panels.agent, "请输入智能体指令", "例如：用产业链关系筛选新能源股票，市盈率低于 25。");
    return;
  }
  setLoading(panels.agent, "智能体分析中");
  const payload = { message };
  const llm = buildLlmConfig();
  if (llm) payload.llm = llm;
  const data = await postJson("/api/agent", payload, panels.agent);
  if (data) renderAgentResult(panels.agent, data);
}

async function runObserve(codeOverride) {
  const code = normalizeStockCode(codeOverride || $("#observeCode").value);
  if (!code) {
    setError(panels.observe, "请输入股票代码", "例如：300750.SZ", {
      label: "回到筛选页选一只观察",
      action: "go-observe-screen",
    });
    return;
  }
  $("#observeCode").value = code;
  activateWorkbenchView("observe", { href: "#sectionObserve", updateHash: true });
  setLoading(panels.observe, "观察行情和技术面");
  const params = new URLSearchParams({
    minute_period: $("#observeMinutePeriod").value || "1",
    series_limit: "160",
    minute_limit: "180",
  });
  const startDate = readDateParam("observeStart", "");
  const endDate = readDateParam("observeEnd", "");
  if (startDate) params.set("start_date", startDate);
  if (endDate) params.set("end_date", endDate);
  const data = await getJson(`/api/observe/${encodeURIComponent(code)}?${params}`, panels.observe);
  if (data) renderObserveResult(panels.observe, data);
}

function initDataSource() {
  if (!dataSource.select) return;
  const savedSource = normalizeDataSource(localStorage.getItem(DATA_SOURCE_KEY));
  if (savedSource && [...dataSource.select.options].some((option) => option.value === savedSource)) {
    dataSource.select.value = savedSource;
  } else {
    dataSource.select.value = DEFAULT_DATA_SOURCE;
    localStorage.setItem(DATA_SOURCE_KEY, DEFAULT_DATA_SOURCE);
  }
  if (dataSource.refresh) {
    dataSource.refresh.checked = localStorage.getItem(DATA_REFRESH_KEY) === "true";
  }
  const savedProxy = localStorage.getItem(DATA_PROXY_KEY);
  if (dataSource.proxy && savedProxy && [...dataSource.proxy.options].some((option) => option.value === savedProxy)) {
    dataSource.proxy.value = savedProxy;
  }
  initSourceSelects();
  updateSourceStatus();
  loadDataStatus().finally(() => maybeAutoRefreshUniverse("startup"));
}

function initAutoRefresh() {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") maybeAutoRefreshUniverse("visible");
  });
  window.addEventListener("focus", () => maybeAutoRefreshUniverse("focus"));
}

function initWatchlist() {
  syncBacktestSourceControls();
  renderWatchlistPanel();
  syncWatchlistButtons();
  updateBacktestIdleState();
}

function readWatchlist() {
  try {
    const parsed = JSON.parse(localStorage.getItem(WATCHLIST_KEY) || "[]");
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((item) => item && normalizeStockCode(item.code))
      .map((item) => ({
        code: normalizeStockCode(item.code),
        name: String(item.name || item.code || ""),
        industry: String(item.industry || ""),
        addedAt: String(item.addedAt || new Date().toISOString()),
        source: String(item.source || "screen"),
        screenCriteriaSummary: String(item.screenCriteriaSummary || ""),
      }));
  } catch {
    return [];
  }
}

function saveWatchlist(items) {
  localStorage.setItem(WATCHLIST_KEY, JSON.stringify(items));
  renderWatchlistPanel();
  syncWatchlistButtons();
  updateBacktestIdleState();
  updateResearchSummaries();
}

function isWatchlisted(code) {
  const normalized = normalizeStockCode(code);
  return readWatchlist().some((item) => item.code === normalized);
}

function toggleWatchlistFromElement(element) {
  const code = normalizeStockCode(element.dataset.watchlistCode);
  if (!code) return;
  const item = {
    code,
    name: element.dataset.watchlistName || code,
    industry: element.dataset.watchlistIndustry || "",
    addedAt: new Date().toISOString(),
    source: element.dataset.watchlistSource || "screen",
    screenCriteriaSummary: currentCriteriaSummaryText(),
  };
  toggleWatchlistStock(item);
}

function toggleWatchlistStock(item) {
  const normalized = normalizeStockCode(item.code);
  if (!normalized) return;
  const current = readWatchlist();
  const existingIndex = current.findIndex((entry) => entry.code === normalized);
  if (existingIndex >= 0) {
    current.splice(existingIndex, 1);
    saveWatchlist(current);
    setMaintenanceNote(`${normalized} 已取消收藏。`);
    return;
  }
  saveWatchlist([{ ...item, code: normalized, addedAt: new Date().toISOString() }, ...current]);
  setMaintenanceNote(`${item.name || normalized} 已加入自选观察池。`);
}

function removeWatchlistStock(code) {
  const normalized = normalizeStockCode(code);
  if (!normalized) return;
  saveWatchlist(readWatchlist().filter((item) => item.code !== normalized));
  setMaintenanceNote(`${normalized} 已从自选观察池移除。`);
}

function clearWatchlist() {
  if (!readWatchlist().length) return;
  saveWatchlist([]);
  setMaintenanceNote("自选观察池已清空。");
}

function renderWatchlistPanel() {
  if (!watchlistUi.panel) return;
  const items = readWatchlist();
  const source = getBacktestSource();
  watchlistUi.panel.hidden = source !== "watchlist";
  if (watchlistUi.count) watchlistUi.count.textContent = `${items.length} 只`;
  if (watchlistUi.empty) watchlistUi.empty.hidden = true;
  if (!watchlistUi.items) return;
  watchlistUi.items.innerHTML = items.length
    ? items
        .map(
          (item) => `
        <article class="watchlist-item">
          <div>
            <strong>${escapeHtml(item.name || item.code)}</strong>
            <span>${escapeHtml(item.code)} ${escapeHtml(item.industry || "")}</span>
            ${item.screenCriteriaSummary ? `<em>${escapeHtml(item.screenCriteriaSummary)}</em>` : ""}
          </div>
          <button type="button" data-watchlist-remove="${escapeHtml(item.code)}">移除</button>
        </article>
      `,
        )
        .join("")
    : renderEmpty("先从筛选结果收藏股票", { label: "去筛选收藏", action: "go-screen" });
}

function syncWatchlistButtons() {
  document.querySelectorAll("[data-watchlist-code]").forEach((button) => {
    const saved = isWatchlisted(button.dataset.watchlistCode);
    const label = saved ? "已收藏" : "收藏";
    button.classList.toggle("saved", saved);
    const icon = button.querySelector(".watchlist-icon");
    const text = button.querySelector(".watchlist-label");
    if (icon) icon.textContent = saved ? "★" : "☆";
    if (text) text.textContent = label;
    if (!icon && !text) button.textContent = label;
    button.setAttribute("aria-pressed", String(saved));
    button.setAttribute("aria-label", `${label} ${button.dataset.watchlistName || button.dataset.watchlistCode || ""}`.trim());
    button.setAttribute("title", label);
  });
}

function currentCriteriaSummaryText() {
  return workbench.criteriaSummary?.textContent || "";
}

function getBacktestSource() {
  return watchlistUi.source?.value === "watchlist" ? "watchlist" : "criteria";
}

function setBacktestSource(value) {
  const normalized = value === "watchlist" ? "watchlist" : "criteria";
  if (watchlistUi.source) watchlistUi.source.value = normalized;
  syncBacktestSourceControls();
  renderWatchlistPanel();
  updateBacktestIdleState();
  updateResearchSummaries();
}

function syncBacktestSourceControls() {
  const source = getBacktestSource();
  watchlistUi.sourceOptions.forEach((option) => {
    const active = option.dataset.backtestSourceOption === source;
    option.classList.toggle("active", active);
    option.setAttribute("aria-pressed", String(active));
  });
}

function updateBacktestIdleState() {
  if (!panels.backtest?.classList.contains("empty")) return;
  const items = readWatchlist();
  const source = getBacktestSource();
  const canUseWatchlist = items.length > 0;
  const action =
    source === "watchlist" && canUseWatchlist
      ? null
      : canUseWatchlist
        ? { label: "切到自选观察池", action: "watchlist-backtest" }
        : { label: "先运行筛选", action: "run-screen" };
  const text =
    source === "watchlist" && canUseWatchlist ? "等待自选观察池回测结果" : canUseWatchlist ? `自选观察池已有 ${items.length} 只股票` : "等待回测";
  panels.backtest.innerHTML = renderEmpty(text, action);
}

function initSourceSelects() {
  document.querySelectorAll(".source-control select").forEach((select) => {
    if (!(select instanceof HTMLSelectElement) || select.dataset.enhanced === "true") return;
    const control = select.closest(".source-control");
    if (!control) return;

    select.dataset.enhanced = "true";
    select.tabIndex = -1;
    select.setAttribute("aria-hidden", "true");
    const idBase = select.id || `sourceSelect${Math.random().toString(16).slice(2)}`;
    const custom = document.createElement("div");
    const button = document.createElement("button");
    const menu = document.createElement("div");

    custom.className = "source-select";
    button.type = "button";
    button.className = "source-select-button";
    button.setAttribute("aria-haspopup", "listbox");
    button.setAttribute("aria-expanded", "false");
    button.setAttribute("aria-controls", `${idBase}Menu`);
    menu.className = "source-select-menu";
    menu.id = `${idBase}Menu`;
    menu.setAttribute("role", "listbox");
    menu.hidden = true;

    [...select.options].forEach((option) => {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "source-select-option";
      item.dataset.value = option.value;
      item.setAttribute("role", "option");
      item.textContent = option.textContent;
      item.addEventListener("click", () => {
        select.value = option.value;
        syncSourceSelect(select);
        closeSourceSelects();
        select.dispatchEvent(new Event("change", { bubbles: true }));
      });
      menu.appendChild(item);
    });

    button.addEventListener("click", (event) => {
      event.stopPropagation();
      const shouldOpen = menu.hidden;
      closeSourceSelects();
      setSourceSelectOpen(custom, shouldOpen);
    });
    button.addEventListener("keydown", (event) => {
      if (!["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) return;
      event.preventDefault();
      closeSourceSelects();
      setSourceSelectOpen(custom, true);
      const activeOption = menu.querySelector('[aria-selected="true"]') || menu.querySelector(".source-select-option");
      activeOption?.focus();
    });
    menu.addEventListener("keydown", (event) => {
      const options = [...menu.querySelectorAll(".source-select-option")];
      const currentIndex = Math.max(0, options.indexOf(document.activeElement));
      if (event.key === "Escape") {
        event.preventDefault();
        closeSourceSelects();
        button.focus();
        return;
      }
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const delta = event.key === "ArrowDown" ? 1 : -1;
        options[(currentIndex + delta + options.length) % options.length]?.focus();
        return;
      }
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        document.activeElement?.click();
      }
    });

    custom.append(button, menu);
    control.appendChild(custom);
    select.addEventListener("change", () => syncSourceSelect(select));
    syncSourceSelect(select);
  });

  document.addEventListener("click", closeSourceSelects);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") closeSourceSelects();
  });
}

function syncSourceSelect(select) {
  const control = select.closest(".source-control");
  const custom = control?.querySelector(".source-select");
  const button = custom?.querySelector(".source-select-button");
  const selectedOption = select.selectedOptions[0];
  if (!custom || !button || !selectedOption) return;
  const selectedText = selectedOption.textContent || selectedOption.value;
  const labelText = control.querySelector(":scope > span")?.textContent?.trim();
  button.textContent = selectedText;
  button.setAttribute("aria-label", labelText ? `${labelText}：${selectedText}` : selectedText);
  custom.querySelectorAll(".source-select-option").forEach((option) => {
    const isSelected = option.dataset.value === select.value;
    option.classList.toggle("selected", isSelected);
    option.setAttribute("aria-selected", String(isSelected));
    option.tabIndex = isSelected ? 0 : -1;
  });
}

function setSourceSelectOpen(custom, isOpen) {
  const button = custom.querySelector(".source-select-button");
  const menu = custom.querySelector(".source-select-menu");
  custom.classList.toggle("open", isOpen);
  button?.setAttribute("aria-expanded", String(isOpen));
  if (menu) menu.hidden = !isOpen;
}

function closeSourceSelects() {
  document.querySelectorAll(".source-select.open").forEach((custom) => setSourceSelectOpen(custom, false));
}

function getSelectedDataSource() {
  return normalizeDataSource(dataSource.select?.value);
}

function normalizeDataSource(source) {
  const value = String(source || "").trim().toLowerCase();
  if (["tdx", "astock", "akshare", "eastmoney"].includes(value)) return "tdx";
  return DEFAULT_DATA_SOURCE;
}

function dataSourceHeaders() {
  const headers = { "X-Stock-Provider": getSelectedDataSource() };
  if (dataSource.refresh?.checked) headers["X-Stock-Refresh"] = "true";
  headers["X-Stock-Proxy"] = getSelectedProxyMode();
  return headers;
}

function stockSearchHeaders() {
  return {
    "X-Stock-Provider": getSelectedDataSource(),
    "X-Stock-Proxy": getSelectedProxyMode(),
  };
}

function getSelectedProxyMode() {
  return dataSource.proxy?.value || "none";
}

function updateSourceStatus() {
  if (!dataSource.status) return;
  const source = getSelectedDataSource();
  const label = sourceLabel(source);
  const suffix = dataSource.refresh?.checked ? " 刷新" : "";
  const proxySuffix = getSelectedProxyMode() === "none" ? " 直连" : "";
  dataSource.status.innerHTML = `<i aria-hidden="true"></i>${escapeHtml(label + suffix + proxySuffix)}`;
  if (dataSource.select) syncSourceSelect(dataSource.select);
  if (dataSource.proxy) syncSourceSelect(dataSource.proxy);
}

async function runDataTask(button, task) {
  const original = button.textContent;
  button.disabled = true;
  button.classList.add("is-busy");
  button.textContent = "处理中";
  try {
    await task();
  } finally {
    button.disabled = false;
    button.classList.remove("is-busy");
    button.textContent = original;
  }
}

async function loadDataStatus() {
  if (!dataSource.universe) return;
  setMaintenanceNote("读取数据状态中");
  try {
    renderDataStatus(await requestJson("GET", "/api/data-sources/status"));
  } catch (err) {
    setMaintenanceNote(`数据状态读取失败：${err.message}`);
  }
}

async function maybeAutoRefreshUniverse(trigger = "startup") {
  if (!shouldCheckAutoRefreshUniverse()) return;
  autoRefreshInFlight = true;
  const progress = isMobileTauriRuntime() && trigger === "startup" ? startRefreshProgress() : null;
  if (progress) setMaintenanceNote("首次安装正在联网生成股票池");
  try {
    const data = await requestJson("POST", "/api/data-sources/auto-refresh-universe", {
      mode: "light",
      max_bytes: 209715200,
      daily_days: 500,
      minute_days: 3,
    });
    localStorage.setItem(AUTO_REFRESH_CHECK_KEY, String(Date.now()));
    renderDataStatus(data.status);
    const notes = data.notes || [];
    if (data.refreshed) {
      if (progress) finishRefreshProgress(data.initial_refresh ? "股票池初始化完成" : "股票池刷新完成");
      setMaintenanceNote(notes.join(" ") || "交易日收盘后已自动刷新基础股票池。");
    } else if (data.due) {
      setMaintenanceNote(notes.join(" ") || "交易日自动刷新未完成，可手动刷新。");
    } else if (notes.length && trigger === "startup") {
      setMaintenanceNote(notes.join(" "));
    }
  } catch (err) {
    if (progress) failRefreshProgress("股票池初始化失败");
    setMaintenanceNote(`股票池自动刷新检查失败：${err.message}`);
  } finally {
    autoRefreshInFlight = false;
    if (progress) window.setTimeout(() => stopRefreshProgress(progress), 900);
  }
}

function shouldCheckAutoRefreshUniverse() {
  if (!dataSource.universe || autoRefreshInFlight) return false;
  const lastChecked = Number(localStorage.getItem(AUTO_REFRESH_CHECK_KEY) || 0);
  return !Number.isFinite(lastChecked) || Date.now() - lastChecked >= AUTO_REFRESH_CHECK_INTERVAL_MS;
}

async function refreshUniverse() {
  const mobileRuntime = isMobileTauriRuntime();
  const progress = startRefreshProgress();
  setMaintenanceNote(mobileRuntime ? "正在联网更新股票池" : "刷新股票池中，真实数据源可能需要几十秒");
  try {
    const data = await requestJson("POST", "/api/data-sources/refresh-universe", {
      mode: "light",
      max_bytes: 209715200,
      daily_days: 500,
      minute_days: 3,
    });
    finishRefreshProgress("股票池刷新完成");
    renderDataStatus(data.status);
    setMaintenanceNote((data.notes || []).join(" ") || "股票池刷新完成");
  } catch (err) {
    failRefreshProgress("股票池刷新失败");
    setMaintenanceNote(`股票池刷新失败：${err.message}`);
  } finally {
    window.setTimeout(() => stopRefreshProgress(progress), 900);
  }
}

async function pruneCache() {
  setMaintenanceNote("清理可丢弃缓存中");
  try {
    const data = await requestJson("POST", "/api/data-sources/prune-cache", {
      mode: "light",
      max_bytes: 209715200,
      daily_days: 500,
      minute_days: 3,
    });
    renderDataStatus(data.status);
    setMaintenanceNote(`已删除 ${data.removed_files || 0} 个文件，释放 ${formatBytes(data.removed_bytes || 0)}。`);
  } catch (err) {
    setMaintenanceNote(`缓存清理失败：${err.message}`);
  }
}

function setMaintenanceNote(text) {
  if (dataSource.note) dataSource.note.textContent = text;
}

function startRefreshProgress() {
  if (!dataSource.progress) return null;
  const stages = [
    [15, "准备数据源"],
    [32, "获取股票池"],
    [54, "写入本地缓存"],
    [72, "检查缓存占用"],
    [85, "等待腾讯行情返回"],
  ];
  let index = 0;
  dataSource.progress.hidden = false;
  setRefreshProgress(8, "准备刷新");
  return window.setInterval(() => {
    const [value, label] = stages[Math.min(index, stages.length - 1)];
    setRefreshProgress(value, label);
    index += 1;
  }, 900);
}

function finishRefreshProgress(label) {
  setRefreshProgress(100, label || "刷新完成");
}

function failRefreshProgress(label) {
  setRefreshProgress(100, label || "刷新失败");
  dataSource.progress?.classList.add("failed");
}

function stopRefreshProgress(timer) {
  if (timer) window.clearInterval(timer);
  if (!dataSource.progress) return;
  dataSource.progress.hidden = true;
  dataSource.progress.classList.remove("failed");
  setRefreshProgress(0, "准备刷新");
}

function setRefreshProgress(value, label) {
  const percent = Math.min(Math.max(Number(value) || 0, 0), 100);
  if (dataSource.progressLabel) dataSource.progressLabel.textContent = label;
  if (dataSource.progressValue) dataSource.progressValue.textContent = `${Math.round(percent)}%`;
  if (dataSource.progressBar) dataSource.progressBar.style.width = `${percent}%`;
}

function startPanelProgress(node, title, stages) {
  if (!node) return null;
  let index = 0;
  node.className = `${basePanelClass(node)} loading progress-loading`;
  node.innerHTML = `
    <div class="analysis-progress">
      <div>
        <span>${escapeHtml(title)}</span>
        <strong data-progress-value>8%</strong>
      </div>
      <div class="progress-track" aria-hidden="true"><span data-progress-bar style="width: 8%"></span></div>
      <p data-progress-label>准备分析</p>
    </div>
  `;
  return window.setInterval(() => {
    const [value, label] = stages[Math.min(index, stages.length - 1)];
    const percent = Math.min(Math.max(Number(value) || 0, 0), 100);
    const valueNode = node.querySelector("[data-progress-value]");
    const barNode = node.querySelector("[data-progress-bar]");
    const labelNode = node.querySelector("[data-progress-label]");
    if (valueNode) valueNode.textContent = `${Math.round(percent)}%`;
    if (barNode) barNode.style.width = `${percent}%`;
    if (labelNode) labelNode.textContent = label;
    index += 1;
  }, 700);
}

function initLlmSettings() {
  if (!llmSettings.model) return;
  try {
    const saved = JSON.parse(localStorage.getItem(LLM_SETTINGS_KEY) || "{}");
    if (saved.api_key) {
      llmSettings.apiKey.value = saved.api_key;
      llmSettings.rememberKey.checked = true;
    }
    llmSettings.baseUrl.value = saved.base_url || "";
    llmSettings.model.value = saved.model || "";
    llmSettings.temperature.value = saved.temperature ?? "";
    llmSettings.timeout.value = saved.timeout_seconds ?? "";
    llmSettings.jsonMode.checked = saved.json_mode !== false;
  } catch {
    localStorage.removeItem(LLM_SETTINGS_KEY);
  }
  updateLlmStatus();
}

function buildLlmConfig() {
  if (!llmSettings.model) return null;
  const config = {};
  const apiKey = llmSettings.apiKey.value.trim();
  const baseUrl = normalizeBaseUrl(llmSettings.baseUrl.value);
  const model = llmSettings.model.value.trim();
  const temperature = Number.parseFloat(llmSettings.temperature.value);
  const timeout = Number.parseFloat(llmSettings.timeout.value);

  if (apiKey) config.api_key = apiKey;
  if (baseUrl) config.base_url = baseUrl;
  if (model) config.model = model;
  if (Number.isFinite(temperature)) config.temperature = Math.min(Math.max(temperature, 0), 2);
  if (Number.isFinite(timeout)) config.timeout_seconds = Math.min(Math.max(timeout, 1), 180);
  config.json_mode = llmSettings.jsonMode.checked;

  return Object.keys(config).length > 1 || config.json_mode === false ? config : null;
}

function saveLlmSettings() {
  const config = buildLlmConfig() || {};
  if (!llmSettings.rememberKey.checked) delete config.api_key;
  localStorage.setItem(LLM_SETTINGS_KEY, JSON.stringify(config));
  updateLlmStatus("已保存");
}

function clearLlmSettings() {
  localStorage.removeItem(LLM_SETTINGS_KEY);
  llmSettings.apiKey.value = "";
  llmSettings.baseUrl.value = "";
  llmSettings.model.value = "";
  llmSettings.temperature.value = "";
  llmSettings.timeout.value = "";
  llmSettings.jsonMode.checked = true;
  llmSettings.rememberKey.checked = false;
  updateLlmStatus();
}

function updateLlmStatus(customText) {
  if (!llmSettings.status) return;
  if (customText) {
    llmSettings.status.textContent = customText;
    return;
  }
  const hasKey = Boolean(llmSettings.apiKey.value.trim());
  const hasEndpoint = Boolean(llmSettings.baseUrl.value.trim() || llmSettings.model.value.trim());
  if (hasKey && hasEndpoint) {
    llmSettings.status.textContent = "自定义接口";
  } else if (hasKey) {
    llmSettings.status.textContent = "自定义密钥";
  } else if (hasEndpoint) {
    llmSettings.status.textContent = "服务端密钥";
  } else {
    llmSettings.status.textContent = "服务端默认";
  }
}

function normalizeBaseUrl(value) {
  return value.trim().replace(/\/+$/, "");
}

function initFormControls() {
  initIndustryOptions();
  initRebalanceOptions();
  initMarketConfirmers();
  initStockSuggesters();
  initDateInputs();
  initCriteriaSummary();
}

function initIndustryOptions() {
  const industryInput = $("#industry");
  const options = [...document.querySelectorAll("[data-industry-option]")];
  if (!industryInput || !options.length) return;

  const setIndustry = (value) => {
    industryInput.value = value || "";
    options.forEach((option) => {
      const isActive = (option.dataset.industryOption || "") === industryInput.value;
      option.classList.toggle("active", isActive);
      option.setAttribute("aria-pressed", String(isActive));
    });
  };

  options.forEach((option) => {
    option.setAttribute("aria-pressed", option.classList.contains("active") ? "true" : "false");
    option.addEventListener("click", () => {
      setIndustry(option.dataset.industryOption || "");
      updateResearchSummaries();
    });
  });
  setIndustry(industryInput.value);
}

function initRebalanceOptions() {
  const input = $("#btRebalance");
  const options = [...document.querySelectorAll("[data-rebalance-option]")];
  if (!input || !options.length) return;

  const setFrequency = (value) => {
    input.value = value || "monthly";
    options.forEach((option) => {
      const isActive = (option.dataset.rebalanceOption || "") === input.value;
      option.classList.toggle("active", isActive);
      option.setAttribute("aria-pressed", String(isActive));
    });
    updateResearchSummaries();
  };

  options.forEach((option) => {
    option.setAttribute("aria-pressed", option.classList.contains("active") ? "true" : "false");
    option.addEventListener("click", () => setFrequency(option.dataset.rebalanceOption || "monthly"));
  });
  setFrequency(input.value);
}

function initCriteriaSummary() {
  [
    "minRoe",
    "minMcap",
    "maxPe",
    "maxPb",
    "resultLimit",
    "btStart",
    "btEnd",
    "btTopN",
    "btSource",
    "btRebalance",
    "btCostBps",
    "btBenchmark",
    "includeSt",
    "sectorMode",
    "sortBy",
    "sortDir",
  ].forEach((id) => {
    const input = $(`#${id}`);
    input?.addEventListener("input", updateResearchSummaries);
    input?.addEventListener("change", updateResearchSummaries);
  });
  updateResearchSummaries();
}

function initMarketConfirmers() {
  const fields = [...document.querySelectorAll(".market-field")];
  if (!fields.length) return;

  fields.forEach((field) => {
    const input = field.querySelector("input[data-code-confirm]");
    const panel = field.querySelector(".market-confirm");
    if (!input || !panel) return;

    input.addEventListener("input", () => {
      input.value = sanitizeStockCodeInput(input.value);
      updateMarketConfirm(input, panel);
    });
    input.addEventListener("focus", () => updateMarketConfirm(input, panel));
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") hideMarketConfirm(panel);
    });
    panel.addEventListener("mousedown", (event) => event.preventDefault());
    panel.querySelectorAll("[data-market]").forEach((button) => {
      button.addEventListener("click", () => {
        const digits = stockCodeDigits(input.value);
        if (digits.length !== 6) return;
        input.value = normalizeStockCode(digits, button.dataset.market);
        hideMarketConfirm(panel);
        input.focus();
      });
    });
    updateMarketConfirm(input, panel);
  });

  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    if (target?.closest(".market-field")) return;
    document.querySelectorAll(".market-confirm").forEach(hideMarketConfirm);
  });
}

function initStockSuggesters() {
  const inputs = [...document.querySelectorAll("input[data-code-confirm]")];
  if (!inputs.length) return;

  inputs.forEach((input) => {
    const field = input.closest(".market-field") || input.parentElement;
    if (!field) return;

    const panel = document.createElement("div");
    panel.className = "stock-suggest";
    panel.hidden = true;
    panel.setAttribute("role", "listbox");
    field.append(panel);

    let timer = 0;
    let requestId = 0;
    let activeIndex = -1;

    const hide = () => {
      panel.hidden = true;
      panel.innerHTML = "";
      activeIndex = -1;
      input.setAttribute("aria-expanded", "false");
      input.removeAttribute("aria-activedescendant");
    };

    const choose = (stock) => {
      if (!stock?.code) return;
      input.value = stock.code;
      hide();
      input.dispatchEvent(new Event("change", { bubbles: true }));
      input.focus();
    };

    const setActive = (index) => {
      const options = [...panel.querySelectorAll("[data-stock-suggest-option]")];
      if (!options.length) return;
      activeIndex = (index + options.length) % options.length;
      options.forEach((option, optionIndex) => {
        const isActive = optionIndex === activeIndex;
        option.classList.toggle("active", isActive);
        option.setAttribute("aria-selected", String(isActive));
        if (isActive) input.setAttribute("aria-activedescendant", option.id);
      });
    };

    const render = (items) => {
      if (!items.length) {
        hide();
        const marketPanel = field.querySelector(".market-confirm");
        if (marketPanel) updateMarketConfirm(input, marketPanel);
        return;
      }

      field.querySelectorAll(".market-confirm").forEach(hideMarketConfirm);
      panel.innerHTML = items
        .map(
          (stock, index) => `
            <button id="${input.id || "stock"}Suggest${index}" type="button" role="option" data-stock-suggest-option="${index}">
              <strong>${escapeHtml(stock.code || "")}</strong>
              <span>${escapeHtml(stock.name || "-")}</span>
              <em>${escapeHtml(stock.industry || "未知行业")}</em>
            </button>
          `,
        )
        .join("");
      panel.hidden = false;
      input.setAttribute("aria-expanded", "true");
      activeIndex = -1;
      panel.querySelectorAll("[data-stock-suggest-option]").forEach((button, index) => {
        button.addEventListener("click", () => choose(items[index]));
      });
    };

    const search = async () => {
      const query = input.value.trim();
      if (!query) {
        hide();
        return;
      }

      const currentRequest = ++requestId;
      try {
        const params = new URLSearchParams({ q: query, limit: String(STOCK_SEARCH_LIMIT) });
        const items = await requestJson("GET", `/api/stock-search?${params}`, undefined, stockSearchHeaders());
        if (currentRequest !== requestId) return;
        render(Array.isArray(items) ? items.slice(0, STOCK_SEARCH_LIMIT) : []);
      } catch {
        if (currentRequest === requestId) hide();
      }
    };

    const scheduleSearch = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(search, 160);
    };

    input.setAttribute("aria-autocomplete", "list");
    input.setAttribute("aria-expanded", "false");
    input.addEventListener("input", scheduleSearch);
    input.addEventListener("focus", scheduleSearch);
    input.addEventListener("keydown", (event) => {
      const options = [...panel.querySelectorAll("[data-stock-suggest-option]")];
      if (panel.hidden || !options.length) return;
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive(activeIndex + 1);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive(activeIndex - 1);
      } else if (event.key === "Enter" && activeIndex >= 0) {
        event.preventDefault();
        options[activeIndex].click();
      } else if (event.key === "Escape") {
        hide();
      }
    });
    input.addEventListener("blur", () => window.setTimeout(hide, 120));
    panel.addEventListener("mousedown", (event) => event.preventDefault());
  });
}

function initDateInputs() {
  document.querySelectorAll('input[type="date"]').forEach((input) => {
    const dateValue = toDateInputValue(input.value || defaultDateInputValue(input));
    if (dateValue) input.value = dateValue;
    input.addEventListener("click", () => showDatePicker(input));
  });
}

function defaultDateInputValue(input) {
  return DEFAULT_TODAY_DATE_INPUT_IDS.has(input.id) ? currentSystemDateInputValue() : "";
}

function showDatePicker(input) {
  if (typeof input.showPicker !== "function") return;
  try {
    input.showPicker();
  } catch {
    // Some browsers only allow showPicker from direct pointer activation.
  }
}

function updateMarketConfirm(input, panel) {
  const digits = stockCodeDigits(input.value);
  const shouldShow = digits.length === 6 && !hasMarketSuffix(input.value);
  panel.hidden = !shouldShow;
  if (!shouldShow) return;

  const suggestedMarket = inferMarketFromDigits(digits);
  panel.querySelectorAll("[data-market]").forEach((button) => {
    button.classList.toggle("suggested", button.dataset.market === suggestedMarket);
  });
}

function hideMarketConfirm(panel) {
  panel.hidden = true;
}

function buildCriteria(overrides = {}) {
  const criteria = {
    include_st: $("#includeSt").checked,
    limit: readInt("resultLimit", DEFAULT_RESULT_LIMIT),
    sort_by: $("#sortBy").value,
    sort_dir: $("#sortDir").value,
    ...overrides,
  };

  const industry = $("#industry").value.trim();
  if (industry) criteria.industry = industry;

  addNumber(criteria, "min_roe", "minRoe");
  addNumber(criteria, "max_pe", "maxPe");
  addNumber(criteria, "max_pb", "maxPb");
  addNumber(criteria, "min_market_cap_billion", "minMcap");
  return criteria;
}

function updateBacktestScope() {
  const node = $("#backtestScope");
  if (!node) return;
  const cost = clampFloat($("#btCostBps")?.value, 0, 500, 10);
  const source = getBacktestSource();
  const watchlistItems = readWatchlist();
  const topN = clampInt($("#btTopN")?.value, 1, 100, 10);
  const sourceText =
    source === "watchlist"
      ? `自选观察池 ${Math.min(topN, watchlistItems.length)} / ${watchlistItems.length} 只`
      : ($("#industry")?.value ? `行业 ${$("#industry").value}` : "全部行业");
  const label = node.closest(".backtest-context")?.querySelector("span");
  if (label) label.textContent = source === "watchlist" ? "使用自选观察池" : "使用当前研究条件";
  const parts = [
    sourceText,
    `持仓 ${topN} 只`,
    `${displayDateParam("btStart", "2020-01-01")} 至 ${displayDateParam("btEnd", currentSystemDateInputValue())}`,
    rebalanceLabel($("#btRebalance")?.value || "monthly"),
    `${formatNumber(cost)} bps`,
    benchmarkLabel($("#btBenchmark")?.value || "candidate_equal_weight"),
  ];
  node.textContent = parts.join(" · ");
}

function updateResearchSummaries() {
  updateBacktestScope();
  updateCriteriaSummary();
}

function updateCriteriaSummary() {
  if (!workbench.criteriaSummary) return;
  const parts = [
    $("#industry")?.value ? `行业 ${$("#industry").value}` : "全部行业",
    $("#minRoe")?.value ? `ROE ≥ ${formatPercent(Number($("#minRoe").value))}` : "",
    $("#maxPe")?.value ? `PE ≤ ${formatNumber($("#maxPe").value)}` : "",
    $("#minMcap")?.value ? `市值 ≥ ${formatNumber($("#minMcap").value)} 亿` : "",
    `返回 ${clampInt($("#resultLimit")?.value, 1, 200, DEFAULT_RESULT_LIMIT)} 只`,
    rebalanceLabel($("#btRebalance")?.value || "monthly"),
  ].filter(Boolean);
  workbench.criteriaSummary.textContent = parts.join(" · ");
}

function rebalanceLabel(value) {
  const labels = {
    none: "买入持有",
    monthly: "月度再平衡",
    quarterly: "季度再平衡",
  };
  return labels[value] || "月度再平衡";
}

function benchmarkLabel(value) {
  const labels = {
    candidate_equal_weight: "候选池等权基准",
    none: "无基准",
  };
  return labels[value] || value || "候选池等权基准";
}

function displayDateParam(id, fallback) {
  const value = $(`#${id}`)?.value;
  return value || fallback;
}

function addNumber(target, key, id) {
  const value = readNumber(id);
  if (value !== null) target[key] = value;
}

function readNumber(id) {
  const value = Number.parseFloat($(`#${id}`).value);
  return Number.isFinite(value) ? value : null;
}

function readInt(id, fallback) {
  return clampInt($(`#${id}`).value, 1, 200, fallback);
}

function readStockCode(id) {
  return normalizeStockCode($(`#${id}`)?.value);
}

function normalizeStockCode(value, market) {
  const raw = sanitizeStockCodeInput(value);
  if (!raw) return "";

  const suffixed = raw.match(/^(\d{6})\.(SH|SZ|BJ)$/);
  if (suffixed) return `${suffixed[1]}.${suffixed[2]}`;

  const prefixed = raw.match(/^(SH|SZ|BJ)(\d{6})$/);
  if (prefixed) return `${prefixed[2]}.${prefixed[1]}`;

  const digits = stockCodeDigits(raw);
  if (digits.length !== 6) return "";
  return `${digits}.${market || inferMarketFromDigits(digits)}`;
}

function sanitizeStockCodeInput(value) {
  return String(value || "")
    .trim()
    .toUpperCase()
    .replace(/[^\dA-Z.]/g, "");
}

function stockCodeDigits(value) {
  const raw = sanitizeStockCodeInput(value);
  const prefixed = raw.match(/^(SH|SZ|BJ)(\d{6})$/);
  if (prefixed) return prefixed[2];
  const match = raw.match(/\d{6}/);
  return match ? match[0] : "";
}

function hasMarketSuffix(value) {
  const raw = sanitizeStockCodeInput(value);
  return /^(\d{6})\.(SH|SZ|BJ)$/.test(raw) || /^(SH|SZ|BJ)(\d{6})$/.test(raw);
}

function inferMarketFromDigits(digits) {
  if (/^[569]/.test(digits)) return "SH";
  if (/^[48]/.test(digits)) return "BJ";
  return "SZ";
}

function readDateParam(id, fallback = "") {
  return normalizeDateParam($(`#${id}`)?.value, fallback);
}

function currentSystemDateInputValue() {
  const now = new Date();
  return [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
  ].join("-");
}

function currentSystemDateCompact() {
  return currentSystemDateInputValue().replaceAll("-", "");
}

function normalizeDateParam(value, fallback = "") {
  const raw = String(value || "").trim();
  if (!raw) return fallback;
  const inputDate = raw.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (inputDate) return `${inputDate[1]}${inputDate[2]}${inputDate[3]}`;
  const compactDate = raw.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (compactDate) return compactDate[0];
  return raw.replaceAll("-", "");
}

function toDateInputValue(value) {
  const raw = String(value || "").trim();
  const compactDate = raw.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (compactDate) return `${compactDate[1]}-${compactDate[2]}-${compactDate[3]}`;
  return raw;
}

function parseCodes(raw) {
  return raw
    .split(/[,，\s]+/)
    .map((item) => normalizeStockCode(item))
    .filter(Boolean);
}

function clampInt(raw, min, max, fallback) {
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value)) return fallback;
  return Math.min(Math.max(value, min), max);
}

function clampFloat(raw, min, max, fallback) {
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value)) return fallback;
  return Math.min(Math.max(value, min), max);
}

async function postJson(url, payload, resultNode) {
  try {
    return await requestJson("POST", url, payload);
  } catch (err) {
    setError(resultNode, "请求异常", err.message);
    return null;
  }
}

async function getJson(url, resultNode) {
  try {
    return await requestJson("GET", url);
  } catch (err) {
    setError(resultNode, "请求异常", err.message);
    return null;
  }
}

async function requestJson(method, url, payload, headers = dataSourceHeaders()) {
  const tauriResult = await requestTauriJson(method, url, payload);
  if (tauriResult.handled) return tauriResult.data;

  const request = {
    method,
    headers: method === "POST" ? { "Content-Type": "application/json", ...headers } : headers,
  };
  if (payload !== undefined) request.body = JSON.stringify(payload);

  const resp = await fetch(url, request);
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || `HTTP ${resp.status}`);
  }
  return await resp.json();
}

async function requestTauriJson(method, url, payload) {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke || !isMobileTauriRuntime()) return { handled: false };

  const normalizedMethod = String(method || "GET").toUpperCase();
  const parsed = new URL(url, window.location.href);
  const path = parsed.pathname;
  const handler = tauriMobileRouteHandler(normalizedMethod, path);
  if (handler) return { handled: true, data: await handler({ invoke, parsed, path, payload }) };
  if (normalizedMethod === "GET" || normalizedMethod === "POST") throw new Error(`移动端暂不支持该接口：${path}`);
  return { handled: false };
}

function tauriMobileRouteHandler(method, path) {
  if (method === "GET") {
    return (
      TAURI_MOBILE_GET_ROUTES[path] ||
      TAURI_MOBILE_GET_PREFIX_ROUTES.find((route) => path.startsWith(route.prefix))?.handler ||
      null
    );
  }
  if (method === "POST") return TAURI_MOBILE_POST_ROUTES[path] || null;
  return null;
}

function isTauriRuntime() {
  return Boolean(window.__TAURI__?.core?.invoke);
}

function isMobileTauriRuntime() {
  return isTauriRuntime() && !isDesktopBackendOrigin();
}

function isDesktopBackendOrigin() {
  const protocol = window.location.protocol;
  const hostname = window.location.hostname;
  return (
    (protocol === "http:" || protocol === "https:") &&
    (hostname === "127.0.0.1" || hostname === "localhost" || hostname === "::1")
  );
}

function parseUpstreamManualUrls() {
  return String($("#upstreamManualUrls")?.value || "")
    .split(/\r?\n/)
    .map((item) => item.trim())
    .filter((item) => /^https?:\/\//i.test(item))
    .slice(0, 12);
}

function parseUpstreamImportDescriptor(rawValue) {
  const raw = String(rawValue || "").trim();
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      return {
        manifest_url: parsed.manifest_url || parsed.manifestUrl || "",
        pack_url: parsed.pack_url || parsed.packUrl || "",
        token: parsed.token || "",
      };
    }
  } catch {
    // 输入不是 JSON 时继续按 URL 解析。
  }
  if (/^https?:\/\//i.test(raw)) {
    return { manifest_url: raw, pack_url: deriveUpstreamPackUrl(raw) };
  }
  return {};
}

async function fetchUpstreamImportPayload(descriptor) {
  const manifestResponse = await fetch(descriptor.manifest_url, { cache: "no-store" });
  if (!manifestResponse.ok) {
    throw new Error(`manifest 下载失败：HTTP ${manifestResponse.status}`);
  }
  const manifest = await manifestResponse.json();
  const packUrl = descriptor.pack_url || deriveUpstreamPackUrl(descriptor.manifest_url, manifest.files?.pack);
  const packResponse = await fetch(packUrl, { cache: "no-store" });
  if (!packResponse.ok) {
    throw new Error(`RAG 包下载失败：HTTP ${packResponse.status}`);
  }
  const packBuffer = await packResponse.arrayBuffer();
  return {
    manifest,
    pack_base64: arrayBufferToBase64(packBuffer),
  };
}

function deriveUpstreamPackUrl(manifestUrl, packFile = "rag_pack.sqlite") {
  try {
    const url = new URL(manifestUrl);
    const parts = url.pathname.split("/");
    parts[parts.length - 1] = packFile || "rag_pack.sqlite";
    url.pathname = parts.join("/");
    return url.toString();
  } catch {
    return "";
  }
}

function arrayBufferToBase64(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

function delay(ms) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

async function buildTauriSectorScreen(invoke, payload = {}) {
  const data = await loadMobileMarketData(invoke);
  const maxSectors = clampInt(payload.max_sectors, 1, 50, DEFAULT_SECTOR_GROUP_LIMIT);
  const perSectorLimit = clampInt(payload.per_sector_limit, 1, 50, DEFAULT_PER_SECTOR_LIMIT);
  const minSectorCandidates = clampInt(payload.min_sector_candidates, 1, 500, perSectorLimit);
  const poolLimit = maxSectors * Math.max(perSectorLimit, minSectorCandidates) * SECTOR_SCREEN_POOL_MULTIPLIER;
  const criteria = {
    ...(payload.criteria || {}),
    limit: Math.max(Number(payload.criteria?.limit || 0), poolLimit),
  };
  const screen = await invoke("core_screen_with_data", { payload: { data, criteria } });
  const bySector = new Map();
  for (const item of screen.items || []) {
    const sector = item.stock?.industry || "未分组";
    if (!bySector.has(sector)) bySector.set(sector, []);
    bySector.get(sector).push(item);
  }
  const groups = [...bySector.entries()]
    .filter(([, items]) => items.length >= minSectorCandidates)
    .map(([sector, items]) => ({
      sector,
      total: items.length,
      returned: Math.min(items.length, perSectorLimit),
      average_score: items.reduce((sum, item) => sum + Number(item.score || 0), 0) / Math.max(items.length, 1),
      items: items.slice(0, perSectorLimit),
    }))
    .sort((left, right) => right.average_score - left.average_score || right.total - left.total || left.sector.localeCompare(right.sector))
    .slice(0, maxSectors);
  return {
    total: screen.total || 0,
    returned: groups.reduce((sum, group) => sum + group.returned, 0),
    sector_count: groups.length,
    groups,
    notes: ["移动端使用内置通达信数据集进行本地板块分组。"],
  };
}

async function observeTauriStock(invoke, rawCode, params) {
  const code = normalizeStockCode(rawCode);
  if (!code) throw new Error(`股票代码无效：${rawCode || ""}`);

  const data = await loadMobileMarketData(invoke);
  const stock = findMobileStock(data, code);
  if (!stock) throw new Error(`未找到股票 ${code}`);

  const notes = ["数据源：通达信移动数据包。"];
  let trend = null;
  try {
    trend = await invoke("core_trend_with_data", {
      payload: {
        data,
        request: {
          code: stock.code,
          start_date: normalizeDateParam(params.get("start_date"), "20200101"),
          end_date: normalizeDateParam(params.get("end_date"), currentSystemDateCompact()),
          series_limit: clampInt(params.get("series_limit"), 20, 500, 120),
        },
      },
    });
  } catch (error) {
    notes.push(`日线技术面不可用：${error?.message || error}`);
  }

  if (!Object.keys(data.histories || {}).length) {
    notes.push("当前移动数据包未内置历史 K 线，已展示基础行情和估值快照。");
  }

  const minutePeriod = ["1", "5", "15", "30", "60"].includes(params.get("minute_period"))
    ? params.get("minute_period")
    : "1";
  return {
    source: "tdx",
    stock,
    financial_indicators: buildMobileFinancialIndicators(stock),
    trend,
    minute_period: minutePeriod,
    minute_bars: [],
    order_book: null,
    notes,
  };
}

function findMobileStock(data, rawCode) {
  const normalized = normalizeStockCode(rawCode);
  const digits = stockCodeDigits(rawCode);
  return (data.stocks || []).find((stock) => {
    const stockCode = stock?.code || "";
    return normalizeStockCode(stockCode) === normalized || stockCodeDigits(stockCode) === digits;
  });
}

function buildMobileFinancialIndicators(stock) {
  const items = [];
  const add = (label, rawValue, formatter, tone = "neutral") => {
    if (rawValue === null || rawValue === undefined || rawValue === "") return;
    const value = Number(rawValue);
    if (!Number.isFinite(value)) return;
    items.push({ label, value: formatter(value), raw_value: value, tone });
  };

  add("市盈率(TTM)", stock.pe, formatNumber);
  add("市净率(最新)", stock.pb, formatNumber);
  if (stock.pe) add("每股收益(计算)", Number(stock.price || 0) / Number(stock.pe), (value) => `${formatNumber(value)}元`);
  if (stock.pb) add("每股净资产", Number(stock.price || 0) / Number(stock.pb), (value) => `${formatNumber(value)}元`);
  add("净资产收益率", stock.roe, formatPercent, Number(stock.roe || 0) >= 0 ? "rise" : "fall");
  add("市值", stock.market_cap_billion, (value) => `${formatNumber(value)}亿`);
  add("股息率", stock.dividend_yield, formatPercent);

  if (!items.length) return null;
  return {
    title: "最新指标",
    period: "移动数据包",
    source: "行情估值",
    items,
    notes: [],
  };
}

async function searchTauriStocks(invoke, params) {
  const query = String(params.get("q") || "").trim().toLowerCase();
  const limit = clampInt(params.get("limit"), 1, 20, STOCK_SEARCH_LIMIT);
  if (!query) return [];
  const data = await loadMobileMarketData(invoke);
  return (data.stocks || [])
    .filter((stock) => `${stock?.code || ""} ${stock?.name || ""} ${stock?.industry || ""}`.toLowerCase().includes(query))
    .slice(0, limit);
}

async function loadMobileMarketData(invoke, options = {}) {
  if (options.force) {
    mobileMarketDataPromise = null;
    mobileMarketDataSummary = null;
    mobileMarketDataMeta = null;
  }
  if (!mobileMarketDataPromise) {
    mobileMarketDataPromise = (async () => {
      try {
        return await loadCachedMobileMarketData(invoke);
      } catch (error) {
        if (options.cacheOnly) throw error;
      }
      await refreshMobileMarketData(invoke, { seed: null });
      if (mobileMarketDataPromise) return await mobileMarketDataPromise;
      return await loadCachedMobileMarketData(invoke);
    })().catch((error) => {
      mobileMarketDataPromise = null;
      throw error;
    });
  }
  return await mobileMarketDataPromise;
}

async function readMobileMarketDataCache(invoke) {
  const cached = await invoke("core_mobile_market_data_read");
  return cached?.exists && cached.data ? cached : null;
}

async function loadCachedMobileMarketData(invoke) {
  const cached = await readMobileMarketDataCache(invoke);
  if (!cached) throw new Error("手机本地股票池为空，需要联网生成。");
  return applyMobileMarketDataRecord(cached, "cache");
}

function applyMobileMarketDataRecord(record, source) {
  mobileMarketDataSummary = record.summary || {
    stock_count: record.data?.stocks?.length || 0,
    warnings: [],
  };
  mobileMarketDataMeta = {
    source,
    bytes: Number(record.bytes || 0),
    updatedAt: mobileRecordUpdatedAt(record) || record.data?.generated_at || null,
  };
  return record.data;
}

function mobileRecordUpdatedAt(record) {
  const epochMs = Number(record?.updated_at_epoch_ms);
  if (Number.isFinite(epochMs) && epochMs > 0) return new Date(epochMs).toISOString();
  return record?.updated_at || null;
}

function isLikelyTradingDay(now = new Date()) {
  const day = now.getDay();
  return day !== 0 && day !== 6;
}

function shouldUsePreviousCloseForMobileRefresh(now = new Date()) {
  if (!isLikelyTradingDay(now)) return false;
  const minutes = now.getHours() * 60 + now.getMinutes();
  return minutes < 15 * 60 + 5;
}

async function refreshMobileMarketData(invoke, options = {}) {
  let seed = Object.prototype.hasOwnProperty.call(options, "seed") ? options.seed : null;
  if (!Object.prototype.hasOwnProperty.call(options, "seed")) {
    try {
      seed = await loadCachedMobileMarketData(invoke);
    } catch {
      seed = null;
    }
  }
  mobileMarketDataPromise = null;
  mobileMarketDataSummary = null;
  mobileMarketDataMeta = null;
  const result = await invoke("core_mobile_market_data_refresh_tencent", {
    payload: {
      seed,
      scan_candidates: true,
      max_candidates: MOBILE_TENCENT_MAX_CANDIDATES,
      max_failed_batches: MOBILE_TENCENT_MAX_FAILED_BATCHES,
      max_refresh_secs: MOBILE_TENCENT_MAX_REFRESH_SECS,
      use_previous_close: shouldUsePreviousCloseForMobileRefresh(),
    },
  });
  if (result?.status?.data) {
    const data = applyMobileMarketDataRecord(result.status, "cache");
    mobileMarketDataPromise = Promise.resolve(data);
  }
  const status = await mobileDataStatus(invoke);
  status.refresh_result = result;
  status.notes = [...mobileRefreshNotes(status, "联网刷新完成"), ...(status.notes || [])];
  return status;
}

function mobileRefreshNotes(status, fallback = "") {
  const refresh = status?.refresh_result;
  if (!refresh) return fallback ? [fallback] : [];
  const fetched = Number(refresh.fetched || 0);
  const requested = Number(refresh.requested || 0);
  const preserved = Number(refresh.preserved || 0);
  const failed = Number(refresh.failed_batches || 0);
  const stoppedEarly = Boolean(refresh.stopped_early);
  const stopReason = String(refresh.stop_reason || "");
  const stockCount = Number(status?.stock_count || 0);
  const base =
    fetched > 0
      ? `已联网更新 ${fetched} 只股票行情，保留 ${preserved} 只本地股票，候选 ${requested} 只。`
      : `腾讯行情暂未返回有效股票，已保留 ${preserved || stockCount} 只本地股票。`;
  if (stoppedEarly && stopReason === "timeout") {
    return [`${base} 已达到 ${MOBILE_TENCENT_MAX_REFRESH_SECS} 秒移动端刷新上限，避免长时间等待。`];
  }
  if (stoppedEarly) return [`${base} 连续失败 ${failed} 批后提前停止，避免移动端长时间等待。`];
  if (failed > 0) return [`${base} 失败批次 ${failed} 批，其余股票已继续处理。`];
  return [base];
}

async function pruneMobileMarketData(invoke) {
  const cleared = await invoke("core_mobile_market_data_clear");
  mobileMarketDataPromise = null;
  mobileMarketDataSummary = null;
  mobileMarketDataMeta = null;
  const status = await mobileDataStatus(invoke);
  return {
    removed_files: cleared?.removed ? 1 : 0,
    removed_bytes: Number(cleared?.removed_bytes || 0),
    status,
    notes: [cleared?.removed ? "已清理手机本地股票池缓存，需要联网重新生成。" : "手机本地股票池缓存为空。"],
  };
}

async function mobileDataStatus(invoke) {
  try {
    const data = await loadMobileMarketData(invoke, { cacheOnly: true });
    const summary = mobileMarketDataSummary || { stock_count: data.stocks?.length || 0, warnings: [] };
    const meta = mobileMarketDataMeta || {};
    const stockCount = summary.stock_count || data.stocks?.length || 0;
    return {
      source: "tencent",
      universe_count: stockCount,
      cache_bytes: Number(meta.bytes || 0),
      cache_limit_bytes: 0,
      universe_updated_at: meta.updatedAt || data.generated_at || null,
      policy: { mode: "mobile_online", source: meta.source || "cache" },
      notes: [
        `移动端当前使用手机本地股票池 ${stockCount} 只，来源为腾讯联网更新。`,
        ...((data.notes || []).slice(0, 2)),
        ...(summary.warnings || []),
      ],
    };
  } catch (error) {
    return {
      source: "tencent",
      universe_count: 0,
      cache_bytes: 0,
      cache_limit_bytes: 0,
      universe_updated_at: null,
      policy: { mode: "empty" },
      notes: [`移动端股票池尚未生成：${error.message}`, "首次安装会联网生成股票池；也可以点击“联网更新股票池”重试。"],
    };
  }
}

function renderDataStatus(status) {
  if (!status || !dataSource.universe) return;
  const policy = status.policy || {};
  const empty = policy.mode === "empty";
  dataSource.universe.textContent = `${formatNumber(status.universe_count)} 只`;
  dataSource.cache.textContent = formatBytes(status.cache_bytes);
  dataSource.updated.textContent = status.universe_updated_at ? formatDateTime(status.universe_updated_at) : "-";
  if (dataSource.cacheLabel) dataSource.cacheLabel.textContent = "手机缓存";
  if (dataSource.policy) {
    dataSource.policy.textContent = empty ? "未初始化 · 需联网生成" : "手机本地 · 腾讯联网更新";
  }
  if (dataSource.refreshUniverse) dataSource.refreshUniverse.textContent = "联网更新股票池";
  if (dataSource.pruneCache) {
    dataSource.pruneCache.textContent = "清理缓存";
    dataSource.pruneCache.disabled = Number(status.cache_bytes || 0) <= 0;
  }
  setMaintenanceNote((status.notes || []).join(" "));
}

function renderScreenResult(node, data) {
  const items = data.items || [];
  renderResult(node, {
    summary: [
      ["命中", data.returned ?? items.length],
      ["候选", data.total ?? 0],
      ["最高分", items[0] ? formatNumber(items[0].score) : "-"],
    ],
    body: [
      items.length ? renderResultActions("当前筛选条件", data.returned ?? items.length) : "",
      items.length
        ? renderStockList(items.map(screenItemToView))
        : renderEmpty("没有符合条件的股票", { label: "重跑筛选", action: "run-screen" }),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderSectorScreenResult(node, data) {
  const groups = data.groups || [];
  renderResult(node, {
    summary: [
      ["板块", data.sector_count ?? groups.length],
      ["展示", data.returned ?? 0],
      ["候选", data.total ?? 0],
    ],
    body: [
      groups.length ? renderResultActions("当前分板块条件", data.returned ?? 0) : "",
      groups.length
        ? renderSectorGroups(groups)
        : renderEmpty("没有符合条件的板块", { label: "调整条件", action: "go-screen" }),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderSectorGroups(groups) {
  return `
    <div class="sector-groups">
      ${groups
        .map((group) => {
          const items = group.items || [];
          return `
            <section class="sector-group">
              <header>
                <div>
                  <h3>${escapeHtml(group.sector || "未知板块")}</h3>
                  <p>候选 ${formatNumber(group.total)} 只 · 展示 ${formatNumber(group.returned)} 只</p>
                </div>
                <strong>均分 ${formatNumber(group.average_score)}</strong>
              </header>
              ${items.length ? renderStockList(items.map(screenItemToView)) : renderEmpty("该板块没有入选股票")}
            </section>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderGraphResult(node, data) {
  const items = data.items || [];
  renderResult(node, {
    summary: [
      ["返回", data.returned ?? items.length],
      ["关系边", data.relation_count ?? 0],
      ["最高分", items[0] ? formatNumber(items[0].final_score) : "-"],
    ],
    body: [
      items.length
        ? renderStockList(items.map(graphItemToView))
        : renderEmpty("没有可传播的关系信号"),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderTrendAnalysis(node, data) {
  const signal = data.signal || {};
  const series = data.series || [];
  renderResult(node, {
    summary: [
      ["状态", statusLabel(signal.status)],
      ["量化分", `${signal.quant_score ?? 0}/${signal.quant_score_max ?? 90}`],
      ["收盘价", formatNumber(signal.close)],
    ],
    body: [
      renderSignalCard(data.stock || {}, signal),
      series.length ? renderTrendChart(series) : renderEmpty("没有可用趋势曲线"),
      signal.notes?.length ? renderNotes(signal.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderTrendScreenResult(node, data) {
  const items = data.items || [];
  renderResult(node, {
    summary: [
      ["返回", data.returned ?? items.length],
      ["候选", data.total ?? 0],
      ["最高分", items[0] ? formatNumber(items[0].final_score) : "-"],
    ],
    body: [
      items.length
        ? renderStockList(items.map(trendItemToView))
        : renderEmpty("没有趋势信号匹配当前条件"),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderBacktestResult(node, data) {
  const metrics = data.metrics || {};
  const curve = data.equity_curve || [];
  const benchmarkCurve = data.benchmark_curve || [];
  const symbols = data.symbols || [];
  renderResult(node, {
    summary: [
      ["总收益", formatPercent(metrics.total_return)],
      ["年化", formatPercent(metrics.annualized_return)],
      ["最大回撤", formatPercent(metrics.max_drawdown)],
      ["超额", formatPercent(metrics.excess_return)],
    ],
    body: [
      curve.length
        ? `<section class="backtest-primary-chart">${renderSparkline(curve)}</section>`
        : renderEmpty("没有可用净值曲线"),
      benchmarkCurve.length ? renderBenchmarkSparkline(benchmarkCurve) : "",
      renderBacktestHoldings(data),
      renderBacktestComparison(data),
      renderBacktestReliability(data, true),
    ].join(""),
    raw: data,
  });
}

function renderBacktestHoldings(data) {
  const symbols = data.symbols || [];
  if (!symbols.length) return "";
  const sourceNote = (data.notes || []).find((note) => String(note).includes("自选观察池"));
  return `
    <section class="backtest-holdings">
      <header>
        <span>回测股票</span>
        <strong>${formatNumber(symbols.length)} 只</strong>
      </header>
      <div class="symbol-strip">${symbols.map(escapeHtml).join(" · ")}</div>
      ${sourceNote ? `<p>${escapeHtml(sourceNote)}</p>` : ""}
    </section>
  `;
}

function renderBenchmarkSparkline(curve) {
  return `
    <section class="benchmark-sparkline">
      <header>
        <span>基准曲线</span>
        <strong>${formatPercent(curveReturn(curve))}</strong>
      </header>
      ${renderSparkline(curve)}
    </section>
  `;
}

function renderBacktestComparison(data) {
  const metrics = data.metrics || {};
  const benchmarkSymbols = data.benchmark_symbols || [];
  const rebalanceDates = data.rebalance_dates || [];
  return `
    <section class="backtest-comparison">
      <div>
        <span>基准</span>
        <strong>${benchmarkSymbols.length ? `${formatNumber(benchmarkSymbols.length)} 只` : "未启用"}</strong>
      </div>
      <div>
        <span>交易成本</span>
        <strong>${formatMoney(metrics.total_transaction_cost)}</strong>
      </div>
      <div>
        <span>总换手</span>
        <strong>${formatNumber(metrics.total_turnover)}</strong>
      </div>
      <div>
        <span>再平衡</span>
        <strong>${formatNumber(metrics.rebalance_count || 0)} 次</strong>
      </div>
      <div>
        <span>调仓日期</span>
        <strong>${rebalanceDates.length ? escapeHtml(rebalanceDates.slice(0, 4).join(" · ")) : "-"}</strong>
      </div>
    </section>
  `;
}

function curveReturn(curve) {
  if (!curve?.length) return null;
  const start = Number(curve[0]?.equity);
  const end = Number(curve[curve.length - 1]?.equity);
  if (!Number.isFinite(start) || !Number.isFinite(end) || start <= 0) return null;
  return end / start - 1;
}

function renderNewsRagResult(node, data) {
  renderResult(node, {
    summary: [
      ["范围", (data.scope_codes || []).length],
      ["关系边", data.relation_count ?? 0],
      ["消息", data.message_count ?? 0],
      ["判断", (data.findings || []).length],
    ],
    body: renderNewsRagBody(data),
    raw: data,
  });
}

function renderNewsRagBody(data) {
  const findings = data.findings || [];
  return [
    findings.length ? renderNewsFindings(findings) : renderEmpty("没有命中的上下游消息"),
    data.notes?.length ? renderNotes(data.notes) : "",
  ].join("");
}

function renderRagPackBuildResult(node, data) {
  renderResult(node, {
    summary: [
      ["文档", data.document_count ?? 0],
      ["切片", data.chunk_count ?? 0],
      ["向量", data.embedding_dim ?? "-"],
      ["后端", data.embedding_backend || "-"],
    ],
    body: [
      renderKeyValueBlock([
        ["路径", data.path || "-"],
        ["模型", data.embedding_model || "-"],
        ["量化", data.embedding_quantization || "-"],
        ["哈希", data.content_hash || "-"],
      ]),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderRagPackQueryResult(node, data) {
  const hits = data.hits || [];
  const manifest = data.manifest || {};
  renderResult(node, {
    summary: [
      ["命中", hits.length],
      ["版本", manifest.pack_version || "-"],
      ["模型", manifest.embedding_model || "-"],
      ["后端", manifest.embedding_backend || "-"],
    ],
    body: [
      hits.length ? renderRagPackHits(hits) : renderEmpty("离线包没有命中证据"),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderRagPackHits(hits) {
  return `
    <div class="evidence-list">
      ${hits
        .map(
          (hit) => `
            <article>
              <strong>${escapeHtml(hit.title || "-")}</strong>
              <span class="evidence-source">
                <span class="source-tier ${sourceTierClass(hit.source_tier)}">${escapeHtml(sourceTierLabel(hit.source_tier))}</span>
                ${escapeHtml(hit.source || "-")} · ${escapeHtml(hit.published_at || "-")}
              </span>
              <p>${escapeHtml(hit.text || "")}</p>
              <em>${escapeHtml((hit.stock_codes || []).join(" · "))}</em>
            </article>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderUpstreamRagBuildResult(node, build, transfer) {
  const manifest = build.manifest || {};
  const quality = build.quality || {};
  renderResult(node, {
    summary: [
      ["有效", manifest.valid ? "是" : "否"],
      ["文档", manifest.document_count ?? 0],
      ["证据", manifest.evidence_count ?? 0],
      ["关系", manifest.relation_edge_count ?? 0],
    ],
    body: [
      renderKeyValueBlock([
        ["股票", `${manifest.target_stock_name || "-"} ${manifest.target_stock_code || ""}`.trim()],
        ["版本", manifest.pack_version || "-"],
        ["包大小", formatBytes(manifest.file_size || 0)],
        ["SHA256", manifest.sha256 || "-"],
      ]),
      renderUpstreamQuality(quality, manifest),
      transfer ? renderUpstreamTransferBlock(transfer) : renderEmpty("质量门禁未通过，未开启手机同步。"),
      build.notes?.length ? renderNotes(build.notes) : "",
    ].join(""),
    raw: { build, transfer },
  });
}

function renderUpstreamRagDesktopStatus(node, data) {
  const manifest = data.manifest || {};
  renderResult(node, {
    summary: [
      ["状态", data.exists ? "已构建" : "未构建"],
      ["有效", manifest.valid ? "是" : "否"],
      ["关系", manifest.relation_edge_count ?? 0],
      ["传输", data.transfer?.active ? "已开启" : "未开启"],
    ],
    body: data.exists
      ? [
          renderKeyValueBlock([
            ["股票", `${manifest.target_stock_name || "-"} ${manifest.target_stock_code || ""}`.trim()],
            ["版本", manifest.pack_version || "-"],
            ["包大小", formatBytes(manifest.file_size || 0)],
            ["SHA256", manifest.sha256 || "-"],
          ]),
          data.transfer?.active ? renderUpstreamTransferBlock(data.transfer) : renderEmpty("当前没有局域网临时传输会话。"),
          renderUpstreamRelationGraph(manifest),
          renderUpstreamEvidenceChunks(manifest.evidence_chunks || []),
          data.notes?.length ? renderNotes(data.notes) : "",
        ].join("")
      : renderEmpty("尚未构建上下游 RAG 包。"),
    raw: data,
  });
}

function renderUpstreamRagMobileList(node, data) {
  const packs = [...(data.packs || [])].sort((left, right) => Number(Boolean(right.current)) - Number(Boolean(left.current)));
  renderResult(node, {
    summary: [
      ["本机包", packs.length],
      ["当前", packs.filter((pack) => pack.current).length],
      ["目录", data.root || "-"],
      ["状态", packs.length ? "可用" : "空"],
    ],
    body: packs.length
      ? `
        <div class="rag-pack-list">
          ${packs
            .map(
              (pack) => `
                <article>
                  <header>
                    <div>
                      <h3>${escapeHtml(pack.target_stock_name || "-")}</h3>
                      <p>${escapeHtml(pack.target_stock_code || "-")} · ${escapeHtml(pack.pack_version || "-")}</p>
                    </div>
                    <span class="pack-state ${pack.current ? "current" : ""}">${pack.current ? "当前" : "历史"}</span>
                  </header>
                  ${renderKeyValueBlock([
                    ["构建时间", formatDateTime(pack.built_at)],
                    ["包大小", formatBytes(pack.file_size || 0)],
                    ["关系", pack.relation_edge_count ?? 0],
                    ["证据", pack.evidence_count ?? 0],
                  ])}
                  <div class="button-row">
                    <button type="button" data-upstream-detail data-stock-code="${escapeHtml(pack.target_stock_code || "")}" data-pack-version="${escapeHtml(pack.pack_version || "")}">详情</button>
                    <button class="ghost-action" type="button" data-upstream-rollback data-stock-code="${escapeHtml(pack.target_stock_code || "")}">回滚</button>
                  </div>
                </article>
              `,
            )
            .join("")}
        </div>
      `
      : renderEmpty("安卓端尚未导入上下游 RAG 包。"),
    raw: data,
  });
}

function renderUpstreamRagImportResult(node, data) {
  renderResult(node, {
    summary: [
      ["导入", data.imported ? "完成" : "失败"],
      ["股票", data.stock_code || "-"],
      ["版本", data.pack_version || "-"],
      ["状态", "已校验"],
    ],
    body: [
      data.manifest ? renderUpstreamRelationGraph(data.manifest) : "",
      data.manifest ? renderUpstreamEvidenceChunks(data.manifest.evidence_chunks || []) : "",
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderUpstreamRagDetailResult(node, data) {
  const manifest = data.manifest || data;
  renderResult(node, {
    summary: [
      ["股票", manifest.target_stock_code || "-"],
      ["版本", manifest.pack_version || "-"],
      ["关系", manifest.relation_edge_count ?? 0],
      ["证据", manifest.evidence_count ?? 0],
    ],
    body: [
      renderKeyValueBlock([
        ["名称", manifest.target_stock_name || "-"],
        ["行业", manifest.target_stock_industry || "-"],
        ["数据截至", manifest.data_until || "-"],
        ["SHA256", manifest.sha256 || "-"],
      ]),
      renderUpstreamRelationGraph(manifest),
      renderUpstreamEvidenceChunks(manifest.evidence_chunks || []),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderUpstreamQuality(quality, manifest) {
  const errors = quality.errors || manifest.quality?.errors || [];
  const warnings = quality.warnings || manifest.quality?.warnings || [];
  if (!errors.length && !warnings.length) return renderEmpty("质量门禁通过，可同步到手机。");
  return `
    <div class="upstream-quality">
      ${errors.map((item) => `<span class="danger">${escapeHtml(item)}</span>`).join("")}
      ${warnings.map((item) => `<span>${escapeHtml(item)}</span>`).join("")}
    </div>
  `;
}

function renderUpstreamTransferBlock(transfer) {
  return `
    <section class="upstream-transfer">
      <header>
        <div>
          <h3>局域网临时传输</h3>
          <p>过期时间 ${escapeHtml(formatDateTime(transfer.expires_at))}</p>
        </div>
        ${transfer.qr_svg ? `<img src="${escapeHtml(transfer.qr_svg)}" alt="上下游 RAG 包导入二维码" />` : ""}
      </header>
      ${renderKeyValueBlock([
        ["Manifest", transfer.manifest_url || "-"],
        ["RAG 包", transfer.pack_url || "-"],
      ])}
      ${transfer.notes?.length ? renderNotes(transfer.notes) : ""}
    </section>
  `;
}

function renderUpstreamRelationGraph(manifest) {
  const edges = manifest.relation_edges || [];
  if (!edges.length) return renderEmpty("没有可展示的上下游关系边。");
  const target = `${manifest.target_stock_name || ""} ${manifest.target_stock_code || ""}`.trim();
  return `
    <section class="upstream-graph">
      <header>
        <h3>关系图</h3>
        <span>${escapeHtml(target || "-")}</span>
      </header>
      <div class="relation-map">
        ${edges
          .slice(0, 18)
          .map((edge) => {
            const source = edge.source_entity?.entity_name || edge.source_entity?.stock_code || "-";
            const targetName = edge.target_entity?.entity_name || edge.target_entity?.stock_code || "-";
            return `
              <article class="relation-edge ${escapeHtml(edge.status || "")}">
                <span>${escapeHtml(source)}</span>
                <strong>${escapeHtml(relationTypeLabel(edge.relation_type))}</strong>
                <span>${escapeHtml(targetName)}</span>
                <em>${escapeHtml(relationStatusLabel(edge.status))} · ${formatPercent(edge.confidence || 0)}</em>
              </article>
            `;
          })
          .join("")}
      </div>
    </section>
  `;
}

function renderUpstreamEvidenceChunks(chunks) {
  if (!chunks.length) return renderEmpty("没有可展示的证据片段。");
  return `
    <section class="upstream-evidence">
      <header>
        <h3>证据列表</h3>
        <span>${formatNumber(chunks.length)} 条预览</span>
      </header>
      <div class="evidence-list">
        ${chunks
          .slice(0, 24)
          .map(
            (item) => `
              <article>
                <strong>${escapeHtml(item.title || "-")}</strong>
                <span class="evidence-source">
                  <span class="source-tier ${sourceTierClass(item.source_tier)}">${escapeHtml(sourceTierLabel(item.source_tier))}</span>
                  ${escapeHtml(item.source_name || "-")} · ${escapeHtml(item.published_at || "-")}
                </span>
                <p>${escapeHtml(item.evidence_text || "")}</p>
              </article>
            `,
          )
          .join("")}
      </div>
    </section>
  `;
}

function renderKeyValueBlock(items) {
  return `
    <div class="detail-grid">
      ${items
        .map(
          ([label, value]) => `
            <div>
              <span>${escapeHtml(label)}</span>
              <strong>${escapeHtml(String(value))}</strong>
            </div>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderNewsFindings(findings) {
  return `
    <div class="news-findings">
      ${findings
        .map(
          (finding) => `
            <section class="news-finding">
              <header>
                <div>
                  <h3>${escapeHtml(finding.target || "-")}</h3>
                  <p>${escapeHtml(finding.impact_chain || "")}</p>
                </div>
                <span class="impact-pill ${impactClass(finding.direction)}">${escapeHtml(finding.direction || "不确定")}</span>
              </header>
              <div class="finding-meta">
                <span>置信度 ${escapeHtml(finding.confidence || "低")}</span>
                <span>证据 ${formatNumber((finding.evidence || []).length)}</span>
              </div>
              ${renderEvidenceList(finding.evidence || [])}
              ${finding.pending_checks?.length ? renderChecklist(finding.pending_checks) : ""}
            </section>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderEvidenceList(items) {
  if (!items.length) return renderEmpty("没有可引用证据");
  return `
    <div class="evidence-list">
      ${items
        .map(
          (item) => `
            <article>
              <strong>${escapeHtml(item.title || "-")}</strong>
              <span class="evidence-source">
                <span class="source-tier ${sourceTierClass(item.source_tier)}">${escapeHtml(sourceTierLabel(item.source_tier))}</span>
                ${escapeHtml(item.source || "-")} · ${escapeHtml(item.published_at || "-")}
              </span>
              ${item.summary ? `<p>${escapeHtml(item.summary)}</p>` : ""}
              <em>${escapeHtml((item.stock_codes || []).join(" · "))}</em>
            </article>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderChecklist(items) {
  return `<div class="checklist">${items.map((item) => `<span>${escapeHtml(item)}</span>`).join("")}</div>`;
}

function impactClass(direction) {
  if (direction === "利好") return "positive";
  if (direction === "利空") return "negative";
  if (direction === "中性") return "neutral";
  return "uncertain";
}

function sourceTierLabel(tier) {
  if (tier === "filing") return "公告 / 事实";
  if (tier === "financial_snapshot") return "通达信 / 财务";
  if (tier === "research") return "研报 / 摘要";
  if (tier === "manual_url") return "公开 URL";
  if (tier === "community") return "社区 / 待核查";
  return "新闻 / 事实";
}

function sourceTierClass(tier) {
  if (tier === "filing") return "filing";
  if (tier === "financial_snapshot") return "filing";
  if (tier === "manual_url" || tier === "research") return "news";
  return tier === "community" ? "community" : "news";
}

function renderResultActions(scope, count) {
  return `
    <div class="result-actions">
      <div>
        <span>下一步</span>
        <strong>${escapeHtml(scope)} · ${formatNumber(count)} 个候选</strong>
      </div>
      <button type="button" data-run-backtest>用当前条件回测</button>
    </div>
  `;
}

function renderBacktestReliability(data, collapsed = false) {
  const notes = data.notes || [];
  if (!notes.length) return "";
  if (collapsed) {
    return `
      <details class="reliability-card reliability-details">
        <summary>
          <span>可信度提示</span>
          <strong>${escapeHtml(backtestReliabilityLabel(data))}</strong>
        </summary>
        ${renderNotes(notes)}
      </details>
    `;
  }
  return `
    <section class="reliability-card">
      <header>
        <span>可信度提示</span>
        <strong>${escapeHtml(backtestReliabilityLabel(data))}</strong>
      </header>
      ${renderNotes(notes)}
    </section>
  `;
}

function backtestReliabilityLabel(data) {
  const metrics = data.metrics || {};
  const curve = data.equity_curve || [];
  const used = Number(metrics.num_stocks || 0);
  if (!curve.length || used === 0) return "低";
  if (curve.length < 30 || used < 3) return "中低";
  return "中";
}

function renderObserveResult(node, data) {
  renderResult(node, {
    summary: observeSummary(data),
    body: renderObserveBody(data),
    raw: data,
  });
}

function observeSummary(data) {
  const stock = data.stock || {};
  const minuteBars = data.minute_bars || [];
  return [
    ["数据源", sourceLabel(data.source)],
    ["最新价", formatNumber(stock.price)],
    ["分钟线", `${data.minute_period || "1"}m · ${minuteBars.length}`],
  ];
}

function renderObserveBody(data) {
  const stock = data.stock || {};
  const trend = data.trend || {};
  const signal = trend.signal || {};
  const series = trend.series || [];
  const minuteBars = data.minute_bars || [];
  return [
    renderQuoteCard(stock),
    renderFinancialIndicators(data.financial_indicators),
    data.order_book ? renderOrderBook(data.order_book) : renderEmpty("没有可用盘口"),
    trend.signal ? renderSignalCard(stock, signal) : renderEmpty("没有可用日线技术面"),
    minuteBars.length ? renderMinuteChart(minuteBars) : renderEmpty("没有可用分钟线"),
    series.length ? renderTrendChart(series) : "",
    data.notes?.length ? renderNotes(data.notes) : "",
    signal.notes?.length ? renderNotes(signal.notes) : "",
  ].join("");
}

function renderAgentResult(node, data) {
  const nested = data.data || {};
  const nestedItems = nested.items || [];
  const nestedGroups = nested.groups || [];
  const nestedMetrics = nested.metrics || {};
  const bodyParts = [`<div class="agent-reply">${escapeHtml(data.reply || "已处理")}</div>`];

  if (data.action === "observe_stock") {
    bodyParts.push(nested.stock ? renderObserveBody(nested) : renderEmpty("没有个股观察结果"));
  } else if (data.action === "sector_screen") {
    bodyParts.push(nestedGroups.length ? renderSectorGroups(nestedGroups) : renderEmpty("没有分板块选股结果"));
    if (nested.notes?.length) bodyParts.push(renderNotes(nested.notes));
  } else if (data.action === "graph_screen") {
    bodyParts.push(
      nestedItems.length
        ? renderStockList(nestedItems.map(graphItemToView))
        : renderEmpty("没有关系图结果"),
    );
    if (nested.notes?.length) bodyParts.push(renderNotes(nested.notes));
  } else if (data.action === "trend_screen") {
    bodyParts.push(
      nestedItems.length
        ? renderStockList(nestedItems.map(trendItemToView))
        : renderEmpty("没有趋势选股结果"),
    );
    if (nested.notes?.length) bodyParts.push(renderNotes(nested.notes));
  } else if (data.action === "backtest") {
    bodyParts.push(nestedMetrics.total_return !== undefined ? renderMetricLine(nestedMetrics) : "");
    bodyParts.push(nested.equity_curve?.length ? renderSparkline(nested.equity_curve) : "");
    if (nested.notes?.length) bodyParts.push(renderBacktestReliability(nested));
  } else if (data.action === "news_rag") {
    bodyParts.push(renderNewsRagBody(nested));
  } else if (data.action === "screen") {
    bodyParts.push(
      nestedItems.length
        ? renderStockList(nestedItems.map(screenItemToView))
        : renderEmpty("没有选股结果"),
    );
    if (nested.notes?.length) bodyParts.push(renderNotes(nested.notes));
  } else if (["data_status", "refresh_data", "prune_cache"].includes(data.action)) {
    const status = nested.status || nested;
    bodyParts.push(renderDataStatusCard(status));
    if (nested.notes?.length) bodyParts.push(renderNotes(nested.notes));
  } else {
    bodyParts.push(renderEmpty("智能体没有返回可展示数据"));
  }

  const thirdMetric =
    data.action === "observe_stock"
      ? ["最新价", formatNumber(nested.stock?.price)]
      : data.action === "sector_screen"
      ? ["板块", nested.sector_count ?? nestedGroups.length ?? "-"]
      : data.action === "graph_screen"
      ? ["关系边", nested.relation_count ?? "-"]
      : data.action === "trend_screen"
        ? ["最高分", nestedItems[0] ? formatNumber(nestedItems[0].final_score) : "-"]
        : data.action === "news_rag"
          ? ["消息", nested.message_count ?? "-"]
        : data.action === "data_status" || data.action === "refresh_data" || data.action === "prune_cache"
          ? ["缓存", formatBytes((nested.status || nested).cache_bytes)]
        : ["关系边", nested.relation_count ?? "-"];

  renderResult(node, {
    summary: [
      ["动作", actionLabel(data.action)],
      [
        "结果",
        data.action === "observe_stock"
          ? nested.stock?.code ?? "-"
          : data.action === "news_rag"
            ? nested.relation_count ?? "-"
          : nested.returned ?? (nested.status || nested).universe_count ?? nestedItems.length ?? "-",
      ],
      thirdMetric,
    ],
    body: bodyParts.join(""),
    raw: data,
  });
}

function renderDataStatusCard(status) {
  if (!status) return renderEmpty("没有数据维护状态");
  return `
    <section class="data-status-card">
      <div><span>数据源</span><strong>${escapeHtml(sourceLabel(status.source))}</strong></div>
      <div><span>股票池</span><strong>${formatNumber(status.universe_count)} 只</strong></div>
      <div><span>缓存</span><strong>${formatBytes(status.cache_bytes)}</strong></div>
      <div><span>上限</span><strong>${formatBytes(status.cache_limit_bytes)}</strong></div>
      <div><span>状态</span><strong>${status.stale ? "需刷新" : "可用"}</strong></div>
      <div><span>更新时间</span><strong>${escapeHtml(status.universe_updated_at ? formatDateTime(status.universe_updated_at) : "-")}</strong></div>
      ${status.notes?.length ? `<div class="wide">${renderNotes(status.notes)}</div>` : ""}
    </section>
  `;
}

function screenItemToView(item) {
  return {
    stock: item.stock,
    score: item.score,
    scoreLabel: "综合分",
    reasons: item.reasons || [],
  };
}

function graphItemToView(item) {
  return {
    stock: item.stock,
    score: item.final_score,
    scoreLabel: "最终分",
    weight: item.suggested_weight,
    reasons: item.reasons || [],
    extra: [
      ["基础", item.base_score],
      ["关系", item.relation_score],
    ],
    related: item.related || [],
  };
}

function trendItemToView(item) {
  const signal = item.signal || {};
  return {
    stock: item.stock,
    score: item.final_score,
    scoreLabel: "趋势分",
    reasons: item.reasons || [],
    signal,
    extra: [
      ["基础", item.base_score],
      ["趋势", item.trend_score],
      ["量化", signal.quant_score],
    ],
  };
}

function renderResult(node, { summary, body, raw }) {
  node.className = basePanelClass(node);
  node.innerHTML = `
    <div class="metric-strip">
      ${summary
        .map(
          ([label, value]) => `
            <div class="metric">
              <span>${escapeHtml(label)}</span>
              <strong>${escapeHtml(String(value))}</strong>
            </div>
          `,
        )
        .join("")}
    </div>
    <div class="result-body">${body}</div>
    <details class="raw-json">
      <summary>原始数据</summary>
      <pre>${escapeHtml(JSON.stringify(raw, null, 2))}</pre>
    </details>
  `;
}

function renderStockList(items) {
  return `
    <div class="quote-table">
      <div class="quote-table-head">
        <span>名称</span>
        <span>评分</span>
        <span>市盈率</span>
        <span>市净率</span>
      </div>
      <div class="stock-list">${items.map(renderStockRow).join("")}</div>
    </div>
  `;
}

function renderStockRow(item) {
  const stock = item.stock || {};
  const reasons = item.reasons?.length
    ? `<div class="tag-row">${item.reasons.map((reason) => `<span>${escapeHtml(reasonLabel(reason))}</span>`).join("")}</div>`
    : "";
  const extra = item.extra?.length
    ? `<div class="mini-metrics">${item.extra.map(([label, value]) => `<span>${escapeHtml(label)} ${formatNumber(value)}</span>`).join("")}</div>`
    : "";
  const signal = item.signal ? renderSignalSummary(item.signal) : "";
  const weight = item.weight !== undefined ? `<span class="weight">${formatPercent(item.weight)}</span>` : "";
  const related = item.related?.length ? renderRelated(item.related) : "";
  const saved = isWatchlisted(stock.code);
  const watchLabel = saved ? "已收藏" : "收藏";
  const watchButton = stock.code
    ? `<button class="watchlist-action${saved ? " saved" : ""}" type="button" data-watchlist-code="${escapeHtml(stock.code)}" data-watchlist-name="${escapeHtml(stock.name || stock.code)}" data-watchlist-industry="${escapeHtml(stock.industry || "")}" data-watchlist-source="screen" aria-pressed="${saved ? "true" : "false"}" aria-label="${escapeHtml(watchLabel)} ${escapeHtml(stock.name || stock.code)}" title="${escapeHtml(watchLabel)}"><span class="watchlist-icon" aria-hidden="true">${saved ? "★" : "☆"}</span><span class="watchlist-label">${escapeHtml(watchLabel)}</span></button>`
    : "";
  const observeButton = stock.code
    ? `<button class="observe-action" type="button" data-observe-code="${escapeHtml(stock.code)}">观察</button>`
    : "";

  return `
    <article class="stock-row">
      <div class="stock-grid">
        <div class="stock-title">
          <div>
            <strong>${escapeHtml(stock.name || stock.code || "-")}</strong>
            <span>${escapeHtml(stock.code || "")} ${escapeHtml(stock.industry || "")}</span>
          </div>
        </div>
        <div class="score-badge">
          <small>${escapeHtml(item.scoreLabel || "评分")}</small>
          <b>${formatNumber(item.score)}</b>
          ${weight}
        </div>
        <div class="quote-number">
          <strong>${formatNumber(stock.pe)}</strong>
          <span>市盈率</span>
        </div>
        <div class="quote-number">
          <strong>${formatNumber(stock.pb)}</strong>
          <span>市净率</span>
        </div>
      </div>
      <div class="row-actions">
        <div class="stock-meta">
          <span>价格 ${formatNumber(stock.price)}</span>
          <span>净资产收益率 ${formatPercent(stock.roe)}</span>
          <span>市值 ${formatNumber(stock.market_cap_billion)} 亿</span>
        </div>
        <div class="row-button-group">${watchButton}${observeButton}</div>
      </div>
      ${extra}
      ${signal}
      ${reasons}
      ${related}
    </article>
  `;
}

function renderQuoteCard(stock) {
  return `
    <section class="quote-card">
      <header>
        <div>
          <h3>${escapeHtml(stock.name || stock.code || "-")}</h3>
          <p>${escapeHtml(stock.code || "")} · ${escapeHtml(stock.industry || "未知行业")}</p>
        </div>
        <span class="quote-price">${formatNumber(stock.price)}</span>
      </header>
      <div class="quote-grid">
        <div><span>市盈率</span><strong>${formatNumber(stock.pe)}</strong></div>
        <div><span>市净率</span><strong>${formatNumber(stock.pb)}</strong></div>
        <div><span>净资产收益率</span><strong>${formatPercent(stock.roe)}</strong></div>
        <div><span>市值</span><strong>${formatNumber(stock.market_cap_billion)} 亿</strong></div>
        <div><span>股息率</span><strong>${formatPercent(stock.dividend_yield)}</strong></div>
        <div><span>是否 ST</span><strong>${stock.is_st ? "是" : "否"}</strong></div>
      </div>
    </section>
  `;
}

function renderFinancialIndicators(financial) {
  const items = (financial?.items || []).filter(
    (item) => item && item.value !== undefined && item.value !== null && item.value !== "",
  );
  if (!items.length) return "";

  const meta = [financial.period, financial.source].filter(Boolean).join(" \u00b7 ");
  return `
    <section class="financial-indicators">
      <header>
        <div>
          <h3>${escapeHtml(financial.title || "\u6700\u65b0\u6307\u6807")}</h3>
          ${meta ? `<p>${escapeHtml(meta)}</p>` : ""}
        </div>
      </header>
      <div class="financial-indicator-grid">
        ${items
          .map((item) => {
            const tone = ["rise", "fall"].includes(item.tone) ? item.tone : "neutral";
            return `
              <div class="financial-indicator-item">
                <span class="financial-indicator-label">${escapeHtml(item.label || "-")}</span>
                <strong class="financial-indicator-value ${tone}">${escapeHtml(item.value)}</strong>
              </div>
            `;
          })
          .join("")}
      </div>
      ${financial.notes?.length ? `<div class="financial-indicator-notes">${financial.notes.map(escapeHtml).join(" / ")}</div>` : ""}
    </section>
  `;
}

function renderOrderBook(book) {
  const asks = [...(book.asks || [])].sort((left, right) => right.level - left.level);
  const bids = book.bids || [];
  const rows = [
    ...asks.map((level) => ({ ...level, side: `卖${level.level}`, tone: "ask" })),
    ...bids.map((level) => ({ ...level, side: `买${level.level}`, tone: "bid" })),
  ];
  return `
    <section class="order-book">
      <header>
        <strong>五档盘口</strong>
        <span>${escapeHtml(book.timestamp || "")}</span>
      </header>
      <div class="order-book-grid">
        ${rows
          .map(
            (row) => `
              <div class="${row.tone}">
                <span>${escapeHtml(row.side)}</span>
                <strong>${formatNumber(row.price)}</strong>
                <em>${formatNumber(row.volume)}</em>
              </div>
            `,
          )
          .join("")}
      </div>
      ${book.metrics ? renderBookMetrics(book.metrics) : ""}
    </section>
  `;
}

function renderBookMetrics(metrics) {
  const entries = [
    ["今开", metrics["今开"]],
    ["最高", metrics["最高"]],
    ["最低", metrics["最低"]],
    ["昨收", metrics["昨收"]],
    ["涨跌", metrics["涨跌"]],
    ["涨幅", metrics["涨幅"]],
    ["量比", metrics["量比"]],
    ["换手", metrics["换手"]],
  ];
  return `<div class="mini-metrics">${entries.map(([label, value]) => `<span>${escapeHtml(label)} ${formatNumber(value)}</span>`).join("")}</div>`;
}

function renderSignalSummary(signal) {
  return `
    <div class="signal-summary">
      <span>${escapeHtml(statusLabel(signal.status))}</span>
      <span>量化 ${escapeHtml(String(signal.quant_score ?? 0))}/${escapeHtml(String(signal.quant_score_max ?? 90))}</span>
      <span>SWL ${formatNumber(signal.swl)}</span>
      <span>SWS ${formatNumber(signal.sws)}</span>
      <span>支撑 ${formatNumber(signal.support)}</span>
      <span>阻力 ${formatNumber(signal.resistance)}</span>
    </div>
  `;
}

function renderSignalCard(stock, signal) {
  return `
    <section class="signal-card">
      <header>
        <div>
          <h3>${escapeHtml(stock.name || stock.code || signal.code || "-")}</h3>
          <p>${escapeHtml(stock.code || signal.code || "")} · ${escapeHtml(signal.date || "")}</p>
        </div>
        <span class="state-pill">${escapeHtml(statusLabel(signal.status))}</span>
      </header>
      <div class="signal-grid">
        <div><span>收盘价</span><strong>${formatNumber(signal.close)}</strong></div>
        <div><span>SWL/SWS 线</span><strong>${formatNumber(signal.swl)} / ${formatNumber(signal.sws)}</strong></div>
        <div><span>量化分</span><strong>${escapeHtml(String(signal.quant_score ?? 0))}/${escapeHtml(String(signal.quant_score_max ?? 90))}</strong></div>
        <div><span>支撑位</span><strong>${formatNumber(signal.support)}</strong></div>
        <div><span>阻力位</span><strong>${formatNumber(signal.resistance)}</strong></div>
        <div><span>突破位</span><strong>${formatNumber(signal.breakout)}</strong></div>
        <div><span>反转位</span><strong>${formatNumber(signal.reversal)}</strong></div>
        <div><span>等待线</span><strong>${formatNumber(signal.wait_line)}</strong></div>
      </div>
      ${signal.reasons?.length ? `<div class="tag-row">${signal.reasons.map((reason) => `<span>${escapeHtml(reasonLabel(reason))}</span>`).join("")}</div>` : ""}
    </section>
  `;
}

function renderRelated(relations) {
  return `
    <div class="related-list">
      ${relations
        .slice(0, 3)
        .map(
          (relation) => `
            <div>
              <span>${escapeHtml(relationTypeLabel(relation.relation_type))}</span>
              <strong>${formatPercent(relation.weight)}</strong>
            </div>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderNotes(notes) {
  return `<div class="notes">${notes.map((note) => `<p>${escapeHtml(note)}</p>`).join("")}</div>`;
}

function renderSparkline(curve) {
  const width = 720;
  const height = 150;
  const values = curve.map((point) => Number(point.equity)).filter(Number.isFinite);
  if (values.length < 2) return renderEmpty("净值点不足");

  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const points = values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * width;
      const y = height - ((value - min) / range) * height;
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");

  return `
    <div class="chart-wrap">
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="净值曲线">
        <polyline points="${points}" fill="none" stroke="currentColor" stroke-width="4" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <div class="chart-labels">
        <span>${escapeHtml(curve[0]?.date || "")}</span>
        <span>${escapeHtml(curve[curve.length - 1]?.date || "")}</span>
      </div>
    </div>
  `;
}

function renderMinuteChart(bars) {
  const width = 720;
  const height = 170;
  const values = bars.map((bar) => Number(bar.close)).filter(Number.isFinite);
  if (values.length < 2) return renderEmpty("分钟线点不足");

  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const points = values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * width;
      const y = height - ((value - min) / range) * height;
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
  const last = bars[bars.length - 1];

  return `
    <div class="chart-wrap minute-chart">
      <div class="chart-legend">
        <span>分钟收盘</span>
        <span>${escapeHtml(last?.datetime || "")}</span>
        <span>${formatNumber(last?.close)}</span>
      </div>
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="分钟线">
        <polyline points="${points}" fill="none" stroke="var(--accent-strong)" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <div class="chart-labels">
        <span>${escapeHtml(bars[0]?.datetime || "")}</span>
        <span>${escapeHtml(last?.datetime || "")}</span>
      </div>
    </div>
  `;
}

function renderTrendChart(series) {
  const width = 720;
  const height = 190;
  const values = series
    .flatMap((point) => [point.close, point.swl, point.sws])
    .map(Number)
    .filter(Number.isFinite);
  if (values.length < 2) return renderEmpty("趋势点不足");

  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const yFor = (value) => height - ((Number(value) - min) / range) * height;
  const xFor = (index) => (index / Math.max(series.length - 1, 1)) * width;
  const linePoints = (key) =>
    series
      .map((point, index) => {
        const value = Number(point[key]);
        if (!Number.isFinite(value)) return null;
        return `${xFor(index).toFixed(2)},${yFor(value).toFixed(2)}`;
      })
      .filter(Boolean)
      .join(" ");
  const markers = series
    .map((point, index) => {
      const close = Number(point.close);
      if (!Number.isFinite(close)) return "";
      const cx = xFor(index).toFixed(2);
      const cy = yFor(close).toFixed(2);
      if (point.short_buy) return `<circle cx="${cx}" cy="${cy}" r="4.5" fill="var(--positive)" />`;
      if (point.white_exit) return `<circle cx="${cx}" cy="${cy}" r="4.5" fill="var(--danger)" />`;
      return "";
    })
    .join("");

  return `
    <div class="chart-wrap trend-chart">
      <div class="chart-legend">
        <span>收盘价</span>
        <span style="color: var(--accent-strong)">SWL</span>
        <span style="color: var(--muted)">SWS</span>
      </div>
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="趋势指标曲线">
        <polyline points="${linePoints("close")}" fill="none" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round" />
        <polyline points="${linePoints("swl")}" fill="none" stroke="var(--accent-strong)" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
        <polyline points="${linePoints("sws")}" fill="none" stroke="var(--muted)" stroke-width="2" stroke-dasharray="7 6" stroke-linecap="round" stroke-linejoin="round" />
        ${markers}
      </svg>
      <div class="chart-labels">
        <span>${escapeHtml(series[0]?.date || "")}</span>
        <span>${escapeHtml(series[series.length - 1]?.date || "")}</span>
      </div>
    </div>
  `;
}

function renderMetricLine(metrics) {
  return `
    <div class="metric-line">
      <span>总收益 ${formatPercent(metrics.total_return)}</span>
      <span>年化 ${formatPercent(metrics.annualized_return)}</span>
      <span>最大回撤 ${formatPercent(metrics.max_drawdown)}</span>
    </div>
  `;
}

function renderEmpty(text, action) {
  const button = action
    ? `<button type="button" data-empty-action="${escapeHtml(action.action)}">${escapeHtml(action.label)}</button>`
    : "";
  return `<div class="empty-state${action ? " action-empty" : ""}"><span>${escapeHtml(text)}</span>${button}</div>`;
}

function setLoading(node, text) {
  node.className = `${basePanelClass(node)} loading`;
  node.innerHTML = `<div class="loader"></div><span>${escapeHtml(text)}</span>`;
}

function setError(node, title, detail, action) {
  node.className = `${basePanelClass(node)} error`;
  const button = action
    ? `<button type="button" data-empty-action="${escapeHtml(action.action)}">${escapeHtml(action.label)}</button>`
    : "";
  node.innerHTML = `<strong>${escapeHtml(title)}</strong><p>${escapeHtml(detail || "")}</p>${button}`;
}

function basePanelClass(node) {
  const classes = ["result-panel"];
  if (node.dataset.large === "true") classes.push("compact");
  if (node.classList.contains("medium")) classes.push("medium");
  return classes.join(" ");
}

function formatNumber(value) {
  if (value === null || value === undefined || value === "") return "-";
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  if (Math.abs(number) >= 1000) return number.toLocaleString("zh-CN", { maximumFractionDigits: 0 });
  return number.toLocaleString("zh-CN", { maximumFractionDigits: 3 });
}

function formatMoney(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  return `¥${number.toLocaleString("zh-CN", { maximumFractionDigits: 0 })}`;
}

function formatPercent(value) {
  if (value === null || value === undefined || value === "") return "-";
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  return `${(number * 100).toLocaleString("zh-CN", { maximumFractionDigits: 2 })}%`;
}

function formatBytes(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let size = number;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toLocaleString("zh-CN", { maximumFractionDigits: unit === 0 ? 0 : 1 })} ${units[unit]}`;
}

function formatDateTime(value) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value || "-";
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function actionLabel(action) {
  const labels = {
    screen: "普通选股",
    observe_stock: "个股观察",
    sector_screen: "板块选股",
    graph_screen: "关系图",
    trend_screen: "趋势选股",
    backtest: "回测",
    news_rag: "上下游消息",
    data_status: "数据状态",
    refresh_data: "刷新数据",
    prune_cache: "清理缓存",
    clarify: "澄清",
  };
  return labels[action] || action || "-";
}

function sourceLabel(source) {
  const labels = {
    tdx: "通达信",
    akshare: "通达信",
    eastmoney: "通达信",
    astock: "通达信",
    tencent: "腾讯行情",
  };
  return labels[source] || source || "-";
}

function statusLabel(status) {
  const labels = {
    buy_setup: "短买信号",
    uptrend: "上升趋势",
    hold: "红色持股",
    watch: "青色观望",
    exit: "白色离场",
    oversold: "急速超跌",
    neutral: "中性",
  };
  return labels[status] || status || "-";
}

function reasonLabel(reason) {
  const labels = {
    roe_ok: "净资产收益率达标",
    pe_ok: "市盈率达标",
    pb_ok: "市净率达标",
    mcap_ok: "市值达标",
    strong_relation_signal: "强关系信号",
    moderate_relation_signal: "中等关系信号",
    short_buy_signal: "短买",
    red_hold: "红色持股",
    swl_above_sws: "SWL 强于 SWS",
    high_quant_score: "量化分较高",
    white_exit: "白色离场",
    cyan_watch: "青色观望",
    oversold: "急速超跌",
  };
  return labels[reason] || reason;
}

function relationTypeLabel(type) {
  const labels = {
    industry_peer: "同行业",
    supply_chain: "供应链",
    thematic: "主题相关",
    manufacturing_chain: "制造链",
    upstream_material: "上游材料",
    valuation_peer: "估值相似",
    size_peer: "市值相近",
    supplier: "上游供应",
    customer: "下游客户",
    raw_material_price: "原材料",
    product: "产品应用",
    capacity: "产能项目",
    financial: "财务表现",
    risk: "风险事项",
    event: "披露事项",
  };
  return labels[type] || type || "关系";
}

function relationStatusLabel(status) {
  const labels = {
    confirmed: "已确认",
    supported: "有证据",
    inferred: "推断",
    rumor: "待核查",
  };
  return labels[status] || status || "-";
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function initTheme() {
  const theme = document.documentElement.dataset.theme || getPreferredTheme();
  applyTheme(theme);
  themeToggle?.addEventListener("change", () => {
    const nextTheme = themeToggle.checked ? "dark" : "light";
    localStorage.setItem(THEME_KEY, nextTheme);
    applyTheme(nextTheme);
  });
}

function getPreferredTheme() {
  const saved = localStorage.getItem(THEME_KEY);
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  if (themeToggle) themeToggle.checked = theme === "dark";
  if (themeText) themeText.textContent = theme === "dark" ? "暗色模式" : "亮色模式";
}
