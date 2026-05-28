const $ = (selector) => document.querySelector(selector);

const buttons = {
  screen: $("#screenBtn"),
  graph: $("#graphBtn"),
  trendAnalyze: $("#trendAnalyzeBtn"),
  trendScreen: $("#trendScreenBtn"),
  backtest: $("#backtestBtn"),
  agent: $("#agentBtn"),
  observe: $("#observeBtn"),
};

const panels = {
  screen: $("#screenResult"),
  graph: $("#graphResult"),
  trend: $("#trendResult"),
  backtest: $("#backtestResult"),
  agent: $("#agentResult"),
  observe: $("#observeResult"),
};

const themeToggle = $("#themeToggle");
const themeText = $("#themeText");
const THEME_KEY = "gp-assistant-theme";
const DATA_SOURCE_KEY = "gp-assistant-data-source";
const DATA_REFRESH_KEY = "gp-assistant-source-refresh";
const LLM_SETTINGS_KEY = "gp-assistant-llm-settings";
const dataSource = {
  select: $("#dataSourceSelect"),
  refresh: $("#refreshSource"),
  status: $("#sourceStatus"),
};
const mobileNav = {
  toggle: $("#mobileNavToggle"),
  close: $("#mobileNavClose"),
  panel: $("#mobileNav"),
  overlay: $("#mobileNavOverlay"),
  links: document.querySelectorAll("[data-mobile-nav-link]"),
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
bindActions();

function bindActions() {
  buttons.screen.addEventListener("click", () => runTask(buttons.screen, panels.screen, runScreen));
  buttons.graph.addEventListener("click", () => runTask(buttons.graph, panels.graph, runGraph));
  buttons.trendAnalyze.addEventListener("click", () => runTask(buttons.trendAnalyze, panels.trend, runTrendAnalysis));
  buttons.trendScreen.addEventListener("click", () => runTask(buttons.trendScreen, panels.trend, runTrendScreen));
  buttons.backtest.addEventListener("click", () => runTask(buttons.backtest, panels.backtest, runBacktest));
  buttons.agent.addEventListener("click", () => runTask(buttons.agent, panels.agent, runAgent));
  buttons.observe?.addEventListener("click", () => runTask(buttons.observe, panels.observe, () => runObserve()));
  dataSource.select?.addEventListener("change", () => {
    localStorage.setItem(DATA_SOURCE_KEY, getSelectedDataSource());
    updateSourceStatus();
  });
  dataSource.refresh?.addEventListener("change", () => {
    localStorage.setItem(DATA_REFRESH_KEY, dataSource.refresh.checked ? "true" : "false");
    updateSourceStatus();
  });
  document.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : event.target?.parentElement;
    const action = target?.closest("[data-observe-code]");
    if (!action) return;
    event.preventDefault();
    runObserve(action.dataset.observeCode);
  });
  llmSettings.save?.addEventListener("click", saveLlmSettings);
  llmSettings.clear?.addEventListener("click", clearLlmSettings);
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
  mobileNav.links.forEach((link) => {
    link.addEventListener("click", () => setMobileNavOpen(false));
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") setMobileNavOpen(false);
  });
  trackMobileNavSections();
}

function setMobileNavOpen(isOpen) {
  document.body.classList.toggle("mobile-nav-open", isOpen);
  mobileNav.toggle?.setAttribute("aria-expanded", String(isOpen));
  mobileNav.panel?.setAttribute("aria-hidden", String(!isOpen));
}

function trackMobileNavSections() {
  if (!("IntersectionObserver" in window)) return;
  const links = [...mobileNav.links];
  const sections = links
    .map((link) => document.querySelector(link.getAttribute("href")))
    .filter(Boolean);
  const observer = new IntersectionObserver(
    (entries) => {
      const visible = entries
        .filter((entry) => entry.isIntersecting)
        .sort((left, right) => right.intersectionRatio - left.intersectionRatio)[0];
      if (!visible) return;
      const activeHref = `#${visible.target.id}`;
      links.forEach((link) => link.classList.toggle("active", link.getAttribute("href") === activeHref));
    },
    { rootMargin: "-24% 0px -58% 0px", threshold: [0.08, 0.2, 0.45] },
  );
  sections.forEach((section) => observer.observe(section));
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
  }
}

async function runScreen() {
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
    limit: Math.min(readInt("resultLimit", 30), 100),
  };
  const data = await postJson("/api/graph-screen", payload, panels.graph);
  if (data) renderGraphResult(panels.graph, data);
}

async function runTrendAnalysis() {
  const code = $("#trendCode").value.trim().toUpperCase();
  if (!code) {
    setError(panels.trend, "请输入股票代码", "例如：300750.SZ");
    return;
  }
  setLoading(panels.trend, "趋势指标计算中");
  const payload = {
    code,
    start_date: $("#trendStart").value.trim() || "20200101",
    end_date: $("#trendEnd").value.trim() || "20240101",
    series_limit: 180,
  };
  const data = await postJson("/api/trend", payload, panels.trend);
  if (data) renderTrendAnalysis(panels.trend, data);
}

async function runTrendScreen() {
  setLoading(panels.trend, "趋势选股中");
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    start_date: $("#trendStart").value.trim() || "20200101",
    end_date: $("#trendEnd").value.trim() || "20240101",
    limit: Math.min(readInt("resultLimit", 30), 100),
  };
  const data = await postJson("/api/trend-screen", payload, panels.trend);
  if (data) renderTrendScreenResult(panels.trend, data);
}

async function runBacktest() {
  setLoading(panels.backtest, "回测中");
  const payload = {
    criteria: buildCriteria({ limit: 100 }),
    start_date: $("#btStart").value.trim() || "20200101",
    end_date: $("#btEnd").value.trim() || "20240101",
    top_n: clampInt($("#btTopN").value, 1, 100, 10),
  };
  const data = await postJson("/api/backtest", payload, panels.backtest);
  if (data) renderBacktestResult(panels.backtest, data);
}

async function runAgent() {
  const message = $("#agentMsg").value.trim();
  if (!message) {
    setError(panels.agent, "请输入 Agent 指令", "例如：用产业链关系筛选新能源股票，PE 低于 25。");
    return;
  }
  setLoading(panels.agent, "Agent 分析中");
  const payload = { message };
  const llm = buildLlmConfig();
  if (llm) payload.llm = llm;
  const data = await postJson("/api/agent", payload, panels.agent);
  if (data) renderAgentResult(panels.agent, data);
}

async function runObserve(codeOverride) {
  const code = (codeOverride || $("#observeCode").value || "").trim().toUpperCase();
  if (!code) {
    setError(panels.observe, "请输入股票代码", "例如：300750.SZ");
    return;
  }
  $("#observeCode").value = code;
  document.querySelector("#sectionObserve")?.scrollIntoView({ behavior: "smooth", block: "start" });
  setLoading(panels.observe, "观察行情和技术面");
  const params = new URLSearchParams({
    minute_period: $("#observeMinutePeriod").value || "1",
    series_limit: "160",
    minute_limit: "180",
  });
  const startDate = $("#observeStart").value.trim();
  const endDate = $("#observeEnd").value.trim();
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
  updateSourceStatus();
}

function getSelectedDataSource() {
  return dataSource.select?.value || "mock";
}

function dataSourceHeaders() {
  const headers = { "X-Stock-Provider": getSelectedDataSource() };
  if (dataSource.refresh?.checked) headers["X-Akshare-Refresh"] = "true";
  return headers;
}

function updateSourceStatus() {
  if (!dataSource.status) return;
  const source = getSelectedDataSource();
  const label = source === "akshare" ? "AkShare" : "Mock";
  const suffix = dataSource.refresh?.checked && source === "akshare" ? " 刷新" : "";
  dataSource.status.innerHTML = `<i aria-hidden="true"></i>${escapeHtml(label + suffix)}`;
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
    llmSettings.status.textContent = "自定义 API";
  } else if (hasKey) {
    llmSettings.status.textContent = "自定义 Key";
  } else if (hasEndpoint) {
    llmSettings.status.textContent = "服务端 Key";
  } else {
    llmSettings.status.textContent = "服务端默认";
  }
}

function normalizeBaseUrl(value) {
  return value.trim().replace(/\/+$/, "");
}

function buildCriteria(overrides = {}) {
  const criteria = {
    include_st: $("#includeSt").checked,
    limit: readInt("resultLimit", 30),
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

function parseCodes(raw) {
  return raw
    .split(/[,，\s]+/)
    .map((item) => item.trim().toUpperCase())
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
    body: items.length
      ? renderStockList(items.map(screenItemToView))
      : renderEmpty("没有符合条件的股票"),
    raw: data,
  });
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
  const symbols = data.symbols || [];
  renderResult(node, {
    summary: [
      ["总收益", formatPercent(metrics.total_return)],
      ["年化", formatPercent(metrics.annualized_return)],
      ["最大回撤", formatPercent(metrics.max_drawdown)],
      ["标的数", metrics.num_stocks ?? symbols.length],
    ],
    body: [
      curve.length ? renderSparkline(curve) : renderEmpty("没有可用净值曲线"),
      symbols.length ? `<div class="symbol-strip">${symbols.map(escapeHtml).join(" · ")}</div>` : "",
    ].join(""),
    raw: data,
  });
}

function renderObserveResult(node, data) {
  const stock = data.stock || {};
  const trend = data.trend || {};
  const signal = trend.signal || {};
  const series = trend.series || [];
  const minuteBars = data.minute_bars || [];
  renderResult(node, {
    summary: [
      ["数据源", sourceLabel(data.source)],
      ["最新价", formatNumber(stock.price)],
      ["分钟线", `${data.minute_period || "1"}m · ${minuteBars.length}`],
    ],
    body: [
      renderQuoteCard(stock),
      data.order_book ? renderOrderBook(data.order_book) : renderEmpty("没有可用盘口"),
      trend.signal ? renderSignalCard(stock, signal) : renderEmpty("没有可用日线技术面"),
      minuteBars.length ? renderMinuteChart(minuteBars) : renderEmpty("没有可用分钟线"),
      series.length ? renderTrendChart(series) : "",
      data.notes?.length ? renderNotes(data.notes) : "",
      signal.notes?.length ? renderNotes(signal.notes) : "",
    ].join(""),
    raw: data,
  });
}

function renderAgentResult(node, data) {
  const nested = data.data || {};
  const nestedItems = nested.items || [];
  const nestedMetrics = nested.metrics || {};
  const bodyParts = [`<div class="agent-reply">${escapeHtml(data.reply || "已处理")}</div>`];

  if (data.action === "graph_screen") {
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
  } else if (data.action === "screen") {
    bodyParts.push(
      nestedItems.length
        ? renderStockList(nestedItems.map(screenItemToView))
        : renderEmpty("没有选股结果"),
    );
  } else {
    bodyParts.push(renderEmpty("Agent 没有返回可展示数据"));
  }

  const thirdMetric =
    data.action === "graph_screen"
      ? ["关系边", nested.relation_count ?? "-"]
      : data.action === "trend_screen"
        ? ["最高分", nestedItems[0] ? formatNumber(nestedItems[0].final_score) : "-"]
        : ["关系边", nested.relation_count ?? "-"];

  renderResult(node, {
    summary: [
      ["动作", actionLabel(data.action)],
      ["结果", nested.returned ?? nestedItems.length ?? "-"],
      thirdMetric,
    ],
    body: bodyParts.join(""),
    raw: data,
  });
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
      <summary>原始 JSON</summary>
      <pre>${escapeHtml(JSON.stringify(raw, null, 2))}</pre>
    </details>
  `;
}

function renderStockList(items) {
  return `<div class="stock-list">${items.map(renderStockRow).join("")}</div>`;
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
      <div class="stock-main">
        <div class="stock-title">
          <div>
            <strong>${escapeHtml(stock.name || stock.code || "-")}</strong>
            <span>${escapeHtml(stock.code || "")}</span>
          </div>
          ${observeButton}
        </div>
        <div class="score-badge">
          <small>${escapeHtml(item.scoreLabel || "Score")}</small>
          <b>${formatNumber(item.score)}</b>
          ${weight}
        </div>
      </div>
      <div class="stock-meta">
        <span>${escapeHtml(stock.industry || "Unknown")}</span>
        <span>PE ${formatNumber(stock.pe)}</span>
        <span>PB ${formatNumber(stock.pb)}</span>
        <span>ROE ${formatPercent(stock.roe)}</span>
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
          <p>${escapeHtml(stock.code || "")} · ${escapeHtml(stock.industry || "Unknown")}</p>
        </div>
        <span class="quote-price">${formatNumber(stock.price)}</span>
      </header>
      <div class="quote-grid">
        <div><span>PE</span><strong>${formatNumber(stock.pe)}</strong></div>
        <div><span>PB</span><strong>${formatNumber(stock.pb)}</strong></div>
        <div><span>ROE</span><strong>${formatPercent(stock.roe)}</strong></div>
        <div><span>市值</span><strong>${formatNumber(stock.market_cap_billion)} 亿</strong></div>
        <div><span>股息率</span><strong>${formatPercent(stock.dividend_yield)}</strong></div>
        <div><span>ST</span><strong>${stock.is_st ? "是" : "否"}</strong></div>
      </div>
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
        <div><span>Close</span><strong>${formatNumber(signal.close)}</strong></div>
        <div><span>SWL / SWS</span><strong>${formatNumber(signal.swl)} / ${formatNumber(signal.sws)}</strong></div>
        <div><span>Quant</span><strong>${escapeHtml(String(signal.quant_score ?? 0))}/${escapeHtml(String(signal.quant_score_max ?? 90))}</strong></div>
        <div><span>Support</span><strong>${formatNumber(signal.support)}</strong></div>
        <div><span>Resistance</span><strong>${formatNumber(signal.resistance)}</strong></div>
        <div><span>Breakout</span><strong>${formatNumber(signal.breakout)}</strong></div>
        <div><span>Reversal</span><strong>${formatNumber(signal.reversal)}</strong></div>
        <div><span>Wait</span><strong>${formatNumber(signal.wait_line)}</strong></div>
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
        <span>Minute Close</span>
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
        <span>Close</span>
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

function formatPercent(value) {
  if (value === null || value === undefined || value === "") return "-";
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  return `${(number * 100).toLocaleString("zh-CN", { maximumFractionDigits: 2 })}%`;
}

function actionLabel(action) {
  const labels = {
    screen: "普通选股",
    graph_screen: "关系图",
    trend_screen: "趋势选股",
    backtest: "回测",
    clarify: "澄清",
  };
  return labels[action] || action || "-";
}

function sourceLabel(source) {
  const labels = {
    mock: "Mock",
    akshare: "AkShare",
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
    roe_ok: "ROE 达标",
    pe_ok: "PE 达标",
    pb_ok: "PB 达标",
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
  if (themeText) themeText.textContent = theme === "dark" ? "暗色" : "亮色";
}
