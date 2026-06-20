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
const DATA_PROXY_KEY = "gp-assistant-proxy-mode";
const AUTO_REFRESH_CHECK_KEY = "gp-assistant-auto-refresh-last-check";
const AUTO_REFRESH_CHECK_INTERVAL_MS = 30 * 60 * 1000;
const LLM_SETTINGS_KEY = "gp-assistant-llm-settings";
const WATCHLIST_KEY = "gp-assistant-watchlist";
const DEFAULT_RESULT_LIMIT = 10;
const DEFAULT_SECTOR_GROUP_LIMIT = 12;
const DEFAULT_PER_SECTOR_LIMIT = 5;
const SECTOR_SCREEN_POOL_MULTIPLIER = 8;
const SCREENING_RULES_URL = new URL("screening_rules.json", document.currentScript?.src || window.location.href).toString();
const DEFAULT_SCREENING_RULES = {
  version: 2,
  score_scale: 20,
  group_limit: 10,
  potential_score_threshold: 10,
  factor_weights: { theme: 0.24, fundamental: 0.24, valuation: 0.24, size: 0.14, risk: 0.14 },
  theme_promotion_order: ["materials", "ai_chain", "semiconductor_wafer", "tech", "energy", "game"],
  theme_fill_order: ["ai_chain", "semiconductor_wafer", "materials", "tech", "energy", "game"],
  theme_categories: [
    { key: "materials", label: "新材料", score: 0.96, keywords: ["氟化工", "氟材料", "锂电材料", "电解液", "六氟磷酸锂", "新能材", "新材料", "固态电池", "磁材"] },
    { key: "semiconductor_wafer", label: "半导体晶圆", score: 0.9, keywords: ["半导体晶圆", "晶圆", "晶圆代工", "晶圆制造", "晶圆厂", "硅晶圆", "硅片", "外延片", "外延硅片", "半导体衬底", "衬底", "碳化硅衬底", "sic衬底", "抛光片", "8英寸", "12英寸"] },
    { key: "ai_chain", label: "AI算力与芯片", score: 0.95, keywords: ["半导体", "芯片", "算力", "人工智能", "ai", "光模块", "cpo", "服务器", "液冷", "gpu", "hbm", "存储", "数据中心", "云计算", "大模型", "aigc", "边缘计算", "pcb", "封装", "封测", "eda", "soc"] },
    { key: "tech", label: "科技制造", score: 0.84, keywords: ["机器人", "软件", "通信", "科技", "电子", "自动化", "高端制造", "智能制造"] },
    { key: "energy", label: "新能源", score: 0.82, keywords: ["新能源", "电池", "储能", "光伏", "电力", "能源", "油气", "煤炭", "风电", "充电桩"] },
    { key: "game", label: "游戏传媒", score: 0.78, keywords: ["游戏", "网络游戏", "手游", "电竞", "云游戏", "互动娱乐", "文化传媒", "传媒"] },
  ],
  concept_groups: [
    { label: "半导体晶圆", keywords: ["半导体晶圆", "晶圆", "晶圆代工", "晶圆制造", "晶圆厂", "硅晶圆", "硅片", "外延片", "外延硅片", "半导体衬底", "衬底", "碳化硅衬底", "sic衬底", "抛光片", "8英寸", "12英寸"] },
    { label: "AI算力与芯片", keywords: ["半导体", "芯片", "算力", "人工智能", "ai", "光模块", "cpo", "服务器", "液冷", "gpu", "hbm", "存储", "数据中心", "云计算", "大模型", "aigc", "边缘计算", "pcb", "封装", "封测", "eda", "soc"] },
    { label: "新材料", keywords: ["氟化工", "氟材料", "锂电材料", "电解液", "六氟磷酸锂", "新能材", "新材料", "固态电池", "磁材"] },
    { label: "新能源与储能", keywords: ["新能源", "电池", "储能", "光伏", "电力", "能源", "风电", "充电桩"] },
    { label: "游戏传媒", keywords: ["游戏", "网络游戏", "手游", "电竞", "云游戏", "互动娱乐", "传媒", "广告营销"] },
    { label: "机器人与高端制造", keywords: ["机器人", "工业母机", "自动化", "高端制造", "智能制造", "机械设备"] },
    { label: "消费零售", keywords: ["食品", "饮料", "白酒", "休闲食品", "一般零售", "商贸零售", "家电", "旅游", "酒店", "餐饮"] },
    { label: "医药医疗", keywords: ["医药", "医疗", "生物制品", "创新药", "中药", "化学制药", "医疗器械", "cro"] },
    { label: "金融地产", keywords: ["银行", "证券", "保险", "房地产", "地产", "物业"] },
    { label: "基建建筑", keywords: ["建筑", "房屋建设", "工程建设", "基础建设", "水泥", "铁路", "公路", "装修装饰"] },
    { label: "周期资源", keywords: ["煤炭", "钢铁", "普钢", "有色", "金属", "化工", "石油", "油气", "矿业"] },
    { label: "汽车产业链", keywords: ["汽车", "整车", "零部件", "轮胎", "智能驾驶", "无人驾驶", "汽车服务"] },
    { label: "军工航天", keywords: ["军工", "航天", "航空", "卫星", "船舶", "无人机", "国防"] },
    { label: "交运物流", keywords: ["物流", "航运", "港口", "机场", "航空运输", "铁路运输", "快递"] },
    { label: "公用环保", keywords: ["环保", "水务", "燃气", "供热", "公用事业"] },
  ],
  cold_keywords: ["银行", "基建", "建筑", "建筑装饰", "工程建设", "基础建设", "水泥", "铁路", "公路"],
};
let screeningRules = normalizeScreeningRules(DEFAULT_SCREENING_RULES);
const screeningRulesPromise = loadScreeningRules();
const STOCK_SEARCH_LIMIT = 5;
const DEFAULT_OBSERVE_TRADING_DAYS = 10;
const DEFAULT_DATA_SOURCE = "tdx";
const MOBILE_TENCENT_MAX_CANDIDATES = 6000;
const MOBILE_TENCENT_MAX_FAILED_BATCHES = 4;
const MOBILE_TENCENT_MAX_REFRESH_SECS = 45;
const MOBILE_TENCENT_INVOKE_TIMEOUT_MS = (MOBILE_TENCENT_MAX_REFRESH_SECS + 15) * 1000;
const DEFAULT_TODAY_DATE_INPUT_IDS = new Set(["trendEnd", "btEnd", "observeEnd"]);
let mobileMarketDataPromise = null;
let mobileMarketDataSummary = null;
let mobileMarketDataMeta = null;
let autoRefreshInFlight = false;
let observeRequestId = 0;
let observeTaskId = 0;
let activeObserveController = null;
const OBSERVE_REQUEST_TIMEOUT_MS = 180 * 1000;
const panelProgressTimers = new WeakMap();
const buttonTaskStates = new WeakMap();
const dataSource = {
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
  contextBar: $(".research-context-bar"),
  criteriaOpen: $("#openCriteriaBtn"),
  criteriaClose: $("#closeCriteriaBtn"),
  criteriaOverlay: $("#criteriaOverlay"),
  criteriaPanel: $("#sectionFilters"),
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
  hint: $("#llmHint"),
  save: $("#llmSaveBtn"),
  clear: $("#llmClearBtn"),
};

const agentUi = {
  thread: $("#agentChatThread"),
  detail: $("#agentResult"),
  input: $("#agentMsg"),
  settingsButton: $("#agentSettingsBtn"),
  settingsPanel: $("#llmSettingsPanel"),
  modelStatus: $("#agentModelStatus"),
  quickPrompts: document.querySelectorAll("[data-agent-prompt]"),
};
const agentRuns = new Map();

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
  "/api/observe": async ({ invoke, payload }) => {
    const params = new URLSearchParams();
    for (const key of ["start_date", "end_date", "series_limit"]) {
      if (payload?.[key] !== undefined && payload?.[key] !== null && payload?.[key] !== "") {
        params.set(key, String(payload[key]));
      }
    }
    return observeTauriStock(invoke, payload?.code || "", params);
  },
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

async function loadScreeningRules() {
  try {
    const response = await fetch(SCREENING_RULES_URL, { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    screeningRules = normalizeScreeningRules(await response.json());
  } catch {
    screeningRules = normalizeScreeningRules(DEFAULT_SCREENING_RULES);
  }
  return screeningRules;
}

function normalizeScreeningRules(payload = {}) {
  const conceptGroups = (payload.concept_groups || [])
    .map((group) => ({
      label: String(group?.label || group?.key || "").trim(),
      keywords: (group?.keywords || [])
        .map((keyword) => String(keyword || "").trim().toLowerCase())
        .filter(Boolean),
    }))
    .filter((group) => group.label && group.keywords.length);
  const themeCategories = (payload.theme_categories || [])
    .map((group) => ({
      key: String(group?.key || "").trim(),
      label: String(group?.label || group?.key || "").trim(),
      score: Number(group?.score || 0),
      keywords: (group?.keywords || [])
        .map((keyword) => String(keyword || "").trim().toLowerCase())
        .filter(Boolean),
    }))
    .filter((group) => group.key && group.label && group.keywords.length);
  return {
    version: Number(payload.version || 0),
    score_scale: Number(payload.score_scale || 20),
    group_limit: clampInt(payload.group_limit, 1, 50, 10),
    potential_score_threshold: Number.isFinite(Number(payload.potential_score_threshold))
      ? Number(payload.potential_score_threshold)
      : 10,
    factor_weights: {
      theme: Number(payload.factor_weights?.theme || 0.24),
      fundamental: Number(payload.factor_weights?.fundamental || 0.24),
      valuation: Number(payload.factor_weights?.valuation || 0.24),
      size: Number(payload.factor_weights?.size || 0.14),
      risk: Number(payload.factor_weights?.risk || 0.14),
    },
    theme_promotion_order: (payload.theme_promotion_order || []).map((value) => String(value || "").trim()).filter(Boolean),
    theme_fill_order: (payload.theme_fill_order || []).map((value) => String(value || "").trim()).filter(Boolean),
    theme_categories: themeCategories,
    concept_groups: conceptGroups,
    cold_keywords: (payload.cold_keywords || []).map((value) => String(value || "").trim().toLowerCase()).filter(Boolean),
  };
}

function getScreeningRules() {
  return screeningRulesPromise || Promise.resolve(screeningRules);
}

function screeningConceptGroups(rules = screeningRules) {
  return rules?.concept_groups || [];
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
  agentUi.settingsButton?.addEventListener("click", () => toggleAgentSettings());
  agentUi.input?.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || (!event.ctrlKey && !event.metaKey)) return;
    event.preventDefault();
    runTask(buttons.agent, panels.agent, runAgent);
  });
  agentUi.quickPrompts.forEach((button) => {
    button.addEventListener("click", () => {
      if (!agentUi.input) return;
      agentUi.input.value = button.dataset.agentPrompt || "";
      agentUi.input.focus();
    });
  });
  buttons.observe?.addEventListener("click", () => runObserveTask());
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
    const external = target?.closest("[data-external-url]");
    if (external) {
      event.preventDefault();
      openExternalUrl(external.dataset.externalUrl || external.getAttribute("href") || "");
      return;
    }
    const watch = target?.closest("[data-watchlist-code]");
    if (watch) {
      event.preventDefault();
      toggleWatchlistFromElement(watch);
      return;
    }
    const action = target?.closest("[data-observe-code]");
    if (!action) return;
    event.preventDefault();
    runObserveTask(action.dataset.observeCode);
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
  syncResearchContextBar(normalized);
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
  if (workbench.criteriaPanel) {
    workbench.criteriaPanel.hidden = !isOpen;
    workbench.criteriaPanel.toggleAttribute("inert", !isOpen);
    workbench.criteriaPanel.setAttribute("aria-hidden", String(!isOpen));
  }
  if (workbench.criteriaOverlay) {
    workbench.criteriaOverlay.hidden = !isOpen;
  }
}

function syncResearchContextBar(activeView) {
  if (!workbench.contextBar) return;
  const visible = activeView === "screen";
  workbench.contextBar.hidden = !visible;
  workbench.contextBar.setAttribute("aria-hidden", String(!visible));
  window.dispatchEvent(new Event("resize"));
}

function initStickyOffsets() {
  const shell = $(".app-shell");
  const header = $(".app-header");
  const contextBar = workbench.contextBar;
  if (!shell || !header) return;

  const update = () => {
    const headerHeight = Math.ceil(header.getBoundingClientRect().height);
    const contextHeight = contextBar && !contextBar.hidden ? Math.ceil(contextBar.getBoundingClientRect().height) : 0;
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
  if (!button) {
    try {
      await task();
    } finally {
      updateCriteriaSummary();
    }
    return;
  }

  const state = buttonTaskStates.get(button) || { original: button.textContent, active: 0 };
  state.active += 1;
  buttonTaskStates.set(button, state);
  button.disabled = true;
  button.textContent = "运行中";
  try {
    await task();
  } finally {
    state.active -= 1;
    if (state.active <= 0) {
      button.disabled = false;
      button.textContent = state.original;
      buttonTaskStates.delete(button);
    }
    updateCriteriaSummary();
  }
}

async function runScreen() {
  if ($("#sectorMode")?.checked) {
    await runSectorScreen();
    return;
  }

  const timer = startPanelProgress(panels.screen, "全市场筛选中", [
    [14, "读取股票池缓存"],
    [30, "批量获取行情价格"],
    [48, "合并东财财报指标"],
    [66, "应用扣非净利润规则"],
    [84, "计算综合评分"],
    [94, "生成热门股和潜力股"],
  ]);
  const payload = buildCriteria();
  try {
    const data = await postJson("/api/screen", payload, panels.screen);
    if (data) renderScreenResult(panels.screen, data);
  } finally {
    stopPanelProgress(panels.screen, timer);
  }
}

async function runSectorScreen() {
  const timer = startPanelProgress(panels.screen, "按概念筛选中", [
    [14, "读取股票池缓存"],
    [30, "批量获取行情价格"],
    [48, "合并东财财报指标"],
    [66, "应用当前研究条件"],
    [84, "按概念汇总候选"],
    [94, "生成分组结果"],
  ]);
  const maxSectors = clampInt($("#maxSectors")?.value, 1, 50, DEFAULT_SECTOR_GROUP_LIMIT);
  const perSectorLimit = clampInt($("#perSectorLimit")?.value, 1, 50, DEFAULT_PER_SECTOR_LIMIT);
  const payload = {
    criteria: buildCriteria(),
    max_sectors: maxSectors,
    per_sector_limit: perSectorLimit,
    min_sector_candidates: perSectorLimit,
  };
  try {
    const data = await postJson("/api/sector-screen", payload, panels.screen);
    if (data) renderSectorScreenResult(panels.screen, data);
  } finally {
    stopPanelProgress(panels.screen, timer);
  }
}

async function runGraph() {
  const timer = startPanelProgress(panels.graph, "图谱选股中", [
    [18, "构建基础候选池"],
    [42, "解析种子/主题中心"],
    [68, "计算关系传播分"],
    [90, "生成解释信息"],
  ]);
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    seed_codes: await resolveStockCodeListInput("seedCodes", panels.graph),
    relation_depth: clampInt($("#relationDepth").value, 1, 3, 1),
    relation_weight: clampFloat($("#relationWeight").value, 0, 1, 0.4),
    limit: Math.min(readInt("resultLimit", DEFAULT_RESULT_LIMIT), 100),
  };
  try {
    const data = await postJson("/api/graph-screen", payload, panels.graph);
    if (data) renderGraphResult(panels.graph, data);
  } finally {
    stopPanelProgress(panels.graph, timer);
  }
}

async function runTrendAnalysis() {
  const code = await resolveStockCodeInput("trendCode", panels.trend);
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
  const timer = startPanelProgress(panels.trend, "趋势择时选股中", [
    [18, "构建基础候选池"],
    [42, "计算日线指标"],
    [68, "排序短线买点"],
    [90, "生成解释信息"],
  ]);
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    start_date: readDateParam("trendStart", "20200101"),
    end_date: readDateParam("trendEnd", currentSystemDateCompact()),
    limit: Math.min(readInt("resultLimit", DEFAULT_RESULT_LIMIT), 100),
  };
  try {
    const data = await postJson("/api/trend-screen", payload, panels.trend);
    if (data) renderTrendScreenResult(panels.trend, data);
  } finally {
    stopPanelProgress(panels.trend, timer);
  }
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
  const code = await resolveStockCodeInput("newsCode", panels.newsRag);
  if (!code) {
    setError(panels.newsRag, "请输入目标股票代码", "上下游消息分析需要明确的单只目标股票，例如：300750.SZ。");
    return;
  }
  $("#newsCode").value = code;
  const mobileRuntime = isMobileTauriRuntime();
  const timer = startPanelProgress(
    panels.newsRag,
    mobileRuntime ? "手机端消息分析中" : "拉取利好/利空消息中",
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
          [62, "检索消息证据"],
          [82, "整理利好/利空"],
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
    if (data) {
      updateLlmStatusFromRagResult(data);
      renderNewsRagResult(panels.newsRag, data);
    }
  } finally {
    stopPanelProgress(panels.newsRag, timer);
  }
}

async function runRagPackBuildFromNewsCache() {
  setLoading(panels.newsRag, "构建离线 RAG pack");
  const code = await resolveStockCodeInput("newsCode", panels.newsRag);
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
  const code = await resolveStockCodeInput("newsCode", panels.newsRag);
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
  const code = await resolveStockCodeInput("newsCode", panels.newsRag);
  if (!code) {
    stopPanelProgress(panels.newsRag, timer);
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
    stopPanelProgress(panels.newsRag, timer);
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

async function runAgentLegacy() {
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
  if (data) {
    updateLlmStatusFromAgentResult(data);
    renderAgentResult(panels.agent, data);
  }
}

async function runAgent() {
  const input = agentUi.input || $("#agentMsg");
  const message = String(input?.value || "").trim();
  if (!message) {
    setError(panels.agent, "请输入助手指令", "例如：筛选银行股，PE 低于 10。");
    input?.focus();
    return;
  }

  const runId = createAgentRunId();
  appendAgentUserMessage(message);
  const run = createAgentAssistantRun(runId);
  setLoading(panels.agent, "等待智能体结果流");
  if (input) input.value = "";

  const payload = { message };
  const llm = buildLlmConfig();
  if (llm) payload.llm = llm;

  try {
    await requestAgentStream(runId, payload, (event) => handleAgentStreamEvent(run, event));
  } catch (error) {
    failAgentAssistantRun(run, error.message || "智能体运行失败");
    setError(panels.agent, "智能体运行失败", error.message || "请求没有返回可展示结果。");
  }
}

function toggleAgentSettings() {
  if (!agentUi.settingsPanel) return;
  agentUi.settingsPanel.open = !agentUi.settingsPanel.open;
  if (agentUi.settingsPanel.open) {
    agentUi.settingsPanel.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }
}

function createAgentRunId() {
  return window.crypto?.randomUUID?.() || `agent-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function appendAgentUserMessage(message) {
  if (!agentUi.thread) return;
  const article = document.createElement("article");
  article.className = "agent-message user";
  article.innerHTML = `
    <div class="agent-message-meta">
      <span>你</span>
      <time>${escapeHtml(agentTimeLabel())}</time>
    </div>
    <div class="agent-message-body"><p>${escapeHtml(message)}</p></div>
  `;
  agentUi.thread.appendChild(article);
  scrollAgentThreadToBottom();
}

function createAgentAssistantRun(runId) {
  const article = document.createElement("article");
  article.className = "agent-message assistant active";
  article.dataset.agentRunId = runId;
  article.innerHTML = `
    <div class="agent-message-meta">
      <span>助手</span>
      <time>${escapeHtml(agentTimeLabel())}</time>
    </div>
    <div class="agent-message-body">
      <div class="agent-stream-steps"></div>
      <p class="agent-final-reply">正在准备...</p>
    </div>
  `;
  const run = {
    id: runId,
    article,
    stepsNode: article.querySelector(".agent-stream-steps"),
    replyNode: article.querySelector(".agent-final-reply"),
    steps: [],
    response: null,
  };
  article.addEventListener("click", () => {
    if (run.response) renderAgentResult(panels.agent, run.response);
  });
  agentRuns.set(runId, run);
  agentUi.thread?.appendChild(article);
  scrollAgentThreadToBottom();
  return run;
}

function handleAgentStreamEvent(run, rawEvent) {
  const event = normalizeAgentStreamEvent(rawEvent);
  if (!event) return;
  if (event.type === "status") {
    appendAgentStreamStep(run, event);
    return;
  }
  if (event.type === "result") {
    finishAgentAssistantRun(run, event.response || {});
    return;
  }
  if (event.type === "error") {
    failAgentAssistantRun(run, event.message || "智能体返回错误");
  }
}

function normalizeAgentStreamEvent(rawEvent) {
  if (!rawEvent) return null;
  const event = rawEvent.payload || rawEvent;
  if (typeof event === "string") {
    try {
      return JSON.parse(event);
    } catch {
      return null;
    }
  }
  return event;
}

function appendAgentStreamStep(run, event) {
  const stage = String(event.stage || `stage-${run.steps.length}`);
  const existing = run.steps.find((item) => item.stage === stage);
  const step = {
    stage,
    label: event.label || stage,
    percent: Number(event.percent || 0),
    action: event.action || "",
  };
  if (existing) Object.assign(existing, step);
  else run.steps.push(step);
  renderAgentStreamSteps(run);
  if (run.replyNode) {
    run.replyNode.textContent = step.label;
  }
  scrollAgentThreadToBottom();
}

function renderAgentStreamSteps(run) {
  if (!run.stepsNode) return;
  run.stepsNode.innerHTML = run.steps
    .map(
      (step) => `
        <div class="agent-stream-step">
          <span>${escapeHtml(step.label)}</span>
          <strong>${Math.max(0, Math.min(100, step.percent || 0))}%</strong>
        </div>
      `,
    )
    .join("");
}

function finishAgentAssistantRun(run, response) {
  run.response = response;
  run.article?.classList.remove("active");
  run.article?.classList.add("complete");
  if (run.replyNode) {
    const action = response.action ? ` · ${actionLabel(response.action)}` : "";
    run.replyNode.innerHTML = `${escapeHtml(response.reply || "已完成。")}${escapeHtml(action)}`;
  }
  updateLlmStatusFromAgentResult(response);
  renderAgentResult(panels.agent, response);
  scrollAgentThreadToBottom();
}

function failAgentAssistantRun(run, message) {
  run.article?.classList.remove("active");
  run.article?.classList.add("error");
  if (run.replyNode) {
    run.replyNode.textContent = sanitizeRuntimeMessage(message || "运行失败", 180);
  }
  scrollAgentThreadToBottom();
}

async function requestAgentStream(runId, payload, onEvent) {
  const requestPayload = { ...payload, run_id: runId };
  try {
    if (isMobileTauriRuntime()) {
      await requestTauriAgentStream(runId, requestPayload, onEvent);
    } else {
      await requestDesktopAgentStream(requestPayload, onEvent);
    }
  } catch (streamError) {
    onEvent({
      run_id: runId,
      type: "status",
      stage: "fallback",
      label: "流式接口不可用，切换同步接口",
      percent: 72,
    });
    const fallback = await postJson("/api/agent", requestPayload, panels.agent);
    if (!fallback) throw streamError;
    onEvent({ run_id: runId, type: "result", response: fallback });
  }
}

async function requestDesktopAgentStream(payload, onEvent) {
  const response = await fetch("/api/agent/stream", {
    method: "POST",
    headers: { "Content-Type": "application/json", ...dataSourceHeaders() },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }
  if (!response.body?.getReader) throw new Error("浏览器不支持流式读取。");
  await consumeSseResponse(response, onEvent);
}

async function requestTauriAgentStream(runId, payload, onEvent) {
  const invoke = window.__TAURI__?.core?.invoke;
  const listen = window.__TAURI__?.event?.listen;
  if (!invoke || !listen) throw new Error("移动端事件接口不可用。");

  let unlisten = null;
  let sawResult = false;
  try {
    unlisten = await listen("agent-stream-event", (event) => {
      const payloadEvent = normalizeAgentStreamEvent(event);
      if (!payloadEvent || payloadEvent.run_id !== runId) return;
      if (payloadEvent.type === "result") sawResult = true;
      onEvent(payloadEvent);
    });
    const response = await invoke("core_agent_stream_with_data", {
      payload: {
        data: await loadMobileMarketData(invoke),
        message: payload.message || "",
        run_id: runId,
      },
    });
    if (!sawResult && response) {
      onEvent({ run_id: runId, type: "result", response });
    }
  } finally {
    if (typeof unlisten === "function") unlisten();
  }
}

async function consumeSseResponse(response, onEvent) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const blocks = buffer.split(/\r?\n\r?\n/);
    buffer = blocks.pop() || "";
    for (const block of blocks) {
      const event = parseSseBlock(block);
      if (event) onEvent(event);
    }
  }
  buffer += decoder.decode();
  const trailing = parseSseBlock(buffer);
  if (trailing) onEvent(trailing);
}

function parseSseBlock(block) {
  const lines = String(block || "").split(/\r?\n/);
  let eventType = "message";
  const dataLines = [];
  for (const line of lines) {
    if (line.startsWith("event:")) eventType = line.slice(6).trim();
    if (line.startsWith("data:")) dataLines.push(line.slice(5).trimStart());
  }
  if (!dataLines.length) return null;
  try {
    const payload = JSON.parse(dataLines.join("\n"));
    if (!payload.type) payload.type = eventType;
    return payload;
  } catch {
    return { type: eventType, message: dataLines.join("\n") };
  }
}

function agentTimeLabel() {
  return new Date().toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
}

function scrollAgentThreadToBottom() {
  if (!agentUi.thread) return;
  window.requestAnimationFrame(() => {
    agentUi.thread.scrollTop = agentUi.thread.scrollHeight;
  });
}

async function runObserveTask(codeOverride) {
  const taskId = ++observeTaskId;
  if (activeObserveController) activeObserveController.abort(createRequestAbortError("观察请求已切换", "AbortError"));
  const button = buttons.observe;
  if (button && !button.dataset.originalText) button.dataset.originalText = button.textContent;
  if (button) {
    button.disabled = true;
    button.textContent = "运行中";
  }
  try {
    await runObserve(codeOverride, taskId);
  } finally {
    if (taskId === observeTaskId && button) {
      button.disabled = false;
      button.textContent = button.dataset.originalText || "观察";
      delete button.dataset.originalText;
    }
    updateCriteriaSummary();
  }
}

async function runObserve(codeOverride, taskId = observeTaskId) {
  if (codeOverride) {
    const overrideCode = normalizeStockCode(codeOverride);
    if (overrideCode) $("#observeCode").value = overrideCode;
  }
  const code = codeOverride ? normalizeStockCode(codeOverride) : await resolveStockCodeInput("observeCode", panels.observe);
  if (!code) {
    observeRequestId += 1;
    if (activeObserveController) {
      activeObserveController.abort(createRequestAbortError("观察请求已取消", "AbortError"));
      activeObserveController = null;
    }
    setError(panels.observe, "请输入股票代码", "例如：300750.SZ", {
      label: "回到筛选页选一只观察",
      action: "go-observe-screen",
    });
    return;
  }
  $("#observeCode").value = code;
  activateWorkbenchView("observe", { href: "#sectionObserve", updateHash: true });
  const requestId = ++observeRequestId;
  const controller = typeof AbortController !== "undefined" ? new AbortController() : null;
  activeObserveController = controller;
  const timer = startPanelProgress(panels.observe, "观察分析中", [
    [14, "校验股票代码"],
    [32, "读取日线行情"],
    [52, "计算 KDJ 与趋势推断"],
    [74, "拉取资金与机构席位信息"],
    [94, "整理统一观察结果"],
  ]);
  const payload = {
    code,
    series_limit: 160,
    include_order_book: false,
    include_chip_distribution: true,
  };
  const observeEndInput = $("#observeEnd")?.value || currentSystemDateInputValue();
  payload.start_date = readDateParam("observeStart", defaultObserveStartCompact(observeEndInput));
  payload.end_date = readDateParam("observeEnd", currentSystemDateCompact());
  const llm = buildLlmConfig();
  const mobileRuntime = isMobileTauriRuntime();
  if (llm && !mobileRuntime) payload.llm = llm;
  try {
    const data = await requestJson("POST", "/api/observe", payload, dataSourceHeaders(), {
      signal: controller?.signal,
      timeoutMs: OBSERVE_REQUEST_TIMEOUT_MS,
    });
    if (requestId === observeRequestId && data) renderObserveResult(panels.observe, data);
  } catch (err) {
    if (err.name !== "AbortError" && requestId === observeRequestId) setError(panels.observe, "请求异常", err.message);
  } finally {
    if (activeObserveController === controller) activeObserveController = null;
    if (requestId === observeRequestId) stopPanelProgress(panels.observe, timer);
  }
}

function initDataSource() {
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
  const title =
    source === "watchlist" && canUseWatchlist
      ? "等待自选组合回测"
      : canUseWatchlist
        ? `自选观察池已有 ${items.length} 只股票`
        : "等待回测";
  const detail =
    source === "watchlist" && canUseWatchlist
      ? "点击运行回测，验证收藏股票的组合表现。"
      : canUseWatchlist
        ? "可以继续用当前筛选条件回测，也可以切到自选观察池。"
        : "先运行筛选生成候选组合，或从筛选结果收藏股票后回测。";
  panels.backtest.innerHTML = renderBacktestEmptyState(title, detail, action);
}

function renderBacktestEmptyState(title, detail, action) {
  const button = action
    ? `<button type="button" data-empty-action="${escapeHtml(action.action)}">${escapeHtml(action.label)}</button>`
    : "";
  return `
    <div class="backtest-empty-state">
      <strong>${escapeHtml(title)}</strong>
      <span>${escapeHtml(detail)}</span>
      ${button}
    </div>
  `;
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
  return DEFAULT_DATA_SOURCE;
}

function normalizeDataSource(source) {
  const value = String(source || "").trim().toLowerCase();
  if (["tdx", "astock", "akshare", "eastmoney"].includes(value)) return "tdx";
  return DEFAULT_DATA_SOURCE;
}

function dataSourceHeaders() {
  const headers = { "X-Stock-Provider": getSelectedDataSource() };
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
  const proxySuffix = getSelectedProxyMode() === "none" ? " 直连" : "";
  dataSource.status.innerHTML = `<i aria-hidden="true"></i>${escapeHtml(label + proxySuffix)}`;
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
  stopPanelProgress(node);
  const safeStages = Array.isArray(stages) && stages.length ? stages : [[92, "整理结果"]];
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
  const timer = window.setInterval(() => {
    const [value, label] = safeStages[Math.min(index, safeStages.length - 1)];
    const percent = Math.min(Math.max(Number(value) || 0, 0), 100);
    const valueNode = node.querySelector("[data-progress-value]");
    const barNode = node.querySelector("[data-progress-bar]");
    const labelNode = node.querySelector("[data-progress-label]");
    if (valueNode) valueNode.textContent = `${Math.round(percent)}%`;
    if (barNode) barNode.style.width = `${percent}%`;
    if (labelNode) labelNode.textContent = label;
    index += 1;
  }, 700);
  panelProgressTimers.set(node, timer);
  return timer;
}

function stopPanelProgress(node, timer) {
  if (!node) {
    if (timer) window.clearInterval(timer);
    return;
  }
  const activeTimer = panelProgressTimers.get(node);
  if (timer && activeTimer && activeTimer !== timer) return;
  const timerToStop = timer || activeTimer;
  if (timerToStop) window.clearInterval(timerToStop);
  if (!timer || activeTimer === timer) panelProgressTimers.delete(node);
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
  updateLlmStatus("已保存", "ok", "设置已保存；运行智能体时会按当前配置尝试调用大模型。");
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

function setLlmStatus(text, state = "neutral", hint = "") {
  if (!llmSettings.status) return;
  llmSettings.status.textContent = text;
  llmSettings.status.dataset.state = state;
  if (llmSettings.hint) {
    llmSettings.hint.textContent = hint;
    llmSettings.hint.dataset.state = state;
  }
  syncAgentModelStatus();
}

function syncAgentModelStatus() {
  if (!agentUi.modelStatus || !llmSettings.status) return;
  agentUi.modelStatus.textContent = llmSettings.status.textContent || "本地规则兜底";
  agentUi.modelStatus.dataset.state = llmSettings.status.dataset.state || "neutral";
}

function updateLlmStatus(customText, customState = "neutral", customHint = "") {
  if (customText) {
    setLlmStatus(customText, customState, customHint);
    return;
  }
  const hasKey = Boolean(llmSettings.apiKey.value.trim());
  const hasBaseUrl = Boolean(llmSettings.baseUrl.value.trim());
  const hasModel = Boolean(llmSettings.model.value.trim());
  if (hasKey && (hasBaseUrl || hasModel)) {
    setLlmStatus("自定义大模型", "ok", "将使用当前页面配置；调用失败会自动回退本地规则。");
  } else if (hasKey) {
    setLlmStatus("已填 API Key", "ok", "将使用默认接口和模型；调用失败会自动回退本地规则。");
  } else if (hasBaseUrl || hasModel) {
    setLlmStatus("配置不完整", "warning", "已填写接口或模型，但缺少 API Key；本地规则会作为兜底。");
  } else {
    setLlmStatus("本地未配置", "missing", "消息 RAG 和智能体共用此配置；模型不可用时会回退本地规则。");
  }
}

function updateLlmStatusFromAgentResult(data) {
  updateLlmStatusFromText(collectAgentResultText(data));
}

function updateLlmStatusFromRagResult(data) {
  updateLlmStatusFromText(collectRagResultText(data));
}

function updateLlmStatusFromText(text) {
  if (!text) return;
  if (text.includes("LLM 调用失败") || text.includes("大模型调用失败") || text.includes("RAG 模型分析失败")) {
    setLlmStatus("大模型调用失败", "warning", "已回退本地规则；请检查代理、API Key 或模型服务。");
  } else if (text.includes("未配置 OPENAI_API_KEY") || text.includes("未配置大模型")) {
    setLlmStatus("未配置大模型", "missing", "本次未调用模型，已使用本地规则解析。");
  } else if (text.includes("已调用模型")) {
    setLlmStatus("模型已参与", "ok", "本次已基于检索证据调用大模型生成判断。");
  }
}

function collectAgentResultText(data) {
  const parts = [data?.reply];
  const nested = data?.data || {};
  if (Array.isArray(nested.notes)) parts.push(...nested.notes);
  if (nested.summary) parts.push(nested.summary);
  return parts.filter(Boolean).join(" ");
}

function collectRagResultText(data) {
  const parts = [];
  if (Array.isArray(data?.notes)) parts.push(...data.notes);
  if (Array.isArray(data?.findings)) {
    for (const finding of data.findings.slice(0, 8)) {
      parts.push(finding?.summary, finding?.rationale, ...(finding?.evidence_titles || []));
    }
  }
  return parts.filter(Boolean).join(" ");
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
    "requireInstitutionBuyRatio",
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
      input.value = sanitizeStockLookupInput(input.value);
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
  const inputs = [...document.querySelectorAll("input[data-code-confirm], input[data-stock-list-suggest]")];
  if (!inputs.length) return;

  inputs.forEach((input) => {
    const field = input.closest(".market-field") || input.parentElement;
    if (!field) return;

    const isListInput = input.hasAttribute("data-stock-list-suggest");
    field.classList.add("stock-suggest-field");
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
      input.value = isListInput ? appendStockCodeToken(input.value, stock.code) : stock.code;
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
      const query = isListInput ? lastStockLookupToken(input.value) : input.value.trim();
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


function lastStockLookupToken(value) {
  const parts = String(value || "").split(/[,，;；\s]+/).map((item) => sanitizeStockLookupInput(item));
  return parts.filter(Boolean).pop() || "";
}

function appendStockCodeToken(value, code) {
  const normalized = normalizeStockCode(code);
  if (!normalized) return String(value || "");
  const parts = String(value || "").split(/[,，;；\s]+/).map((item) => sanitizeStockLookupInput(item)).filter(Boolean);
  if (parts.length) parts.pop();
  const codes = [];
  const seen = new Set();
  for (const part of [...parts, normalized]) {
    const parsed = normalizeStockCode(part) || part;
    if (seen.has(parsed)) continue;
    seen.add(parsed);
    codes.push(parsed);
  }
  return codes.join(", ");
}

function initDateInputs() {
  document.querySelectorAll('input[type="date"]').forEach((input) => {
    const dateValue = toDateInputValue(input.value || defaultDateInputValue(input));
    if (dateValue) input.value = dateValue;
    input.addEventListener("click", () => showDatePicker(input));
  });
  initObserveDateRangeDefaults();
}

function defaultDateInputValue(input) {
  if (input.id === "observeStart") return defaultObserveStartDateInputValue();
  return DEFAULT_TODAY_DATE_INPUT_IDS.has(input.id) ? currentSystemDateInputValue() : "";
}

function initObserveDateRangeDefaults() {
  const startInput = $("#observeStart");
  const endInput = $("#observeEnd");
  if (!startInput) return;

  const setAutoDefault = () => {
    const previousAuto = startInput.dataset.autoDefault || "";
    const nextAuto = defaultObserveStartDateInputValue(endInput?.value);
    if (!startInput.value || startInput.value === previousAuto) {
      startInput.value = nextAuto;
    }
    startInput.dataset.autoDefault = nextAuto;
  };

  setAutoDefault();
  endInput?.addEventListener("change", setAutoDefault);
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
    require_institution_buy_ratio_gt_sell_ratio: Boolean($("#requireInstitutionBuyRatio")?.checked),
    min_deducted_net_profit_billion: 0,
    min_deducted_net_profit_growth_rate: 10,
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
    "扣非净利润 > 0",
    "扣非净利润增长率 > 10%",
    $("#requireInstitutionBuyRatio")?.checked ? "机构净买入" : "",
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

async function resolveStockCodeInput(id, resultNode) {
  const input = $(`#${id}`);
  const raw = input?.value || "";
  const direct = normalizeStockCode(raw);
  if (direct) return direct;

  const query = sanitizeStockLookupInput(raw);
  if (!query) return "";

  const stock = await findStockByLookup(query, resultNode);
  if (!stock?.code) return "";
  const code = normalizeStockCode(stock.code);
  if (code && input) {
    input.value = code;
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }
  return code;
}

async function findStockByLookup(query, resultNode) {
  const params = new URLSearchParams({ q: query, limit: "1" });
  try {
    const items = await requestJson("GET", `/api/stock-search?${params}`, undefined, stockSearchHeaders());
    return Array.isArray(items) ? items[0] : null;
  } catch (err) {
    if (resultNode) setError(resultNode, "股票搜索不可用", err.message);
    return null;
  }
}

async function resolveStockCodeListInput(id, resultNode) {
  const input = $(`#${id}`);
  const raw = input?.value || "";
  const parts = raw
    .split(/[,，;；\s]+/)
    .map((item) => sanitizeStockLookupInput(item))
    .filter(Boolean);

  const codes = [];
  const seen = new Set();
  for (const part of parts) {
    let code = normalizeStockCode(part);
    if (!code) {
      const stock = await findStockByLookup(part, resultNode);
      code = normalizeStockCode(stock?.code || "");
    }
    if (code && !seen.has(code)) {
      seen.add(code);
      codes.push(code);
    }
  }
  if (input && codes.length) input.value = codes.join(", ");
  return codes;
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

function sanitizeStockLookupInput(value) {
  return String(value || "")
    .replace(/[\r\n\t]+/g, " ")
    .replace(/\s{2,}/g, " ")
    .trim();
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

function defaultObserveStartDateInputValue(referenceValue = "") {
  return tradingWindowStartDateInputValue(referenceValue || currentSystemDateInputValue(), DEFAULT_OBSERVE_TRADING_DAYS);
}

function defaultObserveStartCompact(referenceValue = "") {
  return normalizeDateParam(defaultObserveStartDateInputValue(referenceValue), currentSystemDateCompact());
}

function tradingWindowStartDateInputValue(referenceValue, tradingDays) {
  const endDate = parseDateInputValue(referenceValue) || parseDateInputValue(currentSystemDateInputValue());
  const targetCount = Math.max(1, Number(tradingDays) || DEFAULT_OBSERVE_TRADING_DAYS);
  const cursor = new Date(endDate.getFullYear(), endDate.getMonth(), endDate.getDate());
  let counted = 0;

  while (counted < targetCount) {
    if (isWeekday(cursor)) counted += 1;
    if (counted >= targetCount) break;
    cursor.setDate(cursor.getDate() - 1);
  }

  return formatDateInputValue(cursor);
}

function parseDateInputValue(value) {
  const raw = String(value || "").trim();
  let match = raw.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!match) match = raw.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (!match) return null;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  if (date.getFullYear() !== Number(match[1]) || date.getMonth() !== Number(match[2]) - 1 || date.getDate() !== Number(match[3])) {
    return null;
  }
  return date;
}

function isWeekday(date) {
  const day = date.getDay();
  return day >= 1 && day <= 5;
}

function formatDateInputValue(date) {
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join("-");
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

function withTimeout(promise, timeoutMs, message) {
  let timer = null;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = window.setTimeout(() => reject(new Error(message)), timeoutMs);
    }),
  ]).finally(() => {
    if (timer) window.clearTimeout(timer);
  });
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

async function requestJson(method, url, payload, headers = dataSourceHeaders(), options = {}) {
  const timeoutSignal = createTimeoutSignal(options.timeoutMs);
  const signal = combineAbortSignals(options.signal, timeoutSignal.signal);
  try {
    const tauriResult = await withAbortSignal(requestTauriJson(method, url, payload), signal);
    if (tauriResult.handled) return tauriResult.data;

    const request = {
      method,
      headers: method === "POST" ? { "Content-Type": "application/json", ...headers } : headers,
    };
    if (signal) request.signal = signal;
    if (payload !== undefined) request.body = JSON.stringify(payload);

    const resp = await fetch(url, request);
    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(text || `HTTP ${resp.status}`);
    }
    return await resp.json();
  } finally {
    timeoutSignal.cancel();
  }
}

function withAbortSignal(promise, signal) {
  if (!signal) return promise;
  if (signal.aborted) return Promise.reject(abortReason(signal));
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      signal.addEventListener("abort", () => reject(abortReason(signal)), { once: true });
    }),
  ]);
}

function createTimeoutSignal(timeoutMs) {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || typeof AbortController === "undefined") {
    return { signal: null, cancel: () => {} };
  }
  const controller = new AbortController();
  const timer = window.setTimeout(() => {
    controller.abort(createRequestAbortError(`请求超过 ${Math.round(timeoutMs / 1000)} 秒未返回`, "TimeoutError"));
  }, timeoutMs);
  return {
    signal: controller.signal,
    cancel: () => window.clearTimeout(timer),
  };
}

function combineAbortSignals(...signals) {
  const activeSignals = signals.filter(Boolean);
  if (!activeSignals.length || typeof AbortController === "undefined") return activeSignals[0] || null;
  if (activeSignals.length === 1) return activeSignals[0];
  const controller = new AbortController();
  const abort = (event) => {
    if (!controller.signal.aborted) controller.abort(abortReason(event.target));
  };
  activeSignals.forEach((signal) => {
    if (signal.aborted) abort({ target: signal });
    else signal.addEventListener("abort", abort, { once: true });
  });
  return controller.signal;
}

function abortReason(signal) {
  return signal?.reason || createRequestAbortError("请求已取消", "AbortError");
}

function createRequestAbortError(message, name = "AbortError") {
  try {
    return new DOMException(message, name);
  } catch (_error) {
    const error = new Error(message);
    error.name = name;
    return error;
  }
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

async function openExternalUrl(rawUrl) {
  const url = normalizeExternalUrl(rawUrl);
  if (!url) return;
  const invoke = window.__TAURI__?.core?.invoke;
  if (invoke) {
    try {
      await invoke("open_external_url", { url });
      return;
    } catch (error) {
      console.warn("open_external_url failed, falling back to window.open", error);
    }
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

function normalizeExternalUrl(rawUrl) {
  try {
    const url = new URL(String(rawUrl || "").trim(), window.location.href);
    return ["http:", "https:"].includes(url.protocol) ? url.toString() : "";
  } catch (_error) {
    return "";
  }
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

function conceptGroupForStock(stock = {}, rules = screeningRules) {
  const text = `${stock.name || ""} ${stock.industry || ""}`.trim().toLowerCase();
  for (const group of screeningConceptGroups(rules)) {
    if ((group.keywords || []).some((keyword) => text.includes(String(keyword).toLowerCase()))) {
      return group.label;
    }
  }
  return "其他概念";
}

function conceptRank(concept, rules = screeningRules) {
  const groups = screeningConceptGroups(rules);
  const index = groups.findIndex((group) => group.label === concept);
  return index >= 0 ? index : groups.length;
}

async function buildTauriSectorScreen(invoke, payload = {}) {
  const data = await loadMobileMarketData(invoke);
  const rules = await getScreeningRules();
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
    const sector = conceptGroupForStock(item.stock, rules);
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
    .sort((left, right) => conceptRank(left.sector, rules) - conceptRank(right.sector, rules) || right.average_score - left.average_score || right.total - left.total || left.sector.localeCompare(right.sector))
    .slice(0, maxSectors);
  return {
    total: screen.total || 0,
    returned: groups.reduce((sum, group) => sum + group.returned, 0),
    sector_count: groups.length,
    groups,
    notes: [
      ...(screen.notes || []),
      "移动端使用内置通达信数据集进行本地概念分组，未命中概念时归入其他概念。",
    ],
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

  return {
    source: "tdx",
    stock,
    financial_indicators: buildMobileFinancialIndicators(stock),
    trend,
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
  add("扣非净利润", stock.deducted_net_profit_billion, (value) => `${formatNumber(value)}亿`, Number(stock.deducted_net_profit_billion || 0) > 0 ? "rise" : "fall");
  add("扣非净利率", stock.deducted_net_profit_margin, (value) => `${formatNumber(Math.abs(value) <= 1 ? value * 100 : value)}%`);
  add("扣非净利润增长率", stock.deducted_net_profit_growth_rate, (value) => `${formatNumber(Math.abs(value) <= 1 ? value * 100 : value)}%`, Number(stock.deducted_net_profit_growth_rate || 0) >= 0 ? "rise" : "fall");
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
  let seed = Object.prototype.hasOwnProperty.call(options, "种子股") ? options.seed : null;
  if (!Object.prototype.hasOwnProperty.call(options, "种子股")) {
    try {
      seed = await loadCachedMobileMarketData(invoke);
    } catch {
      seed = null;
    }
  }
  mobileMarketDataPromise = null;
  mobileMarketDataSummary = null;
  mobileMarketDataMeta = null;
  const result = await withTimeout(
    invoke("core_mobile_market_data_refresh_tencent", {
      payload: {
        seed,
        scan_candidates: true,
        max_candidates: MOBILE_TENCENT_MAX_CANDIDATES,
        max_failed_batches: MOBILE_TENCENT_MAX_FAILED_BATCHES,
        max_refresh_secs: MOBILE_TENCENT_MAX_REFRESH_SECS,
        use_previous_close: shouldUsePreviousCloseForMobileRefresh(),
      },
    }),
    MOBILE_TENCENT_INVOKE_TIMEOUT_MS,
    `腾讯行情刷新超过 ${Math.round(MOBILE_TENCENT_INVOKE_TIMEOUT_MS / 1000)} 秒未返回，已停止等待。请确认手机网络可访问腾讯行情后重试。`,
  );
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
  const groups = (data.groups || []).filter((group) => (group.items || []).length);
  const hotGroup = groups.find((group) => group.key === "hot");
  const potentialGroup = groups.find((group) => group.key === "potential");
  const hasGroups = groups.length > 0;
  const groupReturned = groups.reduce((sum, group) => sum + (group.returned ?? group.items?.length ?? 0), 0);
  const actionCount = hasGroups ? groupReturned : (data.returned ?? items.length);
  renderResult(node, {
    summary: hasGroups
      ? [
          ["热门股", hotGroup?.returned ?? hotGroup?.items?.length ?? 0],
          ["潜力股", potentialGroup?.returned ?? potentialGroup?.items?.length ?? 0],
          ["候选", data.total ?? 0],
        ]
      : [
          ["命中", data.returned ?? items.length],
          ["候选", data.total ?? 0],
          ["最高分", items[0] ? formatNumber(items[0].score) : "-"],
        ],
    body: [
      hasGroups || items.length ? renderResultActions("当前筛选条件", actionCount) : "",
      hasGroups
        ? renderScreenGroups(groups)
        : items.length
        ? renderStockList(items.map(screenItemToView))
        : renderEmpty("没有符合条件的股票", { label: "重跑筛选", action: "run-screen" }),
      data.notes?.length ? renderNotes(data.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderScreenGroups(groups) {
  return `
    <div class="screen-result-groups">
      ${groups
        .map((group) => {
          const items = group.items || [];
          return `
            <section class="screen-result-group" data-group-key="${escapeHtml(group.key || "")}">
              <header>
                <div>
                  <h3>${escapeHtml(group.title || "筛选结果")}</h3>
                  ${group.description ? `<p>${escapeHtml(group.description)}</p>` : ""}
                </div>
                <strong>${formatNumber(group.returned ?? items.length)} / 10</strong>
              </header>
              ${items.length ? renderStockList(items.map(screenItemToView)) : renderEmpty("这一类暂时没有命中股票")}
            </section>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderSectorScreenResult(node, data) {
  const groups = data.groups || [];
  renderResult(node, {
    summary: [
      ["概念", data.sector_count ?? groups.length],
      ["展示", data.returned ?? 0],
      ["候选", data.total ?? 0],
    ],
    body: [
      groups.length ? renderResultActions("当前分概念条件", data.returned ?? 0) : "",
      groups.length
        ? renderSectorGroups(groups)
        : renderEmpty("没有符合条件的概念", { label: "调整条件", action: "go-screen" }),
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
                  <h3>${escapeHtml(group.sector || "未知概念")}</h3>
                  <p>候选 ${formatNumber(group.total)} 只 · 展示 ${formatNumber(group.returned)} 只</p>
                </div>
                <strong>均分 ${formatNumber(group.average_score)}</strong>
              </header>
              ${items.length ? renderStockList(items.map(screenItemToView)) : renderEmpty("该概念没有入选股票")}
            </section>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderGraphResult(node, data) {
  const items = data.items || [];
  const center = data.center_context || {};
  renderResult(node, {
    summary: [
      ["返回", data.returned ?? items.length],
      ["关系边", data.relation_count ?? 0],
      ["中心", center.mode === "theme_center" ? center.label || "主题" : "种子股"],
      ["最高分", items[0] ? formatNumber(items[0].final_score) : "-"],
    ],
    body: [
      items.length
        ? renderStockList(items.map(graphItemToView))
        : renderEmpty("当前条件下没有匹配的关系信号"),
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

function trendScreenStyleLabel(value) {
  const labels = {
    short_buy: "短线买点",
  };
  return labels[value] || value || "短线买点";
}
function renderTrendScreenResult(node, data) {
  const items = data.items || [];
  renderResult(node, {
    summary: [
      ["返回", data.returned ?? items.length],
      ["候选", data.total ?? 0],
      ["口径", trendScreenStyleLabel(data.screen_style)],
      ["最高分", items[0] ? formatNumber(items[0].final_score) : "-"],
    ],
    body: [
      items.length
        ? renderStockList(items.map(trendItemToView))
        : renderEmpty("当前条件下没有匹配的趋势买点"),
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
  const groups = normalizeNewsSentimentGroups(data.sentiment_groups);
  const hasSentimentGroups = Boolean(data.sentiment_groups);
  renderResult(node, {
    summary: [
      ["范围", (data.scope_codes || []).length],
      ["关系边", data.relation_count ?? 0],
      ["消息", data.message_count ?? 0],
      ["模式", hasSentimentGroups ? newsAnalysisModeLabel(groups.mode) : "影响判断"],
    ],
    body: renderNewsRagBody(data, groups),
    raw: data,
  });
}

function renderNewsRagBody(data, groups = normalizeNewsSentimentGroups(data.sentiment_groups)) {
  const findings = data.findings || [];
  const hasPlainNewsGroups = Boolean(data.sentiment_groups) && groups.mode === "plain_news";
  const body = hasPlainNewsGroups
    ? renderPlainNewsGroups(groups)
    : (findings.length ? renderNewsFindings(findings) : renderEmpty("没有命中的上下游消息"));
  return [
    body,
    data.notes?.length ? renderNotes(data.notes) : "",
  ].join("");
}

function normalizeNewsSentimentGroups(groups) {
  const normalized = groups || {};
  return {
    mode: normalized.mode === "llm_analysis" ? "llm_analysis" : "plain_news",
    positive: Array.isArray(normalized.positive) ? normalized.positive : [],
    negative: Array.isArray(normalized.negative) ? normalized.negative : [],
    mixed: Array.isArray(normalized.mixed) ? normalized.mixed : [],
    uncertain: Array.isArray(normalized.uncertain) ? normalized.uncertain : [],
  };
}

function newsAnalysisModeLabel(mode) {
  return mode === "llm_analysis" ? "模型分析" : "本地消息";
}

function renderPlainNewsGroups(groups) {
  const secondary = [...groups.mixed, ...groups.uncertain];
  return `
    <div class="plain-news-groups">
      <section class="plain-news-column positive">
        <header><h3>利好消息</h3><span>${formatNumber(groups.positive.length)}</span></header>
        ${groups.positive.length ? renderEvidenceList(groups.positive, { emptyText: "暂无利好消息", showSentiment: true }) : renderEmpty("暂无利好消息")}
      </section>
      <section class="plain-news-column negative">
        <header><h3>利空消息</h3><span>${formatNumber(groups.negative.length)}</span></header>
        ${groups.negative.length ? renderEvidenceList(groups.negative, { emptyText: "暂无利空消息", showSentiment: true }) : renderEmpty("暂无利空消息")}
      </section>
      ${secondary.length ? `
        <details class="plain-news-secondary">
          <summary><span>中性 / 待核验消息 ${formatNumber(secondary.length)}</span><b class="plain-news-toggle" aria-hidden="true"></b></summary>
          ${renderEvidenceList(secondary, { emptyText: "暂无待核验消息", showSentiment: true })}
        </details>
      ` : ""}
    </div>
  `;
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
                ${renderExternalSourceLink(item.source_url)}
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

function renderEvidenceList(items, options = {}) {
  const emptyText = options.emptyText || "没有可引用证据";
  const showSentiment = Boolean(options.showSentiment);
  if (!items.length) return renderEmpty(emptyText);
  return `
    <div class="evidence-list">
      ${items
        .map(
          (item) => `
            <article>
              <strong>${escapeHtml(item.title || "-")}</strong>
              <span class="evidence-source">
                <span class="source-tier ${sourceTierClass(item.source_tier)}">${escapeHtml(sourceTierLabel(item.source_tier))}</span>
                ${showSentiment ? `<span class="impact-pill ${sentimentClass(item.sentiment)}">${escapeHtml(newsSentimentLabel(item.sentiment))}</span>` : ""}
                <span>来源：${escapeHtml(item.source || "未知来源")}</span>
                <span>发布时间：${escapeHtml(item.published_at || "未知时间")}</span>
              </span>
              ${item.summary ? `<p>${escapeHtml(item.summary)}</p>` : ""}
              <em>关联股票：${escapeHtml((item.stock_codes || []).length ? item.stock_codes.join(" · ") : "未标注")}</em>
              ${renderExternalSourceLink(item.url)}
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

function renderExternalSourceLink(url) {
  const normalized = normalizeExternalUrl(url);
  if (!normalized) return "";
  return `<a class="evidence-link" href="${escapeHtml(normalized)}" data-external-url="${escapeHtml(normalized)}" target="_blank" rel="noreferrer">查看来源</a>`;
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
  if (tier === "manual_url") return "公开资料";
  if (tier === "community") return "社区 / 待核验";
  if (tier === "news") return "新闻 / 事实";
  return "未知来源";
}

function sourceTierClass(tier) {
  if (tier === "filing") return "filing";
  if (tier === "financial_snapshot") return "filing";
  if (tier === "manual_url" || tier === "research") return "news";
  return tier === "community" ? "community" : "news";
}

function newsSentimentLabel(sentiment) {
  const labels = {
    positive: "利好",
    negative: "利空",
    mixed: "中性",
    uncertain: "待核验",
  };
  return labels[sentiment] || "待核验";
}

function sentimentClass(sentiment) {
  if (sentiment === "positive") return "positive";
  if (sentiment === "negative") return "negative";
  if (sentiment === "mixed") return "neutral";
  return "uncertain";
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
  const signal = data.trend?.signal || {};
  const kdjValues = [signal.k, signal.d, signal.j].map(formatNumber).join(" / ");
  return [
    ["数据源", sourceLabel(data.source)],
    ["最新价", formatNumber(stock.price)],
    ["KDJ", kdjValues],
  ];
}

function renderObserveBody(data) {
  const stock = data.stock || {};
  const trend = data.trend || {};
  const signal = trend.signal || {};
  const series = trend.series || [];
  return [
    renderObservationOverview(stock, data.financial_indicators),
    renderQuarterlyEpsPanel(stock, data.financial_indicators),
    trend.signal ? renderSignalCard(stock, signal, { series, chipDistribution: trend.chip_distribution }) : renderEmpty("没有可用日线技术面"),
    data.capital_evidence ? renderCapitalEvidence(data.capital_evidence, { series, signal }) : "",
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
    bodyParts.push(nestedGroups.length ? renderSectorGroups(nestedGroups) : renderEmpty("没有分概念选股结果"));
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
      ? ["概念", nested.sector_count ?? nestedGroups.length ?? "-"]
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
    factorScores: item.factor_scores || {},
    scoreExplanation: item.score_explanation || "",
    concept: item.concept || "",
    themeCategory: item.theme_category || "",
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
    explanation: item.explanation || null,
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
      ["短买", item.trend_score],
      ["量化", signal.quant_score],
    ],
    explanation: item.explanation || null,
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
  const factorSummary = renderFactorSummary(item);
  const weight = item.weight !== undefined ? `<span class="weight">${formatPercent(item.weight)}</span>` : "";
  const related = item.related?.length ? renderRelated(item.related) : "";
  const explanation = renderSelectionExplanation(item.explanation);
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
      ${factorSummary}
      ${signal}
      ${reasons}
      ${related}
      ${explanation}
    </article>
  `;
}

function renderSelectionExplanation(explanation) {
  if (!explanation) return "";
  const basis = Array.isArray(explanation.basis) ? explanation.basis.filter(Boolean) : [];
  const risks = Array.isArray(explanation.risk_checks) ? explanation.risk_checks.filter(Boolean) : [];
  const verification = Array.isArray(explanation.verification) ? explanation.verification.filter(Boolean) : [];
  const breakdown = Array.isArray(explanation.score_breakdown) ? explanation.score_breakdown : [];
  if (!basis.length && !risks.length && !verification.length && !breakdown.length) return "";
  const section = (title, items) => items.length ? `<div><strong>${escapeHtml(title)}</strong>${items.map((item) => `<p>${escapeHtml(item)}</p>`).join("")}</div>` : "";
  return `
    <section class="selection-explain">
      ${section("入选依据", basis)}
      ${breakdown.length ? `<div><strong>分数拆解</strong><div class="explain-score-row">${breakdown.map(renderScoreContribution).join("")}</div></div>` : ""}
      ${section("风险验证", risks)}
      ${section("验证点", verification)}
    </section>
  `;
}

function renderScoreContribution(item) {
  const tone = ["strong", "watch", "weak"].includes(item?.tone) ? item.tone : "neutral";
  return `
    <span class="score-contribution ${tone}">
      <em>${escapeHtml(item?.label || item?.key || "分数拆解")}</em>
      <b>${formatNumber(item?.value)}</b>
      <small>${formatNumber(item?.contribution)}</small>
    </span>
  `;
}
function renderFactorSummary(item) {
  const scores = item.factorScores || item.factor_scores || {};
  const order = ["theme", "fundamental", "valuation", "size", "risk", "institution"];
  const labels = {
    theme: "主题",
    fundamental: "基本面",
    valuation: "估值",
    size: "规模",
    risk: "风险",
    institution: "机构",
  };
  const pills = order
    .filter((key) => Number.isFinite(Number(scores[key])))
    .map((key) => `<span>${escapeHtml(labels[key] || key)} ${escapeHtml(factorTier(scores[key]))}</span>`)
    .join("");
  const explanation = String(item.scoreExplanation || item.score_explanation || "").trim();
  if (!pills && !explanation && !item.concept) return "";
  return `
    <div class="factor-summary">
      ${explanation ? `<p>${escapeHtml(explanation)}</p>` : ""}
      <div class="tag-row factor-row">
        ${item.concept ? `<span>概念 ${escapeHtml(item.concept)}</span>` : ""}
        ${pills}
      </div>
    </div>
  `;
}

function factorTier(value) {
  const score = Number(value);
  if (!Number.isFinite(score)) return "未知";
  if (score >= 0.75) return "强";
  if (score >= 0.5) return "中";
  return "弱";
}

function renderObservationOverview(stock, financial) {
  const sourceMeta = [financial?.period, financial?.source].filter(Boolean).join(" · ");
  const metrics = [
    ["市盈率(TTM)", formatNumber(stock.pe)],
    ["市净率", formatNumber(stock.pb)],
    ["ROE", formatPercent(stock.roe), Number(stock.roe || 0) >= 0 ? "rise" : "fall"],
    ["市值", formatMarketCapYi(stock.market_cap_billion)],
    ["股息率", formatPercent(stock.dividend_yield)],
    ["ST", stock.is_st ? "是" : "否"],
  ].filter(([, value]) => value !== "-");

  return `
    <section class="observe-overview">
      <header class="observe-overview-header">
        <div>
          <h3>${escapeHtml(stock.name || stock.code || "-")}</h3>
          <p>${escapeHtml([stock.code, stock.industry].filter(Boolean).join(" · ") || "未知行业")}</p>
          ${sourceMeta ? `<small>${escapeHtml(sourceMeta)}</small>` : ""}
        </div>
        <strong class="overview-price">${formatNumber(stock.price)}</strong>
      </header>
      <div class="overview-metric-grid">
        ${metrics
          .map(([label, value, tone]) => {
            const safeTone = ["rise", "fall"].includes(tone) ? tone : "neutral";
            return `
              <div class="overview-metric ${safeTone}">
                <span>${escapeHtml(label)}</span>
                <strong>${escapeHtml(value)}</strong>
              </div>
            `;
          })
          .join("")}
      </div>
    </section>
  `;
}

function formatMarketCapYi(value) {
  const number = Number(value);
  return Number.isFinite(number) ? `${formatNumber(number)} 亿` : "-";
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
    (item) =>
      item &&
      item.metric_key !== "quarterly_eps" &&
      item.value !== undefined &&
      item.value !== null &&
      item.value !== "",
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

function renderQuarterlyEpsPanel(stock, financial) {
  const currentYear = new Date().getFullYear();
  const previousYear = currentYear - 1;
  const points = collectQuarterlyEpsItems(financial);
  const pointByPeriod = new Map(points.map((point) => [point.period, point]));
  const latestEpsItem = findFinancialIndicator(financial, (item) => {
    const label = String(item.label || "").toUpperCase();
    return item.metric_key !== "quarterly_eps" && (label.includes("每股收益") || label.includes("EPS"));
  });
  const latestBpsItem = findFinancialIndicator(financial, (item) => {
    const label = String(item.label || "").toUpperCase();
    return label.includes("每股净资产") || label.includes("BPS");
  });
  const latestPoint = points[0];
  const totalShares = estimateTotalSharesYi(stock);
  const sourceMeta = [financial?.period, financial?.source].filter(Boolean).join(" · ");
  const notes = [];
  if (!points.length) notes.push("当前数据源没有返回季度 EPS 明细，先展示最新 EPS 与估算总股本。");
  if (totalShares === null) notes.push("总股本需要总市值和最新价，当前数据不足。");

  return `
    <section class="eps-share-panel">
      <header>
        <div>
          <h3>盈利与股本</h3>
          <p>${escapeHtml(sourceMeta || "报告期 EPS 对比今年和上一年；总股本按总市值 / 最新价估算。")}</p>
        </div>
      </header>
      <div class="share-capital-grid">
        <div>
          <span>总股本</span>
          <strong>${totalShares === null ? "-" : `${formatNumber(totalShares)} 亿股`}</strong>
          <small>估算口径：总市值 / 最新价</small>
        </div>
        <div>
          <span>最新每股收益</span>
          <strong>${escapeHtml(latestEpsItem?.value || (latestPoint ? `${formatNumber(latestPoint.value)}元` : "-"))}</strong>
          <small>${escapeHtml(latestEpsItem?.label || latestPoint?.period || "等待财报数据")}</small>
        </div>
        <div>
          <span>每股净资产</span>
          <strong>${escapeHtml(latestBpsItem?.value || "-")}</strong>
          <small>${escapeHtml(latestBpsItem?.label || "等待财报数据")}</small>
        </div>
      </div>
      <div class="eps-table" role="table" aria-label="季度每股收益变化">
        <div class="eps-table-head" role="row">
          <span>报告期</span>
          <span>Q1</span>
          <span>Q2</span>
          <span>Q3</span>
          <span>Q4</span>
        </div>
        ${[currentYear, previousYear].map((year) => renderEpsYearRow(year, pointByPeriod)).join("")}
      </div>
      ${notes.length ? `<div class="eps-panel-notes">${notes.map(escapeHtml).join(" / ")}</div>` : ""}
    </section>
  `;
}

function renderEpsYearRow(year, pointByPeriod) {
  return `
    <div class="eps-table-row" role="row">
      <strong>${year}</strong>
      ${[1, 2, 3, 4].map((quarter) => renderEpsCell(year, quarter, pointByPeriod)).join("")}
    </div>
  `;
}

function renderEpsCell(year, quarter, pointByPeriod) {
  const point = pointByPeriod.get(`${year}Q${quarter}`);
  if (!point) return `<span class="eps-cell empty" role="cell"><em>-</em><small>未披露</small></span>`;
  const previous = pointByPeriod.get(`${year - 1}Q${quarter}`);
  const delta = previous && previous.value !== 0 ? (point.value - previous.value) / Math.abs(previous.value) : null;
  const tone = delta === null ? "neutral" : delta >= 0 ? "rise" : "fall";
  return `
    <span class="eps-cell ${tone}" role="cell">
      <strong>${formatNumber(point.value)}</strong>
      <small>${delta === null ? "无同比" : `同比 ${formatSignedPercent(delta)}`}</small>
    </span>
  `;
}

function collectQuarterlyEpsItems(financial) {
  const items = financial?.items || [];
  const points = [];
  for (const item of items) {
    if (!item) continue;
    const label = String(item.label || "").toUpperCase();
    const isEps = item.metric_key === "quarterly_eps" || label.includes("每股收益") || label.includes("EPS");
    if (!isEps) continue;
    const period = normalizeFinancialPeriodKey(item.period || item.label || "");
    const value = parseLooseNumber(item.raw_value ?? item.value);
    if (!period || value === null) continue;
    points.push({ period, value, tone: item.tone || "neutral" });
  }
  return points.sort((left, right) => right.period.localeCompare(left.period));
}

function normalizeFinancialPeriodKey(value) {
  const raw = String(value || "").trim().toUpperCase();
  let match = raw.match(/(20\d{2})\s*[QＱ]\s*([1-4])/);
  if (match) return `${match[1]}Q${match[2]}`;

  match = raw.match(/(20\d{2})[-/.年](0?[369]|12)(?:[-/.月]\d{1,2})?/);
  if (match) {
    const month = Number(match[2]);
    const quarter = month === 3 ? 1 : month === 6 ? 2 : month === 9 ? 3 : month === 12 ? 4 : null;
    if (quarter) return `${match[1]}Q${quarter}`;
  }

  const year = raw.match(/(20\d{2})/)?.[1];
  if (!year) return null;
  if (raw.includes("一季") || raw.includes("1季") || raw.includes("Q1")) return `${year}Q1`;
  if (raw.includes("中报") || raw.includes("二季") || raw.includes("2季") || raw.includes("Q2")) return `${year}Q2`;
  if (raw.includes("三季") || raw.includes("3季") || raw.includes("Q3")) return `${year}Q3`;
  if (raw.includes("年报") || raw.includes("四季") || raw.includes("4季") || raw.includes("Q4")) return `${year}Q4`;
  return null;
}

function findFinancialIndicator(financial, predicate) {
  return (financial?.items || []).find((item) => item && predicate(item));
}

function estimateTotalSharesYi(stock) {
  const direct = firstFiniteNumber(
    stock?.total_share_capital_billion,
    stock?.total_shares_billion,
    stock?.share_capital_billion,
  );
  if (direct !== null) return direct;
  const marketCapYi = parseLooseNumber(stock?.market_cap_billion);
  const price = parseLooseNumber(stock?.price);
  if (marketCapYi === null || price === null || price <= 0) return null;
  return marketCapYi / price;
}

function firstFiniteNumber(...values) {
  for (const value of values) {
    const number = parseLooseNumber(value);
    if (number !== null) return number;
  }
  return null;
}

function parseLooseNumber(value) {
  if (value === null || value === undefined || value === "") return null;
  const number = Number(value);
  if (Number.isFinite(number)) return number;
  const match = String(value).replaceAll(",", "").match(/-?\d+(?:\.\d+)?/);
  if (!match) return null;
  const parsed = Number(match[0]);
  return Number.isFinite(parsed) ? parsed : null;
}

function formatSignedPercent(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  const sign = number > 0 ? "+" : "";
  return `${sign}${(number * 100).toLocaleString("zh-CN", { maximumFractionDigits: 1 })}%`;
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
      <span>KDJ ${formatNumber(signal.k)} / ${formatNumber(signal.d)} / ${formatNumber(signal.j)}</span>
      <span>支撑 ${formatNumber(signal.support)}</span>
      <span>阻力 ${formatNumber(signal.resistance)}</span>
    </div>
  `;
}

function renderSignalCard(stock, signal, options = {}) {
  const series = options.series || [];
  const chipDistribution = options.chipDistribution || null;
  const charts = series.length
    ? `<div class="signal-chart-stack">${renderKdjChart(series, signal)}${renderTrendChart(series, chipDistribution)}</div>`
    : "";
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
        <div><span>KDJ</span><strong>${formatNumber(signal.k)} / ${formatNumber(signal.d)} / ${formatNumber(signal.j)}</strong></div>
        <div><span>量化分</span><strong>${escapeHtml(String(signal.quant_score ?? 0))}/${escapeHtml(String(signal.quant_score_max ?? 90))}</strong></div>
        <div><span>形态分</span><strong>${escapeHtml(String(signal.pattern_score ?? 0))}/${escapeHtml(String(signal.pattern_score_max ?? 100))}</strong></div>
        <div><span>支撑位</span><strong>${formatNumber(signal.support)}</strong></div>
        <div><span>阻力位</span><strong>${formatNumber(signal.resistance)}</strong></div>
        <div><span>突破位</span><strong>${formatNumber(signal.breakout)}</strong></div>
        <div><span>反转位</span><strong>${formatNumber(signal.reversal)}</strong></div>
        <div><span>等待线</span><strong>${formatNumber(signal.wait_line)}</strong></div>
      </div>
      ${signal.pattern_signals?.length ? `<div class="tag-row">${signal.pattern_signals.map((reason) => `<span>${escapeHtml(reasonLabel(reason))}</span>`).join("")}</div>` : ""}
      ${signal.reasons?.length ? `<div class="tag-row">${signal.reasons.map((reason) => `<span>${escapeHtml(reasonLabel(reason))}</span>`).join("")}</div>` : ""}
      ${charts}
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
  return `<div class="notes">${notes.map((note) => `<p>${escapeHtml(sanitizeRuntimeMessage(note, 220))}</p>`).join("")}</div>`;
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

function renderKdjChart(series, signal = {}) {
  const width = 720;
  const height = 180;
  const points = (series || [])
    .map((point) => ({
      date: point.date,
      k: Number(point.k),
      d: Number(point.d),
      j: Number(point.j),
    }))
    .filter((point) => [point.k, point.d, point.j].some(Number.isFinite));
  if (points.length < 2) return renderEmpty("KDJ 点不足");

  const values = points.flatMap((point) => [point.k, point.d, point.j]).filter(Number.isFinite);
  const min = Math.min(0, ...values);
  const max = Math.max(100, ...values);
  const range = max - min || 1;
  const xFor = (index) => (index / Math.max(points.length - 1, 1)) * width;
  const yFor = (value) => height - ((Number(value) - min) / range) * height;
  const linePoints = (key) =>
    points
      .map((point, index) => {
        const value = Number(point[key]);
        if (!Number.isFinite(value)) return null;
        return `${xFor(index).toFixed(2)},${yFor(value).toFixed(2)}`;
      })
      .filter(Boolean)
      .join(" ");
  const guideLine = (value, className) => {
    const y = yFor(value).toFixed(2);
    return `<line class="${className}" x1="0" y1="${y}" x2="${width}" y2="${y}" />`;
  };
  const latest = points[points.length - 1] || {};

  return `
    <section class="kdj-panel">
      <header>
        <div>
          <h3>KDJ 动量观察</h3>
          <p>${escapeHtml(latest.date || signal.date || "")}</p>
        </div>
        <span class="state-pill">${escapeHtml(kdjStateLabel(signal, latest))}</span>
      </header>
      <div class="chart-wrap kdj-chart">
        <div class="chart-legend">
          <span class="kdj-legend-k">K ${formatNumber(latest.k)}</span>
          <span class="kdj-legend-d">D ${formatNumber(latest.d)}</span>
          <span class="kdj-legend-j">J ${formatNumber(latest.j)}</span>
        </div>
        <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="KDJ 指标曲线">
          ${guideLine(80, "kdj-guide kdj-guide-high")}
          ${guideLine(20, "kdj-guide kdj-guide-low")}
          <polyline class="kdj-k-line" points="${linePoints("k")}" fill="none" stroke-linecap="round" stroke-linejoin="round" />
          <polyline class="kdj-d-line" points="${linePoints("d")}" fill="none" stroke-linecap="round" stroke-linejoin="round" />
          <polyline class="kdj-j-line" points="${linePoints("j")}" fill="none" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        <div class="chart-labels">
          <span>${escapeHtml(points[0]?.date || "")}</span>
          <span>${escapeHtml(latest.date || "")}</span>
        </div>
      </div>
      ${renderKdjAnalysis(points, signal)}
    </section>
  `;
}

function kdjStateLabel(signal = {}, latest = {}) {
  if (signal.kdj_golden_cross) return "KDJ 金叉";
  if (signal.kdj_dead_cross) return "KDJ 死叉";
  if (signal.kdj_oversold) return "低位钝化";
  if (signal.kdj_overbought) return "高位警戒";
  const k = Number(latest.k ?? signal.k);
  const d = Number(latest.d ?? signal.d);
  if (Number.isFinite(k) && Number.isFinite(d)) return k >= d ? "动量偏强" : "动量偏弱";
  return "等待数据";
}

function renderKdjAnalysis(points, signal = {}) {
  const latest = points[points.length - 1] || {};
  const previous = points.length > 1 ? points[points.length - 2] || {} : {};
  const k = Number(latest.k);
  const d = Number(latest.d);
  const j = Number(latest.j);
  const prevK = Number(previous.k);
  const prevD = Number(previous.d);
  const prevJ = Number(previous.j);
  const dateText = latest.date || signal.date ? `${latest.date || signal.date}：` : "";
  const valueParts = [
    Number.isFinite(k) ? `K ${formatNumber(k)}` : "",
    Number.isFinite(d) ? `D ${formatNumber(d)}` : "",
    Number.isFinite(j) ? `J ${formatNumber(j)}` : "",
  ].filter(Boolean);
  const valueText = valueParts.length ? `${valueParts.join("，")}。` : "KDJ 当前数值不完整。";
  const kdRelation = Number.isFinite(k) && Number.isFinite(d)
    ? k >= d
      ? "K 高于 D，短线动量仍占优。"
      : "K 低于 D，短线动量偏弱。"
    : "K/D 关系暂时无法判断。";
  const jRelation = Number.isFinite(j) && Number.isFinite(k) && Number.isFinite(d)
    ? j > Math.max(k, d)
      ? "J 线在 K/D 上方，拐点放大偏积极。"
      : j < Math.min(k, d)
        ? "J 线落在 K/D 下方，拐点放大偏谨慎。"
        : "J 线贴近 K/D 区间，拐点信号不极端。"
    : "";
  const inferredGoldenCross = Number.isFinite(prevK) && Number.isFinite(prevD) && Number.isFinite(k) && Number.isFinite(d) && prevK < prevD && k >= d;
  const inferredDeadCross = Number.isFinite(prevK) && Number.isFinite(prevD) && Number.isFinite(k) && Number.isFinite(d) && prevK >= prevD && k < d;
  const crossText = signal.kdj_golden_cross || inferredGoldenCross
    ? "本次 K 上穿 D，当前标记为金叉。"
    : signal.kdj_dead_cross || inferredDeadCross
      ? "本次 K 跌破 D，当前标记为死叉。"
      : Number.isFinite(k) && Number.isFinite(d)
        ? "本次没有新的 K/D 交叉。"
        : "交叉信号暂不完整。";
  const moveText = [
    kdjMoveText("K", k, prevK),
    kdjMoveText("D", d, prevD),
    kdjMoveText("J", j, prevJ),
  ].filter(Boolean).join("，");
  const momentumText = moveText ? `${moveText}。` : "缺少上一交易日 KDJ，暂不判断变化方向。";
  const zoneText = signal.kdj_oversold || (Number.isFinite(k) && k <= 20) || (Number.isFinite(d) && d <= 20)
    ? "K/D 位于低位区，修复机会存在，但需要价格和金叉确认。"
    : signal.kdj_overbought || (Number.isFinite(k) && k >= 80) || (Number.isFinite(d) && d >= 80) || (Number.isFinite(j) && j >= 100)
      ? "K/D 或 J 进入高位区，趋势可以延续，但回撤风险同步上升。"
      : "K/D 位于 20-80 中性区，当前重点看 K 能否重新压过 D。";
  const conclusionText = signal.kdj_golden_cross || inferredGoldenCross || (Number.isFinite(k) && Number.isFinite(d) && k >= d && Number.isFinite(prevK) && k >= prevK)
    ? "当前 KDJ 偏向动量修复，仍要看收盘价是否配合站回短线参考线。"
    : signal.kdj_dead_cross || inferredDeadCross || (Number.isFinite(k) && Number.isFinite(d) && k < d)
      ? "当前 KDJ 偏向动量降温，未重新上穿 D 前不宜把它当作强势信号。"
      : "当前 KDJ 还在中性拉扯，下一步看 K/D 是否形成同向扩张。";

  return `
    <section class="kdj-analysis" aria-label="KDJ 当前指标解读">
      <strong>当前 KDJ 解读</strong>
      <p>${escapeHtml(`${dateText}${valueText}${kdRelation}${jRelation}`)}</p>
      <p>${escapeHtml(`${crossText}${momentumText}${zoneText}${conclusionText}`)}</p>
    </section>
  `;
}

function kdjMoveText(label, current, previous) {
  if (!Number.isFinite(current) || !Number.isFinite(previous)) return "";
  const diff = current - previous;
  if (Math.abs(diff) < 0.2) return `${label} 较上一交易日基本走平`;
  return `${label} 较上一交易日${diff > 0 ? "上行" : "回落"} ${formatNumber(Math.abs(diff))}`;
}

function renderTrendChart(series, chipDistribution = null) {
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
      if (point.short_buy) return `<circle cx="${cx}" cy="${cy}" r="3.2" fill="var(--positive)" stroke="var(--surface)" stroke-width="0.8" />`;
      if (point.white_exit) return `<circle cx="${cx}" cy="${cy}" r="3.2" fill="var(--danger)" stroke="var(--surface)" stroke-width="0.8" />`;
      return "";
    })
    .join("");
  const latestPoint = (key) => {
    for (let index = series.length - 1; index >= 0; index -= 1) {
      const value = Number(series[index]?.[key]);
      if (Number.isFinite(value)) {
        return {
          x: xFor(index),
          y: yFor(value),
          value,
        };
      }
    }
    return null;
  };
  const latestClose = latestPoint("close");
  const closeLabel = latestClose
    ? (() => {
        const labelWidth = 112;
        const labelHeight = 24;
        const labelX = Math.min(Math.max(latestClose.x + 8, 4), width - labelWidth - 4);
        const labelY = Math.min(Math.max(latestClose.y - labelHeight - 8, 4), height - labelHeight - 4);
        return `
          <circle class="trend-close-point" cx="${latestClose.x.toFixed(2)}" cy="${latestClose.y.toFixed(2)}" r="4.2" />
          <line class="trend-close-label-line" x1="${latestClose.x.toFixed(2)}" y1="${latestClose.y.toFixed(2)}" x2="${labelX.toFixed(2)}" y2="${(labelY + labelHeight / 2).toFixed(2)}" />
          <g class="trend-close-label">
            <rect x="${labelX.toFixed(2)}" y="${labelY.toFixed(2)}" width="${labelWidth}" height="${labelHeight}" rx="6" />
            <text x="${(labelX + 8).toFixed(2)}" y="${(labelY + 16).toFixed(2)}">收盘价 ${escapeHtml(formatNumber(latestClose.value))}</text>
          </g>
        `;
      })()
    : "";

  return `
    <div class="chart-wrap trend-chart">
      <div class="chart-legend">
        <span class="trend-legend-close">收盘价</span>
        <span class="trend-legend-swl">SWL</span>
        <span class="trend-legend-sws">SWS</span>
      </div>
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="趋势指标曲线">
        <polyline class="trend-close-line" points="${linePoints("close")}" fill="none" stroke-linecap="round" stroke-linejoin="round" />
        <polyline class="trend-swl-line" points="${linePoints("swl")}" fill="none" stroke-linecap="round" stroke-linejoin="round" />
        <polyline class="trend-sws-line" points="${linePoints("sws")}" fill="none" stroke-linecap="round" stroke-linejoin="round" />
        ${markers}
        ${closeLabel}
      </svg>
      <div class="chart-labels">
        <span>${escapeHtml(series[0]?.date || "")}</span>
        <span>${escapeHtml(series[series.length - 1]?.date || "")}</span>
      </div>
      ${renderTrendChartExplanation(series, chipDistribution)}
    </div>
  `;
}

function renderTrendChartExplanation(series, chipDistribution = null) {
  const valid = (series || []).filter((point) => Number.isFinite(Number(point.close)));
  const latest = valid[valid.length - 1] || {};
  const previous = valid.length > 1 ? valid[valid.length - 2] || {} : {};
  const close = Number(latest.close);
  const swl = Number(latest.swl);
  const sws = Number(latest.sws);
  const prevClose = Number(previous.close);
  const prevSwl = Number(previous.swl);
  const prevSws = Number(previous.sws);
  const dateText = latest.date ? `${latest.date}：` : "";
  const valueParts = [
    Number.isFinite(close) ? `收盘价 ${formatNumber(close)}` : "",
    Number.isFinite(swl) ? `SWL ${formatNumber(swl)}` : "",
    Number.isFinite(sws) ? `SWS ${formatNumber(sws)}` : "",
  ].filter(Boolean);
  const valueText = valueParts.length ? `${valueParts.join("，")}。` : "当前趋势数值不完整。";
  const closeState = Number.isFinite(close) && Number.isFinite(swl)
    ? close >= swl
      ? `收盘价高于 SWL ${trendGapText(close, swl)}，短线价格站在参考线之上。`
      : `收盘价低于 SWL ${trendGapText(close, swl)}，短线价格仍被参考线压制。`
    : "收盘价与 SWL 数据不完整，暂不判断短线强弱。";
  const trendState = Number.isFinite(swl) && Number.isFinite(sws)
    ? swl >= sws
      ? `SWL 高于 SWS ${trendGapText(swl, sws)}，短线参考线没有拖累中期。`
      : `SWL 低于 SWS ${trendGapText(swl, sws)}，短线参考线弱于中期。`
    : "SWL 与 SWS 数据不完整，暂不判断中期结构。";
  const moveText = [
    trendMoveText("收盘价", close, prevClose),
    trendMoveText("SWL", swl, prevSwl),
    trendMoveText("SWS", sws, prevSws),
  ].filter(Boolean).join("，");
  const summaryState = Number.isFinite(close) && Number.isFinite(swl) && Number.isFinite(sws)
    ? close >= swl && swl >= sws
      ? "当前是价格、短线、中期顺序偏多的结构，重点看收盘价能否继续留在 SWL 上方。"
      : close < swl && swl < sws
        ? "当前是价格低于短线、短线弱于中期的偏弱结构，先等收盘价收复 SWL。"
        : close < swl && swl >= sws
          ? "中期参考线尚未明显走坏，但价格已回到 SWL 下方，当前先按短线走弱处理。"
          : "价格开始修复到 SWL 上方，但 SWL 仍低于 SWS，当前更像弱势修复初段。"
    : "趋势指标不完整，先等待下一组日线数据。";
  const chipText = renderChipDistributionText(chipDistribution);
  return `
    <section class="trend-explain" aria-label="趋势当前指标解读">
      <strong>当前趋势解读</strong>
      <p>${escapeHtml(`${dateText}${valueText}${closeState}${trendState}`)}</p>
      ${moveText ? `<p>${escapeHtml(`${moveText}。`)}</p>` : ""}
      ${chipText ? `<p>${escapeHtml(chipText)}</p>` : ""}
      <p>${escapeHtml(summaryState)}</p>
    </section>
  `;
}

function trendGapText(value, reference) {
  if (!Number.isFinite(value) || !Number.isFinite(reference)) return "";
  const diff = Math.abs(value - reference);
  const pct = reference ? Math.abs((value - reference) / reference) * 100 : null;
  return Number.isFinite(pct) ? `${formatNumber(diff)}（${formatNumber(pct)}%）` : formatNumber(diff);
}

function trendMoveText(label, current, previous) {
  if (!Number.isFinite(current) || !Number.isFinite(previous)) return "";
  const diff = current - previous;
  if (Math.abs(diff) < 0.01) return `${label}较上一交易日基本走平`;
  return `${label}较上一交易日${diff > 0 ? "上行" : "回落"} ${formatNumber(Math.abs(diff))}`;
}

function renderChipDistributionText(chipDistribution) {
  if (!chipDistribution) return "";
  if (chipDistribution.status === "available") {
    const parts = [];
    if (Number.isFinite(Number(chipDistribution.winner_ratio))) {
      parts.push(`获利盘约 ${formatNumber(chipDistribution.winner_ratio)}%`);
    }
    if (Number.isFinite(Number(chipDistribution.avg_cost))) {
      parts.push(`平均成本约 ${formatNumber(chipDistribution.avg_cost)}`);
    }
    if (Number.isFinite(Number(chipDistribution.cost_90_low)) && Number.isFinite(Number(chipDistribution.cost_90_high))) {
      parts.push(`90%成本区间 ${formatNumber(chipDistribution.cost_90_low)} - ${formatNumber(chipDistribution.cost_90_high)}`);
    }
    if (Number.isFinite(Number(chipDistribution.concentration_90))) {
      parts.push(`90%集中度 ${formatNumber(chipDistribution.concentration_90)}%`);
    }
    const suffix = parts.length ? `：${parts.join("，")}。` : "。";
    return `筹码分布估算已生成${suffix}`;
  }
  const reason = chipDistribution.note || "筹码分布暂不可用";
  return `筹码分布估算暂不可用：${reason}。当前仍按量价推断。`;
}

function renderCapitalBehaviorPanel(series, signal = {}) {
  const points = (series || []).filter((point) =>
    ["accumulation_index", "accumulation_strength", "swing_opportunity", "rebound_signal"].some((key) =>
      Number.isFinite(Number(point[key])),
    ),
  );
  if (points.length < 2) return renderEmpty("吸筹分析点不足");

  const latest = points[points.length - 1] || {};
  const metrics = [
    ["吸筹指标", latest.accumulation_index, "index"],
    ["吸筹强度", latest.accumulation_strength, "strength"],
    ["波段机会", latest.swing_opportunity, "swing"],
    ["绝地反击", latest.rebound_signal, "rebound"],
  ];

  return `
    <section class="capital-behavior">
      <header>
        <div>
          <h3>资金行为分析</h3>
          <p>${escapeHtml(latest.date || signal.date || "")}</p>
        </div>
        <span class="state-pill">形态 ${escapeHtml(String(signal.pattern_score ?? "-"))}</span>
      </header>
      ${renderCapitalMetricExplainPanel(metrics)}
      ${renderAccumulationChart(points)}
      ${renderMacdChart(points)}
      ${renderDragonGrid(points)}
    </section>
  `;
}

function collectCapitalBehaviorMetrics(series) {
  const points = (series || []).filter((point) =>
    ["accumulation_index", "accumulation_strength", "swing_opportunity", "rebound_signal"].some((key) =>
      Number.isFinite(Number(point[key])),
    ),
  );
  const latest = points[points.length - 1] || {};
  const metrics = [
    ["\u5438\u7b79\u6307\u6807", latest.accumulation_index, "index"],
    ["\u5438\u7b79\u5f3a\u5ea6", latest.accumulation_strength, "strength"],
    ["\u6ce2\u6bb5\u673a\u4f1a", latest.swing_opportunity, "swing"],
    ["\u7edd\u5730\u53cd\u51fb", latest.rebound_signal, "rebound"],
  ];
  return { points, latest, metrics };
}

function renderCapitalMetricExplainPanel(metrics, options = {}) {
  if (!metrics.length) return "";
  const focusIndex = Number.isInteger(options.focusIndex) ? options.focusIndex : capitalMetricFocusIndex(metrics);
  const title = options.title || "指标信息";
  const hint = options.hint || "展示当前分数和一句话判断，分数只用于观察优先级。";
  return `
    <section class="capital-metric-panel" aria-label="资金行为指标信息">
      <header>
        <strong>${escapeHtml(title)}</strong>
        <span>${escapeHtml(hint)}</span>
      </header>
      <div class="capital-metric-list">
        ${metrics.map(([label, value, tone], index) => renderCapitalMetricExplainItem(label, value, tone, index === focusIndex)).join("")}
      </div>
    </section>
  `;
}

function renderCapitalMetricExplainItem(label, value, tone, focused = false) {
  const status = capitalMetricStatus(tone, value);
  const numeric = Number(value);
  const signed = tone === "index" && Number.isFinite(numeric) && numeric > 0 ? "+" : "";
  const displayValue = `${signed}${formatNumber(value)}`;
  const sentence = capitalMetricSentence(label, displayValue, tone, status, numeric);
  return `
    <article class="capital-metric-explain ${escapeHtml(tone)} ${focused ? "focused" : ""}">
      <div class="capital-metric-row">
        <span class="capital-metric-title">${escapeHtml(label)}</span>
        <strong class="capital-metric-score">${escapeHtml(displayValue)}</strong>
        <span class="guide-status ${escapeHtml(status.tone)}">${escapeHtml(status.label)}</span>
      </div>
      <p class="capital-metric-one-line">${escapeHtml(sentence)}</p>
    </article>
  `;
}

function renderCapitalMetricGrid(metrics) {
  return renderCapitalMetricExplainPanel(metrics);
}

function renderCapitalBehaviorGuide(metrics) {
  return renderCapitalMetricExplainPanel(metrics);
}

function capitalMetricFocusIndex(metrics) {
  let bestIndex = 0;
  let bestScore = -Infinity;
  metrics.forEach(([, value, tone], index) => {
    const status = capitalMetricStatus(tone, value);
    const score = capitalMetricFocusScore(tone, value, status);
    if (score > bestScore) {
      bestScore = score;
      bestIndex = index;
    }
  });
  return bestIndex;
}

function capitalMetricFocusScore(tone, value, status) {
  const statusRank = { strong: 300, watch: 200, weak: 100, neutral: 0 };
  const numeric = Number(value);
  const magnitude = Number.isFinite(numeric) ? Math.min(Math.abs(numeric), 100) : 0;
  const valueBoost = tone === "index" ? magnitude : magnitude / 2;
  return (statusRank[status.tone] ?? 0) + valueBoost;
}

function capitalMetricSentence(label, displayValue, tone, status, numeric) {
  if (!Number.isFinite(numeric)) return `${label}暂无有效分数，暂不参与当前判断。`;
  const templates = {
    index: {
      strong: `${label} ${displayValue}，资金低位承接明显，可继续看价格是否跟随走强。`,
      watch: `${label} ${displayValue}，资金有轻度承接，但还没形成强确认。`,
      weak: `${label} ${displayValue}，承接偏弱，当前更像流出或弱势整理。`,
    },
    strength: {
      strong: `${label} ${displayValue}，吸筹动作较强，但高位要防短线兑现。`,
      watch: `${label} ${displayValue}，吸筹力度中等，需要价格和成交量继续确认。`,
      weak: `${label} ${displayValue}，吸筹力度有限，暂时只适合观察。`,
    },
    swing: {
      strong: `${label} ${displayValue}，波段空间较好，适合加入重点观察。`,
      watch: `${label} ${displayValue}，有一定波段弹性，等待更明确触发信号。`,
      weak: `${label} ${displayValue}，波段机会偏弱，短线弹性不足。`,
    },
    rebound: {
      strong: `${label} ${displayValue}，超跌反弹概率较高，但仍要看持续性。`,
      watch: `${label} ${displayValue}，有反抽可能，但不能当作趋势反转。`,
      weak: `${label} ${displayValue}，反弹信号不明显，暂不构成主要依据。`,
    },
  };
  return templates[tone]?.[status.tone] || `${label} ${displayValue}，${status.detail}`;
}

function capitalMetricStatus(tone, value) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return { tone: "neutral", label: "无数据", detail: "当前没有足够数据判断。" };
  if (tone === "index") {
    if (numeric >= 8) return { tone: "strong", label: "承接明显", detail: "资金承接信号较强，可结合价格位置继续确认。" };
    if (numeric > 0) return { tone: "watch", label: "轻度承接", detail: "资金承接为正，但力度还不算强。" };
    return { tone: "weak", label: "承接偏弱", detail: "资金承接不足，先降低预期。" };
  }
  if (tone === "strength") {
    if (numeric >= 60) return { tone: "strong", label: "吸筹强", detail: "吸筹动作明显，关注是否放量突破或高位兑现。" };
    if (numeric >= 30) return { tone: "watch", label: "吸筹中等", detail: "有吸筹迹象，但还需要价格和成交量确认。" };
    return { tone: "weak", label: "吸筹偏弱", detail: "目前吸筹力度有限，更适合观察而不是急着判断。" };
  }
  if (tone === "swing") {
    if (numeric >= 60) return { tone: "strong", label: "机会强", detail: "波段机会较强，但仍要配合趋势线和风险位。" };
    if (numeric >= 35) return { tone: "watch", label: "可观察", detail: "有一定波段空间，等待更明确触发信号。" };
    return { tone: "weak", label: "机会弱", detail: "波段弹性不足，暂时不是主线机会。" };
  }
  if (tone === "rebound") {
    if (numeric >= 60) return { tone: "strong", label: "反弹强", detail: "超跌反弹信号较强，重点看持续性。" };
    if (numeric >= 35) return { tone: "watch", label: "反弹中等", detail: "存在反抽可能，但还不能当作趋势反转。" };
    return { tone: "weak", label: "反弹弱", detail: "超跌反弹信号不明显。" };
  }
  return { tone: "neutral", label: "辅助", detail: "用于辅助判断，不单独构成买卖依据。" };
}

function renderAccumulationChart(points) {
  const width = 720;
  const height = 54;
  const tracks = [
    { key: "accumulation_index", label: "吸筹指标", tone: "index", stroke: "var(--positive)" },
    { key: "accumulation_strength", label: "吸筹强度", tone: "strength", stroke: "var(--danger)" },
    { key: "swing_opportunity", label: "波段机会", tone: "swing", stroke: "var(--accent-strong)" },
    { key: "rebound_signal", label: "绝地反击", tone: "rebound", stroke: "var(--muted)", dash: "6 7" },
  ];
  const renderedTracks = tracks
    .map((track) => renderAccumulationTrack(points, track, width, height))
    .filter(Boolean)
    .join("");
  if (!renderedTracks) return "";

  return `
    <div class="chart-wrap accumulation-chart split">
      <div class="capital-track-legend" aria-hidden="true">
        ${tracks
          .map(
            (track) => `
              <span>
                <i class="${escapeHtml(track.tone)}"></i>
                ${escapeHtml(track.label)}
              </span>
            `,
          )
          .join("")}
      </div>
      <div class="capital-track-chart">
        ${renderedTracks}
      </div>
      <div class="chart-labels">
        <span>${escapeHtml(points[0]?.date || "")}</span>
        <span>${escapeHtml(points[points.length - 1]?.date || "")}</span>
      </div>
    </div>
  `;
}

function renderMacdChart(points) {
  const macd = calculateMacd(points);
  const visible = macd.slice(-120);
  if (visible.length < 8) return "";

  const width = 720;
  const height = 150;
  const values = visible.flatMap((point) => [point.dif, point.dea, point.macd]).filter(Number.isFinite);
  if (values.length < 3) return "";

  const minValue = Math.min(0, ...values);
  const maxValue = Math.max(0, ...values);
  const padding = Math.max((maxValue - minValue) * 0.16, 0.08);
  const min = minValue - padding;
  const max = maxValue + padding;
  const range = max - min || 1;
  const xFor = (index) => (index / Math.max(visible.length - 1, 1)) * width;
  const yFor = (value) => height - ((Number(value) - min) / range) * height;
  const baseline = yFor(0);
  const barWidth = Math.max(2, (width / visible.length) * 0.62);
  const linePoints = (key) =>
    visible
      .map((point, index) => `${xFor(index).toFixed(2)},${yFor(point[key]).toFixed(2)}`)
      .join(" ");
  const bars = visible
    .map((point, index) => {
      const x = xFor(index) - barWidth / 2;
      const y = yFor(Math.max(point.macd, 0));
      const h = Math.abs(yFor(point.macd) - baseline);
      const tone = point.macd >= 0 ? "positive" : "negative";
      return `<rect class="macd-bar ${tone}" x="${x.toFixed(2)}" y="${Math.min(y, baseline).toFixed(2)}" width="${barWidth.toFixed(2)}" height="${Math.max(h, 1).toFixed(2)}" rx="1" />`;
    })
    .join("");
  const latest = visible[visible.length - 1] || {};
  const previous = visible[visible.length - 2] || {};
  const status = macdStatus(latest, previous);
  const sampleHint =
    visible.length < 35
      ? "\u5f53\u524d\u6837\u672c\u5c11\uff0c\u53ea\u770b\u65b9\u5411\uff1b"
      : "";

  return `
    <div class="chart-wrap macd-chart">
      <header>
        <div>
          <strong>MACD 动能</strong>
          <span>${sampleHint}DIF 上穿 DEA 偏强，下穿偏弱；柱体变长代表动能增强。</span>
        </div>
        <em class="${escapeHtml(status.tone)}">${escapeHtml(status.label)}</em>
      </header>
      <div class="macd-metrics">
        <span>DIF ${formatNumber(latest.dif)}</span>
        <span>DEA ${formatNumber(latest.dea)}</span>
        <span>MACD ${formatNumber(latest.macd)}</span>
      </div>
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="MACD 12 26 9 指标">
        <line x1="0" y1="${baseline.toFixed(2)}" x2="${width}" y2="${baseline.toFixed(2)}" stroke="var(--line)" stroke-width="1" stroke-dasharray="5 7" />
        <g class="macd-bars">${bars}</g>
        <polyline points="${linePoints("dif")}" fill="none" stroke="var(--accent-strong)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
        <polyline points="${linePoints("dea")}" fill="none" stroke="var(--warning)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      <div class="chart-labels">
        <span>${escapeHtml(visible[0]?.date || "")}</span>
        <span>${escapeHtml(latest.date || "")}</span>
      </div>
    </div>
  `;
}

function calculateMacd(points) {
  const rows = (points || [])
    .map((point) => ({ date: point.date, close: Number(point.close) }))
    .filter((point) => point.date && Number.isFinite(point.close));
  if (rows.length < 8) return [];

  const closes = rows.map((point) => point.close);
  const ema12 = emaSeries(closes, 12);
  const ema26 = emaSeries(closes, 26);
  const dif = ema12.map((value, index) => value - ema26[index]);
  const dea = emaSeries(dif, 9);
  return rows.map((row, index) => ({
    date: row.date,
    dif: dif[index],
    dea: dea[index],
    macd: 2 * (dif[index] - dea[index]),
  }));
}

function emaSeries(values, period) {
  const k = 2 / (period + 1);
  let previous = Number(values[0]) || 0;
  return values.map((value, index) => {
    const numeric = Number(value);
    if (index === 0) {
      previous = Number.isFinite(numeric) ? numeric : previous;
      return previous;
    }
    previous = (Number.isFinite(numeric) ? numeric : previous) * k + previous * (1 - k);
    return previous;
  });
}

function macdStatus(latest, previous) {
  if (![latest?.dif, latest?.dea, latest?.macd].every(Number.isFinite)) {
    return { tone: "neutral", label: "动能不足" };
  }
  if (previous && previous.dif <= previous.dea && latest.dif > latest.dea) {
    return { tone: "strong", label: "金叉转强" };
  }
  if (previous && previous.dif >= previous.dea && latest.dif < latest.dea) {
    return { tone: "weak", label: "死叉转弱" };
  }
  if (latest.dif > latest.dea && latest.macd > 0) return { tone: "strong", label: "多头占优" };
  if (latest.dif < latest.dea && latest.macd < 0) return { tone: "weak", label: "空头占优" };
  return { tone: "watch", label: "震荡观察" };
}

function renderAccumulationTrack(points, track, width, height) {
  const values = points.map((point) => Number(point[track.key])).filter(Number.isFinite);
  if (values.length < 2) return "";
  const minValue = Math.min(0, ...values);
  const maxValue = Math.max(0, ...values);
  const padding = Math.max((maxValue - minValue) * 0.16, 4);
  const min = minValue - padding;
  const max = maxValue + padding;
  const range = max - min || 1;
  const yFor = (value) => height - ((Number(value) - min) / range) * height;
  const xFor = (index) => (index / Math.max(points.length - 1, 1)) * width;
  const baseline = yFor(0);
  const linePoints = points
    .map((point, index) => {
      const value = Number(point[track.key]);
      if (!Number.isFinite(value)) return null;
      return `${xFor(index).toFixed(2)},${yFor(value).toFixed(2)}`;
    })
    .filter(Boolean)
    .join(" ");
  return `
    <div class="capital-track ${escapeHtml(track.tone)}">
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(track.label)}趋势">
        <line x1="0" y1="${baseline.toFixed(2)}" x2="${width}" y2="${baseline.toFixed(2)}" stroke="var(--line)" stroke-width="1" stroke-dasharray="5 7" />
        <polyline points="${linePoints}" fill="none" stroke="${track.stroke}" stroke-width="1.9" ${track.dash ? `stroke-dasharray="${track.dash}"` : ""} stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </div>
  `;
}

function renderDragonGrid(points) {
  const recent = points.slice(-48);
  if (recent.length < 2) return "";
  const latest = recent[recent.length - 1] || {};
  const dimensions = [
    ["趋势", "trend_heat"],
    ["量价", "volume_price_heat"],
    ["异动", "anomaly_heat"],
    ["人气", "popularity_heat"],
  ];
  return `
    <div class="dragon-grid" role="img" aria-label="四维擒龙热度，红色为高热，蓝色为中等，深色为低位">
      <header>
        <div class="dragon-heading">
          <strong>四维擒龙</strong>
          <span>${escapeHtml(recent[0]?.date || "")} - ${escapeHtml(recent[recent.length - 1]?.date || "")}</span>
        </div>
        ${renderDragonLegend()}
      </header>
      ${dimensions
        .map(([label, key]) => {
          const status = heatStatus(latest[key]);
          return `
            <div class="dragon-row">
              <div class="dragon-label">
                <strong>${escapeHtml(label)}</strong>
                <span class="heat-badge ${status.tone}">${escapeHtml(status.label)} ${formatHeatValue(status.heat)}</span>
              </div>
              <div class="dragon-cells">
                ${recent.map((point) => renderDragonCell(point[key], label)).join("")}
              </div>
            </div>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderDragonLegend() {
  return `
    <div class="dragon-legend" aria-hidden="true">
      <span><i class="heat-dot cool"></i>低位 0-41</span>
      <span><i class="heat-dot warm"></i>中等 42-61</span>
      <span><i class="heat-dot hot"></i>高热 62+</span>
    </div>
  `;
}

function heatStatus(value) {
  const numeric = Number(value);
  const heat = Number.isFinite(numeric) ? Math.max(0, Math.min(100, numeric)) : 0;
  if (heat >= 62) return { heat, tone: "hot", label: "高热" };
  if (heat >= 42) return { heat, tone: "warm", label: "中等" };
  return { heat, tone: "cool", label: "低位" };
}

function renderDragonCell(value, label = "热度") {
  const status = heatStatus(value);
  return `<i class="${status.tone}" style="--heat:${(0.28 + status.heat / 140).toFixed(3)}" title="${escapeHtml(label)} ${escapeHtml(status.label)} ${formatHeatValue(status.heat)}"></i>`;
}

function formatHeatValue(value) {
  const number = Number(value);
  return Number.isFinite(number) ? String(Math.round(number)) : "-";
}

function normalizeCapitalEvidenceSections(evidence) {
  if (Array.isArray(evidence?.sections) && evidence.sections.length) {
    return evidence.sections.filter((section) => section.key !== "external_status");
  }
  return buildFallbackEvidenceSections(evidence);
}

function buildFallbackEvidenceSections(evidence) {
  const items = evidence?.items || [];
  const contributions = evidence?.contributions || {};
  if (!items.length && !Object.keys(contributions).length) return [];
  const definitions = [
    { key: "fund_flow", title: "资金流", contribution: "资金流", categories: ["fund_flow"] },
    {
      key: "institution_lhb",
      title: "机构席位",
      contribution: "机构席位",
      categories: ["institution_lhb", "institution_lhb_status"],
    },
    {
      key: "message_sentiment",
      title: "消息情绪",
      contribution: "消息情绪",
      categories: ["news_rag", "community_sentiment"],
    },
    { key: "technical_behavior", title: "技术推断", contribution: "技术推断", categories: ["technical_behavior"] },
  ];
  return definitions
    .map((definition) => {
      const sectionItems = items.filter((item) => definition.categories.includes(item.category));
      const contribution = definition.contribution ? contributions[definition.contribution] || {} : {};
      const score = Number(contribution.score);
      const available = Boolean(sectionItems.length || contribution.available);
      return {
        key: definition.key,
        title: definition.title,
        score: Number.isFinite(score) ? score : null,
        weight: contribution.weight || 0,
        available,
        summary: available ? `${definition.title}命中 ${sectionItems.length} 条证据` : `${definition.title}暂无证据`,
        items: sectionItems,
      };
    })
    .filter((section) => section.available || section.items.length || section.weight);
}

function renderEvidenceSummary(evidence, sections) {
  if (!sections.length) return renderCapitalContributions(evidence?.contributions || {});
  return `
    <div class="capital-contributions">
      ${sections
        .map((section) => {
          const score = Number(section.score);
          const weight = Number(section.weight);
          return `
            <div class="capital-contribution ${section.available ? "available" : "missing"}">
              <span>${escapeHtml(section.title || capitalCategoryLabel(section.key))}</span>
              <strong>${Number.isFinite(score) ? formatNumber(score) : "缺证据"}</strong>
              <em>${Number.isFinite(weight) && weight > 0 ? `权重 ${Math.round(weight * 100)}%` : capitalSectionStatus(section)}</em>
            </div>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderEvidenceSections(sections, technicalContext = {}) {
  if (!sections.length) return "";
  return `
    <div class="capital-evidence-sections">
      ${sections
        .map((section) => {
          if (section.key === "technical_behavior") {
            return renderTechnicalEvidenceSection(section, technicalContext);
          }
          if (section.key === "institution_lhb") {
            return renderInstitutionSeatSection(section);
          }
          const items = section.items || [];
          const score = Number(section.score);
          return `
            <section class="capital-evidence-section ${section.available ? "available" : "missing"}">
              <header>
                <div>
                  <strong>${escapeHtml(section.title || capitalCategoryLabel(section.key))}</strong>
                  ${section.summary ? `<span>${escapeHtml(section.summary)}</span>` : ""}
                </div>
                <em>${Number.isFinite(score) ? formatNumber(score) : capitalSectionStatus(section)}</em>
              </header>
              ${
                items.length
                  ? `<div class="capital-evidence-list compact">${items.map(renderEvidenceItem).join("")}</div>`
                  : renderEmpty("暂无证据")
              }
            </section>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderTechnicalEvidenceSection(section, technicalContext = {}) {
  const items = section.items || [];
  const score = Number(section.score);
  const { points, latest, metrics } = collectCapitalBehaviorMetrics(technicalContext.series || []);
  const signal = technicalContext.signal || {};
  const evidenceNotes = items
    .map((item) => item.note || item.title || "")
    .filter(Boolean)
    .slice(0, 2);
  const patternScore = signal.pattern_score ?? latest.pattern_score ?? "-";
  return `
    <section class="capital-evidence-section technical-fusion ${section.available ? "available" : "missing"}">
      <header>
        <div>
          <strong>${escapeHtml(section.title || "\u6280\u672f\u63a8\u65ad")}</strong>
          ${section.summary ? `<span>${escapeHtml(section.summary)}</span>` : ""}
        </div>
        <em>${Number.isFinite(score) ? formatNumber(score) : capitalSectionStatus(section)}</em>
      </header>
      <div class="technical-fusion-meta">
        <span>${escapeHtml(latest.date || signal.date || "")}</span>
        <span>\u5f62\u6001 ${escapeHtml(String(patternScore))}</span>
      </div>
      ${points.length >= 2 ? renderMacdChart(points) : ""}
      ${metrics.length ? renderCapitalMetricExplainPanel(metrics, { title: "\u6307\u6807\u4fe1\u606f", hint: "\u5148\u770b\u5206\u6570\u72b6\u6001\uff0c\u518d\u770b\u4e00\u53e5\u8bdd\u5224\u65ad\uff1b\u4e0d\u76f4\u63a5\u4f5c\u4e3a\u4e70\u5356\u7ed3\u8bba\u3002" }) : renderEmpty("\u6280\u672f\u6307\u6807\u4e0d\u8db3")}
      ${points.length >= 2 ? renderDragonGrid(points) : ""}
      ${evidenceNotes.length ? `<div class="technical-fusion-notes">${evidenceNotes.map((note) => `<p>${escapeHtml(sanitizeRuntimeMessage(note, 220))}</p>`).join("")}</div>` : ""}
    </section>
  `;
}

function renderInstitutionSeatSection(section) {
  const items = section.items || [];
  const hitItem = items.find((item) => item.category === "institution_lhb");
  const statusItem = items.find((item) => item.category === "institution_lhb_status");
  if (hitItem) {
    return renderInstitutionSeatHit(section, hitItem);
  }
  return renderInstitutionSeatStatus(section, statusItem);
}

function renderInstitutionSeatHit(section, item) {
  const metrics = item.metrics || {};
  const score = Number(section.score);
  const buy = evidenceMetric(metrics, ["机构买入额", "机构买入总额", "机构席位买入额", "累计买入额", "买入额"]);
  const sell = evidenceMetric(metrics, ["机构卖出额", "机构卖出总额", "机构席位卖出额", "累计卖出额", "卖出额"]);
  const net = evidenceMetric(metrics, ["机构净买额", "净买额", "净额"]);
  const reason = evidenceMetric(metrics, ["上榜原因", "类型", "解读"]) || "龙虎榜机构专用席位记录";
  return `
    <section class="capital-evidence-section institution-seat hit available">
      <header>
        <div>
          <strong>${escapeHtml(section.title || "机构席位")}</strong>
          <span>${escapeHtml(item.title || "命中龙虎榜机构席位")}</span>
        </div>
        <em>${Number.isFinite(score) ? formatNumber(score) : "命中"}</em>
      </header>
      <div class="institution-seat-grid">
        ${renderInstitutionSeatMetric("机构买入", buy || "-")}
        ${renderInstitutionSeatMetric("机构卖出", sell || "-")}
        ${renderInstitutionSeatMetric("机构净买", net || "-")}
        ${renderInstitutionSeatMetric("上榜原因", reason, "wide")}
        ${renderInstitutionSeatMetric("日期", item.date || "-")}
      </div>
      <div class="institution-seat-source">
        <span>${escapeHtml(capitalCategoryLabel(item.category))}</span>
        <span>${escapeHtml(sanitizeRuntimeMessage(item.source || "", 100))}</span>
        <span>${escapeHtml(item.confidence || "中")}置信</span>
      </div>
      ${item.note ? `<p class="institution-seat-note">${escapeHtml(sanitizeRuntimeMessage(item.note, 220))}</p>` : ""}
    </section>
  `;
}

function renderInstitutionSeatStatus(section, item) {
  const status = institutionSeatStatus(item);
  const metrics = item?.metrics || {};
  const title = status === "unavailable" ? "机构席位接口不可用" : "近 N 日未上龙虎榜机构席位";
  const statusText =
    evidenceMetric(metrics, ["状态"]) || (status === "unavailable" ? "接口不可用" : "近 N 日未上榜");
  const note =
    item?.note ||
    (status === "unavailable"
      ? "外部机构席位源暂不可用，资金侧结论不使用机构席位加分。"
      : "没有龙虎榜机构专用席位记录，不代表机构没有买卖。");
  return `
    <section class="capital-evidence-section institution-seat ${status} ${section.available ? "available" : "missing"}">
      <header>
        <div>
          <strong>${escapeHtml(section.title || "机构席位")}</strong>
          <span>${escapeHtml(sanitizeRuntimeMessage(section.summary || title, 140))}</span>
        </div>
        <em>${escapeHtml(status === "unavailable" ? "接口不可用" : "未上榜")}</em>
      </header>
      <div class="institution-seat-state">
        <strong>${escapeHtml(sanitizeRuntimeMessage(item?.title || title, 90))}</strong>
        <span>${escapeHtml(statusText)}</span>
        <p>${escapeHtml(sanitizeRuntimeMessage(note, 220))}</p>
      </div>
      <div class="institution-seat-grid compact">
        ${renderInstitutionSeatMetric("查询窗口", evidenceMetric(metrics, ["查询窗口"]) || "-")}
        ${renderInstitutionSeatMetric("已尝试信源", evidenceMetric(metrics, ["已尝试信源"]) || item?.source || "-", "wide")}
        ${
          status === "unavailable"
            ? renderInstitutionSeatMetric("失败源", evidenceMetric(metrics, ["失败源", "失败原因"]) || "-", "wide")
            : ""
        }
      </div>
    </section>
  `;
}

function renderInstitutionSeatMetric(label, value, extraClass = "") {
  return `
    <div class="institution-seat-metric ${extraClass}">
      <span>${escapeHtml(label)}</span>
      <strong>${escapeHtml(sanitizeRuntimeMessage(value, 180))}</strong>
    </div>
  `;
}

function institutionSeatStatus(item) {
  const text = `${item?.title || ""} ${item?.metrics?.状态 || ""}`.toLowerCase();
  if (text.includes("不可用") || text.includes("unavailable") || text.includes("失败")) return "unavailable";
  if (text.includes("未上榜") || text.includes("未上龙虎榜")) return "no-hit";
  return "no-hit";
}

function evidenceMetric(metrics, labels) {
  if (!metrics) return "";
  for (const label of labels) {
    if (metrics[label] !== undefined && metrics[label] !== null && `${metrics[label]}`.trim()) {
      return `${metrics[label]}`.trim();
    }
  }
  return "";
}

function renderMetricPills(metrics) {
  const entries = Object.entries(metrics || {});
  if (!entries.length) return "";
  return `<div class="mini-metrics">${entries
    .map(([label, value]) => `<span>${escapeHtml(label)} ${escapeHtml(sanitizeRuntimeMessage(value, 160))}</span>`)
    .join("")}</div>`;
}

function renderEvidenceItem(item) {
  const score = Number(item.score);
  return `
    <article class="capital-evidence-item">
      <header>
        <div>
          <strong>${escapeHtml(sanitizeRuntimeMessage(item.title || item.category || "资金证据", 80))}</strong>
          <span>${escapeHtml(capitalCategoryLabel(item.category))} · ${escapeHtml(sanitizeRuntimeMessage(item.source || "", 80))}</span>
        </div>
        <em>${escapeHtml(item.date || "")}</em>
      </header>
      <div class="capital-evidence-tags">
        <span>${escapeHtml(sentimentLabel(item.sentiment))}</span>
        <span>${escapeHtml(item.confidence || "低")}置信</span>
        ${Number.isFinite(score) ? `<span>分 ${formatNumber(score)}</span>` : ""}
      </div>
      ${renderMetricPills(item.metrics) || renderEmpty("没有可展示指标")}
      ${item.note ? `<p>${escapeHtml(sanitizeRuntimeMessage(item.note, 220))}</p>` : ""}
      ${renderExternalSourceLink(item.url)}
    </article>
  `;
}

function sanitizeRuntimeMessage(value, maxChars = 220) {
  const text = String(value ?? "").replace(/\s+/g, " ").trim();
  if (!text) return "";
  const parts = text.split("；").map((part) => part.trim()).filter(Boolean);
  if (parts.length > 1) {
    return parts.slice(0, 3).map((part) => sanitizeRuntimeMessage(part, maxChars)).join("；");
  }
  const [prefix, body] = splitRuntimeMessagePrefix(text);
  if (looksLikeRuntimeError(body)) {
    return `${prefix}${runtimeErrorSummary(body)}`;
  }
  return text.length > maxChars ? `${text.slice(0, maxChars)}...` : text;
}

function splitRuntimeMessagePrefix(text) {
  const prefixes = [
    "个股资金流不可用：",
    "龙虎榜机构席位不可用：",
    "资金证据模型配置不可用，已保留本地规则分：",
    "资金证据模型分析失败，已保留本地规则分：",
    "消息缓存证据不可用：",
    "未抓取资金证据：",
  ];
  const prefix = prefixes.find((item) => text.startsWith(item)) || "";
  return prefix ? [prefix, text.slice(prefix.length).trim()] : ["", text];
}

function looksLikeRuntimeError(text) {
  const lowered = String(text || "").toLowerCase();
  return (
    lowered.includes("httpconnectionpool") ||
    lowered.includes("httpsconnectionpool") ||
    lowered.includes("proxyerror") ||
    lowered.includes("remote disconnected") ||
    lowered.includes("max retries exceeded") ||
    lowered.includes("unable to connect to proxy") ||
    lowered.includes("socksio") ||
    lowered.includes("/api/") ||
    lowered.includes("push2his") ||
    lowered.includes("eastmoney") ||
    String(text || "").includes("超过 ")
  );
}

function runtimeErrorSummary(text) {
  const lowered = String(text || "").toLowerCase();
  if (lowered.includes("socksio") || (lowered.includes("socks") && lowered.includes("not installed"))) {
    return "代理配置缺少 SOCKS 支持，请关闭系统代理或安装 httpx[socks]。";
  }
  if (lowered.includes("proxyerror") || lowered.includes("unable to connect to proxy")) {
    return "网络代理连接失败，已跳过本次外部请求。";
  }
  if (lowered.includes("timeout") || lowered.includes("timed out") || String(text || "").includes("超过 ")) {
    return "外部接口请求超时，已跳过本次请求。";
  }
  if (
    lowered.includes("httpconnectionpool") ||
    lowered.includes("httpsconnectionpool") ||
    lowered.includes("remote disconnected") ||
    lowered.includes("max retries exceeded")
  ) {
    return "外部接口网络连接失败，已跳过本次请求。";
  }
  return "外部接口暂不可用，已降级处理。";
}

function capitalSectionStatus(section) {
  return section.available ? "有证据" : "缺证据";
}

function renderCapitalEvidence(evidence, technicalContext = {}) {
  const sections = normalizeCapitalEvidenceSections(evidence);
  const items = evidence?.items || [];
  const displayItems = items.filter((item) => item.category !== "external_status");
  const notes = evidence?.notes || [];
  if (!displayItems.length && !notes.length && !sections.length) return "";
  const score = Number(evidence.composite_score);
  const modelLabel = evidence.model_used ? "模型参与" : "规则分";
  return `
    <section class="capital-evidence">
      <header>
        <div>
          <h3>综合资金证据</h3>
          <p>${escapeHtml(evidence.summary || "资金流和机构席位优先，消息与技术线辅助。")}</p>
        </div>
        <div class="capital-evidence-score">
          <strong>${Number.isFinite(score) ? formatNumber(score) : "-"}</strong>
          <span>${escapeHtml(evidence.confidence || "低")}置信 · ${escapeHtml(modelLabel)}</span>
        </div>
      </header>
      <div class="capital-evidence-meta">
        <span>交易日 ${escapeHtml(evidence.as_of_trade_date || "-")}</span>
        <span>${escapeHtml(capitalFreshnessLabel(evidence.freshness))}</span>
        <span>${escapeHtml(evidence.generated_at ? formatDateTime(evidence.generated_at) : "")}</span>
      </div>
      ${renderEvidenceSummary(evidence, sections)}
      ${
        sections.length
          ? renderEvidenceSections(sections, technicalContext)
          : displayItems.length
          ? `<div class="capital-evidence-list">${displayItems.map(renderCapitalEvidenceItem).join("")}</div>`
          : ""
      }
      ${notes.length ? renderNotes(notes) : ""}
    </section>
  `;
}

function renderCapitalEvidenceItem(item) {
  return renderEvidenceItem(item);
}

function renderCapitalContributions(contributions) {
  const entries = Object.entries(contributions || {});
  if (!entries.length) return "";
  return `
    <div class="capital-contributions">
      ${entries
        .map(([label, value]) => {
          const score = Number(value?.score);
          const weight = Number(value?.weight);
          return `
            <div class="capital-contribution ${value?.available ? "available" : "missing"}">
              <span>${escapeHtml(label)}</span>
              <strong>${Number.isFinite(score) ? formatNumber(score) : "缺证据"}</strong>
              <em>权重 ${Number.isFinite(weight) ? `${Math.round(weight * 100)}%` : "-"}</em>
            </div>
          `;
        })
        .join("")}
    </div>
  `;
}

function capitalCategoryLabel(category) {
  const labels = {
    fund_flow: "主力资金流",
    institution_lhb: "龙虎榜机构",
    institution_lhb_status: "机构席位状态",
    news_rag: "新闻证据",
    community_sentiment: "社区情绪",
    technical_behavior: "技术推断",
  };
  return labels[category] || category || "证据";
}

function capitalFreshnessLabel(freshness) {
  const labels = {
    "fresh-cache": "缓存命中",
    refreshed: "已刷新",
    "stale-cache": "旧缓存",
  };
  return labels[freshness] || freshness || "-";
}

function sentimentLabel(sentiment) {
  const labels = {
    positive: "偏积极",
    negative: "偏谨慎",
    neutral: "中性",
    uncertain: "不确定",
  };
  return labels[sentiment] || sentiment || "不确定";
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
  stopPanelProgress(node);
  node.className = `${basePanelClass(node)} loading`;
  node.innerHTML = `<div class="loader"></div><span>${escapeHtml(text)}</span>`;
}

function setError(node, title, detail, action) {
  stopPanelProgress(node);
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
    sector_screen: "概念选股",
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
    deducted_net_profit_ok: "扣非净利润达标",
    deducted_net_profit_margin_ok: "扣非净利率达标",
    deducted_net_profit_growth_rate_ok: "扣非净利润增长率达标",
    strong_relation_signal: "强关系信号",
    moderate_relation_signal: "中等关系信号",
    short_buy_signal: "短买",
    red_hold: "红色持股",
    swl_above_sws: "SWL 强于 SWS",
    kdj_golden_cross: "KDJ 金叉",
    kdj_dead_cross: "KDJ 死叉",
    kdj_oversold: "KDJ 低位",
    kdj_overbought: "KDJ 高位",
    high_quant_score: "量化分较高",
    white_exit: "白色离场",
    cyan_watch: "青色观望",
    oversold: "急速超跌",
    accumulation_strength: "吸筹强度较高",
    swing_opportunity: "波段机会",
    bottom_accumulation: "底部吸筹",
    rebound_signal: "绝地反击",
    dragon_trend_volume: "趋势量价共振",
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
