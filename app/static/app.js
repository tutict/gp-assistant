const $ = (selector) => document.querySelector(selector);

const buttons = {
  screen: $("#screenBtn"),
  graph: $("#graphBtn"),
  trendAnalyze: $("#trendAnalyzeBtn"),
  trendScreen: $("#trendScreenBtn"),
  backtest: $("#backtestBtn"),
  newsRag: $("#newsRagBtn"),
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
const LLM_SETTINGS_KEY = "gp-assistant-llm-settings";
const DEFAULT_RESULT_LIMIT = 10;
const STOCK_SEARCH_LIMIT = 3;
const dataSource = {
  select: $("#dataSourceSelect"),
  refresh: $("#refreshSource"),
  proxy: $("#proxyModeSelect"),
  status: $("#sourceStatus"),
  universe: $("#universeCount"),
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

initTheme();
initMobileNav();
initDataSource();
initLlmSettings();
initFormControls();
bindActions();

function bindActions() {
  buttons.screen.addEventListener("click", () => runTask(buttons.screen, panels.screen, runScreen));
  buttons.graph.addEventListener("click", () => runTask(buttons.graph, panels.graph, runGraph));
  buttons.trendAnalyze.addEventListener("click", () => runTask(buttons.trendAnalyze, panels.trend, runTrendAnalysis));
  buttons.trendScreen.addEventListener("click", () => runTask(buttons.trendScreen, panels.trend, runTrendScreen));
  buttons.backtest.addEventListener("click", () => runTask(buttons.backtest, panels.backtest, runBacktest));
  buttons.newsRag?.addEventListener("click", () => runTask(buttons.newsRag, panels.newsRag, runNewsRag));
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
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const action = target?.closest("[data-observe-code]");
    if (!action) return;
    event.preventDefault();
    runObserve(action.dataset.observeCode);
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const action = target?.closest("[data-run-backtest]");
    if (!action) return;
    event.preventDefault();
    runBacktestFromScreen();
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
    setLoading(panels.screen, "按板块筛选中");
    const payload = {
      criteria: buildCriteria(),
      max_sectors: clampInt($("#maxSectors")?.value, 1, 50, 8),
      per_sector_limit: clampInt($("#perSectorLimit")?.value, 1, 50, 3),
      min_sector_candidates: 1,
    };
    const data = await postJson("/api/sector-screen", payload, panels.screen);
    if (data) renderSectorScreenResult(panels.screen, data);
    return;
  }

  setLoading(panels.screen, "筛选中");
  const payload = buildCriteria();
  const data = await postJson("/api/screen", payload, panels.screen);
  if (data) renderScreenResult(panels.screen, data);
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
    end_date: readDateParam("trendEnd", "20240101"),
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
    end_date: readDateParam("trendEnd", "20240101"),
    limit: Math.min(readInt("resultLimit", DEFAULT_RESULT_LIMIT), 100),
  };
  const data = await postJson("/api/trend-screen", payload, panels.trend);
  if (data) renderTrendScreenResult(panels.trend, data);
}

async function runBacktest() {
  setLoading(panels.backtest, "回测中");
  updateBacktestScope();
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    start_date: readDateParam("btStart", "20200101"),
    end_date: readDateParam("btEnd", "20240101"),
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

async function runNewsRag() {
  const timer = startPanelProgress(panels.newsRag, "上下游消息分析中", [
    [18, "读取已有关系图"],
    [38, "更新本地消息缓存"],
    [62, "检索证据"],
    [82, "生成影响判断"],
  ]);
  const code = readStockCode("newsCode");
  if (code) $("#newsCode").value = code;
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    code: code || null,
    seed_codes: code ? [code] : parseCodes($("#seedCodes")?.value || ""),
    days: clampInt($("#newsDays")?.value, 1, 365, 30),
    max_items: 24,
  };
  try {
    const data = await postJson("/api/news-rag", payload, panels.newsRag);
    if (data) renderNewsRagResult(panels.newsRag, data);
  } finally {
    if (timer) window.clearInterval(timer);
  }
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
    setError(panels.observe, "请输入股票代码", "例如：300750.SZ");
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
  const savedSource = localStorage.getItem(DATA_SOURCE_KEY);
  if (savedSource && [...dataSource.select.options].some((option) => option.value === savedSource)) {
    dataSource.select.value = savedSource;
  }
  if (dataSource.refresh) {
    dataSource.refresh.checked = localStorage.getItem(DATA_REFRESH_KEY) === "true";
  }
  const savedProxy = localStorage.getItem(DATA_PROXY_KEY);
  if (dataSource.proxy && savedProxy && [...dataSource.proxy.options].some((option) => option.value === savedProxy)) {
    dataSource.proxy.value = savedProxy;
  }
  updateSourceStatus();
  loadDataStatus();
}

function getSelectedDataSource() {
  return dataSource.select?.value || "astock";
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
  return dataSource.proxy?.value || "system";
}

function updateSourceStatus() {
  if (!dataSource.status) return;
  const source = getSelectedDataSource();
  const label = sourceLabel(source);
  const suffix = dataSource.refresh?.checked ? " 刷新" : "";
  const proxySuffix = getSelectedProxyMode() === "none" ? " 直连" : "";
  dataSource.status.innerHTML = `<i aria-hidden="true"></i>${escapeHtml(label + suffix + proxySuffix)}`;
}

async function runDataTask(button, task) {
  const original = button.textContent;
  button.disabled = true;
  button.textContent = "处理中";
  try {
    await task();
  } finally {
    button.disabled = false;
    button.textContent = original;
  }
}

async function loadDataStatus() {
  if (!dataSource.universe) return;
  setMaintenanceNote("读取数据状态中");
  try {
    const resp = await fetch("/api/data-sources/status", {
      method: "GET",
      headers: dataSourceHeaders(),
    });
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    renderDataStatus(await resp.json());
  } catch (err) {
    setMaintenanceNote(`数据状态读取失败：${err.message}`);
  }
}

async function refreshUniverse() {
  const progress = startRefreshProgress();
  setMaintenanceNote("刷新股票池中，真实数据源可能需要几十秒");
  try {
    const resp = await fetch("/api/data-sources/refresh-universe", {
      method: "POST",
      headers: { "Content-Type": "application/json", ...dataSourceHeaders() },
      body: JSON.stringify({ mode: "light", max_bytes: 209715200, daily_days: 500, minute_days: 3 }),
    });
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
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
    const resp = await fetch("/api/data-sources/prune-cache", {
      method: "POST",
      headers: { "Content-Type": "application/json", ...dataSourceHeaders() },
      body: JSON.stringify({ mode: "light", max_bytes: 209715200, daily_days: 500, minute_days: 3 }),
    });
    if (!resp.ok) throw new Error(await resp.text());
    const data = await resp.json();
    renderDataStatus(data.status);
    setMaintenanceNote(`已删除 ${data.removed_files || 0} 个文件，释放 ${formatBytes(data.removed_bytes || 0)}。`);
  } catch (err) {
    setMaintenanceNote(`缓存清理失败：${err.message}`);
  }
}

function renderDataStatus(status) {
  if (!status || !dataSource.universe) return;
  dataSource.universe.textContent = `${formatNumber(status.universe_count)} 只`;
  dataSource.cache.textContent = formatBytes(status.cache_bytes);
  dataSource.updated.textContent = status.universe_updated_at
    ? `更新于 ${formatDateTime(status.universe_updated_at)}`
    : "未建立缓存";
  const policy = status.policy || {};
  dataSource.policy.textContent = `${policy.mode === "full" ? "完整" : "轻量"}模式 · 上限 ${formatBytes(status.cache_limit_bytes)}`;
  setMaintenanceNote((status.notes || []).join(" ") || "数据状态正常");
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
    [85, "等待数据源返回"],
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
        const resp = await fetch(`/api/stock-search?${params}`, {
          method: "GET",
          headers: stockSearchHeaders(),
        });
        if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
        const items = await resp.json();
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
    const dateValue = toDateInputValue(input.value);
    if (dateValue) input.value = dateValue;
    input.addEventListener("click", () => showDatePicker(input));
  });
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
  const parts = [
    $("#industry")?.value ? `行业 ${$("#industry").value}` : "全部行业",
    `持仓 ${clampInt($("#btTopN")?.value, 1, 100, 10)} 只`,
    `${displayDateParam("btStart", "2020-01-01")} 至 ${displayDateParam("btEnd", "2024-01-01")}`,
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
    const resp = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...dataSourceHeaders() },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      const text = await resp.text();
      setError(resultNode, `请求失败：${resp.status}`, text);
      return null;
    }
    return await resp.json();
  } catch (err) {
    setError(resultNode, "请求异常", err.message);
    return null;
  }
}

async function getJson(url, resultNode) {
  try {
    const resp = await fetch(url, {
      method: "GET",
      headers: dataSourceHeaders(),
    });
    if (!resp.ok) {
      const text = await resp.text();
      setError(resultNode, `请求失败：${resp.status}`, text);
      return null;
    }
    return await resp.json();
  } catch (err) {
    setError(resultNode, "请求异常", err.message);
    return null;
  }
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
      items.length ? renderStockList(items.map(screenItemToView)) : renderEmpty("没有符合条件的股票"),
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
      groups.length ? renderSectorGroups(groups) : renderEmpty("没有符合条件的板块"),
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
      curve.length ? renderSparkline(curve) : renderEmpty("没有可用净值曲线"),
      benchmarkCurve.length ? renderBenchmarkSparkline(benchmarkCurve) : "",
      renderBacktestComparison(data),
      symbols.length ? `<div class="symbol-strip">${symbols.map(escapeHtml).join(" · ")}</div>` : "",
      renderBacktestReliability(data),
    ].join(""),
    raw: data,
  });
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
  if (tier === "community") return "社区 / 待核查";
  return "新闻 / 事实";
}

function sourceTierClass(tier) {
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

function renderBacktestReliability(data) {
  const notes = data.notes || [];
  if (!notes.length) return "";
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
        ${observeButton}
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

function renderEmpty(text) {
  return `<div class="empty-state">${escapeHtml(text)}</div>`;
}

function setLoading(node, text) {
  node.className = `${basePanelClass(node)} loading`;
  node.innerHTML = `<div class="loader"></div><span>${escapeHtml(text)}</span>`;
}

function setError(node, title, detail) {
  node.className = `${basePanelClass(node)} error`;
  node.innerHTML = `<strong>${escapeHtml(title)}</strong><p>${escapeHtml(detail || "")}</p>`;
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
    akshare: "公开行情",
    eastmoney: "东方财富",
    astock: "A股全栈",
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
  };
  return labels[type] || type || "关系";
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
