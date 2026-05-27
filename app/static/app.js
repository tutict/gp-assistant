const $ = (selector) => document.querySelector(selector);

const buttons = {
  screen: $("#screenBtn"),
  graph: $("#graphBtn"),
  backtest: $("#backtestBtn"),
  agent: $("#agentBtn"),
};

const panels = {
  screen: $("#screenResult"),
  graph: $("#graphResult"),
  backtest: $("#backtestResult"),
  agent: $("#agentResult"),
};

const themeToggle = $("#themeToggle");
const themeText = $("#themeText");
const THEME_KEY = "gp-assistant-theme";

initTheme();
bindActions();

function bindActions() {
  buttons.screen.addEventListener("click", () => runTask(buttons.screen, panels.screen, runScreen));
  buttons.graph.addEventListener("click", () => runTask(buttons.graph, panels.graph, runGraph));
  buttons.backtest.addEventListener("click", () => runTask(buttons.backtest, panels.backtest, runBacktest));
  buttons.agent.addEventListener("click", () => runTask(buttons.agent, panels.agent, runAgent));
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
  const data = await postJson("/api/agent", { message }, panels.agent);
  if (data) renderAgentResult(panels.agent, data);
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
      headers: { "Content-Type": "application/json" },
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

  renderResult(node, {
    summary: [
      ["动作", actionLabel(data.action)],
      ["结果", nested.returned ?? nestedItems.length ?? "-"],
      ["关系边", nested.relation_count ?? "-"],
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
  const weight = item.weight !== undefined ? `<span class="weight">${formatPercent(item.weight)}</span>` : "";
  const related = item.related?.length ? renderRelated(item.related) : "";

  return `
    <article class="stock-row">
      <div class="stock-main">
        <div>
          <strong>${escapeHtml(stock.name || stock.code || "-")}</strong>
          <span>${escapeHtml(stock.code || "")}</span>
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
      ${reasons}
      ${related}
    </article>
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
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  if (Math.abs(number) >= 1000) return number.toLocaleString("zh-CN", { maximumFractionDigits: 0 });
  return number.toLocaleString("zh-CN", { maximumFractionDigits: 3 });
}

function formatPercent(value) {
  const number = Number(value);
  if (!Number.isFinite(number)) return "-";
  return `${(number * 100).toLocaleString("zh-CN", { maximumFractionDigits: 2 })}%`;
}

function actionLabel(action) {
  const labels = {
    screen: "普通选股",
    graph_screen: "关系图",
    backtest: "回测",
    clarify: "澄清",
  };
  return labels[action] || action || "-";
}

function reasonLabel(reason) {
  const labels = {
    roe_ok: "ROE 达标",
    pe_ok: "PE 达标",
    pb_ok: "PB 达标",
    mcap_ok: "市值达标",
    strong_relation_signal: "强关系信号",
    moderate_relation_signal: "中等关系信号",
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
