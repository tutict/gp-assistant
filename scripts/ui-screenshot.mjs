#!/usr/bin/env node

import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const { chromium } = frontendRequire("playwright");

const option = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const checkOnly = process.argv.includes("--check-only");
const serveBuild = process.argv.includes("--serve");
let baseUrl = option("--url", "http://127.0.0.1:4173").replace(/\/$/, "");
const date = new Date().toISOString().slice(0, 10);
const checkSearchOverlay = process.argv.includes('--check-search-overlay');
const headerSettingsOnly = process.argv.includes("--header-settings-only");
const defaultOutput = fileURLToPath(
  new URL("../artifacts/ui-shots/" + date + "/", import.meta.url),
);
const outputRoot = resolve(option("--output", defaultOutput));
const buildRoot = resolve(fileURLToPath(new URL("../desktop/mobile-dist/", import.meta.url)));
const devices = [
  { name: "phone-360", width: 360, height: 780, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-430", width: 430, height: 932, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "tablet-768", width: 768, height: 1024, dpr: 2, mobile: true, theme: "dark", density: "comfortable" },
  { name: "desktop-1280", width: 1280, height: 860, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-2560", width: 2560, height: 1440, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1440-compact", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "compact" },
];
const routes = [
  { name: "screen", hash: "#sectionScreen", ready: ".screen-panel-container" },
  { name: "observe", hash: "#sectionObserve", ready: ".observe-panel-container" },
  { name: "backtest", hash: "#sectionBacktest", ready: ".backtest-context" },
  { name: "news", hash: "#sectionNewsRag", ready: ".research-workspace" },
  { name: "agent", hash: "#sectionAgent", ready: ".agent-workspace" },
  {
    name: "settings",
    hash: "#sectionAgent",
    prepare: async (page) => {
      const mobileMenu = page.locator(".agent-mobile-menu");
      if (await mobileMenu.isVisible()) {
        await mobileMenu.click();
        await page.locator(".agent-workspace:not(.rail-collapsed)").waitFor();
      }
      const toggle = page.locator(".llm-settings-toggle");
      await toggle.scrollIntoViewIfNeeded();
      await toggle.click();
      await page.locator(".llm-settings-body").waitFor({ state: "visible" });
    },
  },
];

const mockStocks = [
  { code: "600519.SH", name: "贵州茅台", industry: "食品饮料", price: 1428.6, change_pct: 0.018, pe: 22.4, pb: 7.3, roe: 31.2 },
  { code: "000858.SZ", name: "五粮液", industry: "食品饮料", price: 128.4, change_pct: -0.006, pe: 18.7, pb: 4.6, roe: 24.1 },
  { code: "300750.SZ", name: "宁德时代", industry: "电力设备", price: 286.1, change_pct: 0.012, pe: 19.8, pb: 4.1, roe: 22.7 },
];
const mockScreenResult = {
  total: mockStocks.length,
  returned: mockStocks.length,
  items: mockStocks.map((stock, index) => ({
    stock,
    score: 88 - index * 5,
    reasons: ["盈利质量稳定", "趋势强度达标"],
    quality_score: 90 - index * 4,
    trend_score: 84 - index * 3,
    risk_score: 76 - index * 2,
    factor_scores: { quality: 90 - index * 4, trend: 84 - index * 3, risk: 76 - index * 2 },
    score_explanation: "基于质量、趋势与风险因子的桌面回归样例。",
    reason_tags: ["质量", "趋势"],
  })),
  notes: ["截图 harness 注入的确定性榜单数据。"],
};
const mockTrendSeries = Array.from({ length: 90 }, (_, index) => {
  const dateValue = new Date(Date.UTC(2026, 3, 1 + index));
  const close = 118 + index * 0.34 + Math.sin(index / 4) * 3.2;
  return {
    date: dateValue.toISOString().slice(0, 10),
    open: close - 0.8,
    high: close + 1.7,
    low: close - 1.9,
    close,
    volume: 1800000 + index * 21000,
    swl: close - 1.4,
    sws: close + Math.sin(index / 7),
    k: 48 + Math.sin(index / 5) * 22,
    d: 50 + Math.sin(index / 6) * 17,
    j: 44 + Math.sin(index / 4) * 28,
  };
});
const mockObserveResult = {
  source: "ui-harness",
  stock: mockStocks[0],
  financial_indicators: {
    title: "核心财务指标",
    period: "2026Q2",
    source: "mock",
    items: [
      { metric_key: "roe", label: "ROE", value: 31.2, tone: "positive" },
      { metric_key: "revenue_growth", label: "营收增长", value: 12.6, tone: "positive" },
      { metric_key: "net_profit_growth", label: "净利增长", value: 15.4, tone: "positive" },
    ],
  },
  trend: {
    stock: mockStocks[0],
    signal: {
      code: "600519.SH",
      date: "2026-07-29",
      close: 148.2,
      previous_close: 146.8,
      close_change_pct: 0.0095,
      quant_score: 78,
      quant_score_max: 100,
      pattern_score: 8,
      pattern_score_max: 10,
      status: "hold",
      reasons: ["中期趋势保持向上", "动量处于健康区间"],
    },
    series: mockTrendSeries,
  },
  capital_evidence: {
    stock_code: "600519.SH",
    composite_score: 74,
    confidence: "high",
    as_of_trade_date: "2026-07-29",
    freshness: "fresh",
    summary: "资金证据与基本面方向一致。",
    sections: [
      { key: "northbound", title: "北向资金", score: 78, available: true, summary: "近五日保持净流入。" },
      { key: "institution", title: "机构席位", score: 69, available: true, summary: "公开席位交易活跃。" },
    ],
  },
  notes: ["截图 harness 注入的确定性观察数据。"],
};
const curve = Array.from({ length: 18 }, (_, index) => ({
  date: new Date(Date.UTC(2025, index, 1)).toISOString().slice(0, 10),
  equity: 1 + index * 0.018 + Math.sin(index / 2) * 0.012,
}));
const benchmarkCurve = curve.map((point, index) => ({
  date: point.date,
  equity: 1 + index * 0.011 + Math.sin(index / 3) * 0.008,
}));
const mockBacktestResult = {
  metrics: {
    total_return: 0.326,
    annualized_return: 0.184,
    max_drawdown: -0.087,
    num_stocks: 10,
    benchmark_total_return: 0.197,
    benchmark_annualized_return: 0.116,
    benchmark_max_drawdown: -0.104,
    excess_return: 0.129,
    total_transaction_cost: 0.006,
    total_turnover: 2.8,
    rebalance_count: 17,
    precision_at_n: 0.64,
  },
  equity_curve: curve,
  benchmark_curve: benchmarkCurve,
  symbols: mockStocks.map((stock) => stock.code),
  benchmark_symbols: mockStocks.map((stock) => stock.code),
  strategy_mode: "candidate_snapshot",
  notes: ["截图 harness 注入的确定性回测数据。"],
};
const mockDataStatus = {
  universe_count: 5231,
  cache_bytes: 67108864,
  quote_trade_date: "20260804",
  current_trade_date: "20260804",
  stale: false,
};

const headerBaselineDevices = [
  { name: "desktop-1440-dark", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1920-dark", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920-light", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "phone-390-dark", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390-light", width: 390, height: 844, dpr: 3, mobile: true, theme: "light", density: "comfortable" },
];

const replayConversationId = "ui-replay-conversation";
const replayRunId = "ui-replay-run";
const replayConversation = {
  id: replayConversationId,
  title: "Agent 复盘浏览器门禁",
  mode: "expert",
  messages: [
    { role: "user", content: "复盘这次市场诊断", timestamp: 1_786_423_200_000 },
    {
      role: "assistant",
      content: "市场诊断已完成，可查看运行复盘。",
      timestamp: 1_786_423_204_000,
      runId: replayRunId,
    },
  ],
  createdAt: 1_786_423_200_000,
  updatedAt: 1_786_423_204_000,
};
const mockAgentRunSummary = {
  run_id: replayRunId,
  conversation_id: replayConversationId,
  question: "复盘这次市场诊断",
  mode: "expert",
  status: "completed",
  started_at_epoch_ms: 1_786_423_200_000,
  completed_at_epoch_ms: 1_786_423_204_000,
  duration_ms: 4_000,
};
const mockAgentRunDetail = {
  ...mockAgentRunSummary,
  events: [
    { type: "status", stage: "planning", label: "市场状态诊断" },
    { type: "tool_start", payload: { tool: "adaptive_screen", label: "自适应筛选" } },
    { type: "tool_result", payload: { status: "completed", output_summary: "候选与证据已生成" } },
    { type: "evidence", label: "证据目录已锁定" },
    { type: "final", message: "复盘结论已生成" },
  ],
  result: {
    reply: "市场诊断与决策路径已经完成。",
    action: "data_status",
    answer_sections: [
      { title: "复盘结论", bullets: ["市场状态与证据方向一致。"] },
    ],
    warnings: ["仅用于浏览器门禁验证。"],
    data: { source: "ui-replay-harness", state: "ready" },
  },
};

const contentTypes = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

async function startBuiltAppServer() {
  const indexPath = resolve(buildRoot, "index.html");
  if (!existsSync(indexPath)) {
    throw new Error("Built frontend is missing. Run npm run build:app before --serve.");
  }
  const rootPrefix = buildRoot.endsWith(sep) ? buildRoot : buildRoot + sep;
  const server = createServer((request, response) => {
    const requestUrl = new URL(request.url || "/", "http://127.0.0.1");
    const relativePath = decodeURIComponent(requestUrl.pathname).replace(/^\/+/, "") || "index.html";
    const candidate = resolve(buildRoot, relativePath);
    const allowed = candidate === buildRoot || candidate.startsWith(rootPrefix);
    const filePath = allowed && existsSync(candidate) && statSync(candidate).isFile()
      ? candidate
      : indexPath;
    response.writeHead(200, {
      "content-type": contentTypes[extname(filePath)] || "application/octet-stream",
      "cache-control": "no-store",
    });
    response.end(readFileSync(filePath));
  });
  await new Promise((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Unable to resolve harness server address");
  return {
    server,
    url: "http://127.0.0.1:" + address.port,
  };
}

function deviceUrl(device, hash) {
  const query = new URLSearchParams();
  if (device.theme === "light") query.set("theme", "light");
  if (device.density === "compact") query.set("density", "compact");
  return baseUrl + "/" + (query.size ? "?" + query.toString() : "") + hash;
}

async function installHarnessState(page) {
  await page.addInitScript((stocks) => {
    localStorage.setItem("stock-optimizer-watchlist", JSON.stringify(
      stocks.map((stock) => ({
        code: stock.code,
        name: stock.name,
        industry: stock.industry,
        source: "ui-harness",
        added_at: "2026-08-04T09:00:00.000Z",
      })),
    ));
  }, mockStocks);
  await page.route("**/api/data-sources/status", (route) => route.fulfill({ json: mockDataStatus }));
  await page.route("**/api/stocks?*", (route) => route.fulfill({ json: mockStocks }));
  await page.route("**/api/screen", (route) => route.fulfill({ json: mockScreenResult }));
  await page.route("**/api/observe/**", (route) => route.fulfill({ json: mockObserveResult }));
  await page.route("**/api/backtest", (route) => route.fulfill({ json: mockBacktestResult }));
  await page.route("**/api/research/overview", (route) => route.fulfill({ json: {} }));
  await page.route("**/api/research/messages?*", (route) => route.fulfill({ json: { items: [] } }));
  await page.route("**/api/research/threads", (route) => route.fulfill({ json: { items: [] } }));
  await page.route("**/api/research/index-status", (route) => route.fulfill({ json: {} }));
  await page.route("**/api/research/refresh*", (route) => route.fulfill({ json: { refreshed: true } }));
}

async function captureHeaderBaselines(browser, targetRoot) {
  const reports = [];
  for (const device of headerBaselineDevices) {
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: device.mobile,
      isMobile: device.mobile,
      colorScheme: device.theme,
    });
    try {
      const page = await context.newPage();
      await installHarnessState(page);
      await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
      await page.locator(".app-header").waitFor({ state: "visible" });
      const directory = resolve(targetRoot, "header-settings", device.name);
      mkdirSync(directory, { recursive: true });
      await page.screenshot({ path: resolve(directory, "header-default.png"), fullPage: false });

      const trigger = page.locator(".settings-trigger");
      await trigger.click();
      const dialog = page.getByRole("dialog", { name: "设置" });
      await dialog.waitFor({ state: "visible" });
      await page.waitForTimeout(240);
      await page.screenshot({ path: resolve(directory, "settings-open.png"), fullPage: false });

      const panel = await dialog.boundingBox();
      if (!panel) throw new Error(`${device.name} settings panel has no bounding box`);
      const bottom = panel.y + panel.height;
      if (device.mobile) {
        if (panel.width < device.width - 2 || Math.abs(bottom - device.height) > 2
          || panel.height > device.height * 0.78 + 2) {
          throw new Error(`${device.name} settings sheet geometry is invalid: ${JSON.stringify(panel)}`);
        }
      } else if (Math.abs(panel.x + panel.width - device.width) > 2
        || Math.abs(panel.height - device.height) > 2) {
        throw new Error(`${device.name} settings drawer geometry is invalid: ${JSON.stringify(panel)}`);
      }

      reports.push({ device: device.name, panel });
    } finally {
      await context.close();
    }
  }
  return reports;
}

const boxFor = async (page, selector) => {
  const locator = page.locator(selector).first();
  if (!await locator.count()) return null;
  return locator.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return {
      x: Math.round(box.x),
      y: Math.round(box.y),
      width: Math.round(box.width),
      height: Math.round(box.height),
      display: style.display,
      position: style.position,
      visibility: style.visibility,
    };
  });
};

const pageDiagnostics = async (page, routeName) => page.evaluate((name) => {
  const root = document.querySelector("#root");
  const app = document.querySelector(".app");
  const workspace = document.querySelector(".agent-workspace");
  const rail = document.querySelector(".agent-rail");
  const activeView = app?.getAttribute("data-active-view") || "";
  const overlay = document.querySelector("vite-error-overlay, #webpack-dev-server-client-overlay, [data-nextjs-dialog-overlay]");
  const railStyle = rail ? getComputedStyle(rail) : null;
  return {
    route: name,
    title: document.title,
    activeView,
    rootTextLength: root?.textContent?.trim().length || 0,
    rootWidth: root?.clientWidth || 0,
    rootScrollWidth: root?.scrollWidth || 0,
    bodyWidth: document.body.clientWidth,
    bodyScrollWidth: document.body.scrollWidth,
    frameworkOverlay: Boolean(overlay),
    workspaceClass: workspace?.className || "",
    railTransform: railStyle?.transform || "",
    railPosition: railStyle?.position || "",
    activeNavLabels: Array.from(document.querySelectorAll(".nav-link.active")).map((element) => element.textContent?.trim() || ""),
  };
}, routeName);

function assertPageDiagnostics(diagnostics, deviceName) {
  if (diagnostics.rootTextLength < 10) {
    throw new Error(deviceName + "/" + diagnostics.route + " rendered a blank application shell");
  }
  if (diagnostics.frameworkOverlay) {
    throw new Error(deviceName + "/" + diagnostics.route + " rendered a framework error overlay");
  }
  if (diagnostics.bodyScrollWidth > diagnostics.bodyWidth + 1) {
    throw new Error(deviceName + "/" + diagnostics.route + " has horizontal body overflow");
  }
}

async function runShortcutChecks(page, device) {
  await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
  await page.locator("#root").waitFor({ state: "visible" });

  await page.keyboard.press("2");
  await page.waitForFunction(() => window.location.hash === "#sectionObserve");

  await page.keyboard.press("Control+K");
  const search = page.getByLabel("搜索股票");
  if (!await search.evaluate((element) => document.activeElement === element)) {
    throw new Error("Ctrl+K did not focus global stock search");
  }

  await page.keyboard.press("1");
  if (await page.evaluate(() => window.location.hash) !== "#sectionObserve") {
    throw new Error("Number shortcut fired while the search input was focused");
  }

  await search.evaluate((element) => element.blur());
  await page.keyboard.press("?");
  const help = page.getByRole("dialog", { name: "快捷键帮助" });
  await help.waitFor({ state: "visible" });
  await page.keyboard.press("Escape");
  await help.waitFor({ state: "hidden" });

  return {
    ctrlKFocusedSearch: true,
    numberNavigationHash: "#sectionObserve",
    editableTargetIgnored: true,
    helpOpenedAndClosed: true,
  };
}

async function runSearchOverlayChecks(page, device) {
  await page.route('**/api/stock-search?*', (route) => route.fulfill({ json: mockStocks }));
  await page.goto(deviceUrl(device, '#sectionScreen'), { waitUntil: 'networkidle' });
  await page.locator('.header-search input').fill('tc');
  await page.locator('.header-search .stock-suggest button').first().waitFor({ state: 'visible' });

  const layering = await page.evaluate(() => {
    const header = document.querySelector('.app-header');
    const sidebar = document.querySelector('.sidebar');
    const dropdown = document.querySelector('.header-search .stock-suggest');
    if (!header || !sidebar || !dropdown) throw new Error('Search overlay elements are missing');
    const headerZIndex = Number.parseInt(getComputedStyle(header).zIndex, 10);
    const sidebarZIndex = Number.parseInt(getComputedStyle(sidebar).zIndex, 10);
    const dropdownBox = dropdown.getBoundingClientRect();
    const sidebarBox = sidebar.getBoundingClientRect();
    const overlapWidth = Math.max(0, Math.min(dropdownBox.right, sidebarBox.right) - Math.max(dropdownBox.left, sidebarBox.left));
    const overlapHeight = Math.max(0, Math.min(dropdownBox.bottom, sidebarBox.bottom) - Math.max(dropdownBox.top, sidebarBox.top));
    const sampleX = Math.max(dropdownBox.left, sidebarBox.left) + Math.min(8, overlapWidth / 2);
    const sampleY = Math.max(dropdownBox.top, sidebarBox.top) + Math.min(8, overlapHeight / 2);
    const topElement = overlapWidth > 0 && overlapHeight > 0
      ? document.elementFromPoint(sampleX, sampleY)
      : dropdown;
    return {
      headerZIndex,
      sidebarZIndex,
      dropdownIsTopLayer: Boolean(topElement && dropdown.contains(topElement)),
      dropdownBox: {
        x: Math.round(dropdownBox.x),
        y: Math.round(dropdownBox.y),
        width: Math.round(dropdownBox.width),
        height: Math.round(dropdownBox.height),
      },
      sidebarBox: {
        x: Math.round(sidebarBox.x),
        y: Math.round(sidebarBox.y),
        width: Math.round(sidebarBox.width),
        height: Math.round(sidebarBox.height),
      },
      overlap: Math.round(overlapWidth * overlapHeight),
    };
  });

  if (!layering.dropdownIsTopLayer) {
    throw new Error('Header search overlay is obscured by the sidebar: ' + JSON.stringify(layering));
  }
  return layering;
}

async function installAgentReplayState(page) {
  await page.addInitScript(({ conversation, activeConversationId }) => {
    localStorage.setItem("stock-optimizer-agent-conversations", JSON.stringify([conversation]));
    localStorage.setItem("stock-optimizer-agent-active-conversation", JSON.stringify(activeConversationId));
  }, {
    conversation: replayConversation,
    activeConversationId: replayConversationId,
  });
}

async function installAgentReplayRoutes(page, requests) {
  await page.route("**/api/agent/runs?*", (route) => {
    const url = new URL(route.request().url());
    requests.list.push({
      conversationId: url.searchParams.get("conversation_id"),
      url: url.toString(),
    });
    return route.fulfill({ json: { runs: [mockAgentRunSummary] } });
  });
  await page.route("**/api/agent/runs/**", (route) => {
    const url = new URL(route.request().url());
    requests.detail.push(url.toString());
    if (url.pathname !== `/api/agent/runs/${replayRunId}`) {
      return route.fulfill({ status: 404, json: { run: null } });
    }
    return route.fulfill({ json: { run: mockAgentRunDetail } });
  });
}

function assertReplay(condition, message, diagnostics) {
  if (condition) return;
  const suffix = diagnostics === undefined ? "" : ": " + JSON.stringify(diagnostics);
  throw new Error("Agent replay check failed: " + message + suffix);
}

function assertBoxInViewport(box, device, label) {
  assertReplay(Boolean(box), label + " has no bounding box");
  const tolerance = 2;
  assertReplay(
    box.x >= -tolerance
      && box.y >= -tolerance
      && box.x + box.width <= device.width + tolerance
      && box.y + box.height <= device.height + tolerance,
    label + " is outside the viewport",
    { box, viewport: { width: device.width, height: device.height } },
  );
}

async function replayGeometry(page) {
  return page.evaluate(() => {
    const rect = (selector) => {
      const element = document.querySelector(selector);
      if (!element) return null;
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return {
        x: box.x,
        y: box.y,
        width: box.width,
        height: box.height,
        right: box.right,
        bottom: box.bottom,
        position: style.position,
        overflowY: style.overflowY,
      };
    };
    const drawer = document.querySelector(".agent-run-drawer");
    const sample = document.elementFromPoint(4, Math.floor(window.innerHeight / 2));
    return {
      viewport: { width: window.innerWidth, height: window.innerHeight },
      bodyScrollWidth: document.body.scrollWidth,
      rootScrollWidth: document.documentElement.scrollWidth,
      drawer: rect(".agent-run-drawer"),
      header: rect(".agent-run-drawer-header"),
      body: rect(".agent-run-detail"),
      back: rect(".agent-run-drawer-back"),
      close: rect(".agent-run-drawer-close"),
      rail: rect(".agent-rail"),
      stage: rect(".agent-chat-stage"),
      drawerOwnsMobileSample: Boolean(drawer && sample && drawer.contains(sample)),
    };
  });
}

function assertAgentReplayGeometry(geometry, device) {
  const tolerance = 2;
  const { drawer, header, body, back, close, rail, stage, viewport } = geometry;
  assertReplay(Boolean(drawer && header && body && back && close && rail && stage), "layout elements are missing", geometry);
  assertReplay(geometry.bodyScrollWidth <= viewport.width + 1, "body has horizontal overflow", geometry);
  assertReplay(geometry.rootScrollWidth <= viewport.width + 1, "document has horizontal overflow", geometry);
  for (const [label, box] of [["drawer", drawer], ["header", header], ["detail body", body], ["back", back], ["close", close]]) {
    assertBoxInViewport(box, device, `${device.name} ${label}`);
  }
  assertReplay(body.y >= header.bottom - tolerance, "drawer header overlaps the scroll body", geometry);
  assertReplay(body.bottom <= drawer.bottom + tolerance, "drawer scroll body is clipped", geometry);
  assertReplay(Math.abs(header.x - drawer.x) <= tolerance && Math.abs(header.right - drawer.right) <= tolerance, "header width does not match drawer", geometry);
  assertReplay(Math.abs(body.x - drawer.x) <= tolerance && Math.abs(body.right - drawer.right) <= tolerance, "scroll body width does not match drawer", geometry);
  assertReplay(["auto", "scroll"].includes(body.overflowY), "detail body is not scrollable", geometry);

  if (device.name === "desktop-1440") {
    assertReplay(drawer.position === "absolute", "desktop drawer is not an overlay", geometry);
    assertReplay(drawer.width >= 440 - tolerance && drawer.width <= 640 + tolerance, "desktop drawer width is outside clamp", geometry);
    assertReplay(Math.abs(drawer.right - stage.right) <= tolerance, "desktop drawer is not aligned to the chat stage", geometry);
    assertReplay(rail.right <= drawer.x + tolerance, "conversation rail overlaps the desktop drawer", geometry);
    return "overlay:" + Math.round(drawer.width) + "px";
  }

  assertReplay(drawer.position === "fixed", "mobile drawer is not fixed", geometry);
  assertReplay(
    Math.abs(drawer.x) <= tolerance
      && Math.abs(drawer.y) <= tolerance
      && Math.abs(drawer.width - viewport.width) <= tolerance
      && Math.abs(drawer.height - viewport.height) <= tolerance,
    "mobile drawer is not a full-screen sheet",
    geometry,
  );
  assertReplay(geometry.drawerOwnsMobileSample, "conversation rail covers the mobile drawer", geometry);
  return "sheet:" + Math.round(drawer.width) + "x" + Math.round(drawer.height);
}

async function runAgentReplayChecks(browser) {
  const replayDevices = devices.filter((device) => ["desktop-1440", "tablet-768", "phone-390"].includes(device.name));
  const reports = [];

  for (const device of replayDevices) {
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: device.mobile,
      isMobile: device.mobile,
      colorScheme: "dark",
    });
    const page = await context.newPage();
    const consoleIssues = [];
    const requests = { list: [], detail: [] };
    page.on("console", (message) => {
      if (message.type() === "error") consoleIssues.push("console: " + message.text());
    });
    page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));

    try {
      await installHarnessState(page);
      await installAgentReplayState(page);
      await installAgentReplayRoutes(page, requests);
      await page.goto(deviceUrl(device, "#sectionAgent"), { waitUntil: "networkidle" });
      await page.locator(".agent-workspace").waitFor({ state: "visible" });

      const historyTrigger = page.locator(device.mobile ? ".agent-mobile-history" : ".agent-thread-history");
      assertReplay(await historyTrigger.isVisible(), `${device.name} history trigger is not visible`);
      assertBoxInViewport(await historyTrigger.boundingBox(), device, `${device.name} history trigger`);

      const currentRequest = page.waitForRequest((request) => {
        const url = new URL(request.url());
        return url.pathname === "/api/agent/runs" && url.searchParams.get("conversation_id") === replayConversationId;
      });
      await historyTrigger.click();
      await currentRequest;

      const dialog = page.getByRole("dialog", { name: "Agent 运行复盘" });
      await dialog.waitFor({ state: "visible" });
      const runRow = dialog.locator(".agent-run-select");
      await runRow.waitFor({ state: "visible" });
      const currentScope = dialog.getByRole("button", { name: "当前会话" });
      const allScope = dialog.getByRole("button", { name: "全部运行" });
      assertBoxInViewport(await currentScope.boundingBox(), device, `${device.name} current scope`);
      assertBoxInViewport(await allScope.boundingBox(), device, `${device.name} all scope`);
      assertReplay(requests.list[0]?.conversationId === replayConversationId, "current scope omitted conversation_id", requests.list);

      const allRequest = page.waitForRequest((request) => {
        const url = new URL(request.url());
        return url.pathname === "/api/agent/runs" && !url.searchParams.has("conversation_id");
      });
      await allScope.click();
      await allRequest;
      await runRow.waitFor({ state: "visible" });
      assertReplay(requests.list.some((request) => request.conversationId === null), "all scope retained conversation_id", requests.list);

      const detailRequest = page.waitForRequest((request) => new URL(request.url()).pathname === `/api/agent/runs/${replayRunId}`);
      await runRow.click();
      await detailRequest;
      const detail = dialog.locator(".agent-run-detail");
      await detail.waitFor({ state: "visible" });
      await dialog.locator(".agent-run-overview").getByText(mockAgentRunSummary.question).waitFor();
      assertReplay(await dialog.locator(".agent-run-timeline-item").count() >= 4, "timeline events were not rendered");
      await dialog.locator(".agent-answer-sections").getByText("复盘结论").waitFor();
      await dialog.locator(".agent-warning-list").getByText("仅用于浏览器门禁验证。").waitFor();
      await dialog.locator(".agent-run-result").waitFor({ state: "visible" });

      const backButton = dialog.getByRole("button", { name: "返回运行列表" });
      await page.waitForFunction(() => document.activeElement?.classList.contains("agent-run-drawer-back"));
      assertReplay(await backButton.evaluate((element) => document.activeElement === element), "focus left the drawer after list-to-detail navigation");
      const layout = assertAgentReplayGeometry(await replayGeometry(page), device);

      await page.keyboard.press("Escape");
      await dialog.waitFor({ state: "hidden" });
      assertReplay(await historyTrigger.evaluate((element) => document.activeElement === element), "Escape did not restore focus to the history trigger");

      const directTrigger = page.locator(".agent-message-replay");
      assertReplay(await directTrigger.isVisible(), `${device.name} direct replay trigger is not visible`);
      assertBoxInViewport(await directTrigger.boundingBox(), device, `${device.name} direct replay trigger`);
      const directRequest = page.waitForRequest((request) => new URL(request.url()).pathname === `/api/agent/runs/${replayRunId}`);
      await directTrigger.click();
      await directRequest;
      await dialog.locator(".agent-run-detail").waitFor({ state: "visible" });
      await dialog.locator(".agent-run-overview").getByText(mockAgentRunSummary.question).waitFor();
      assertReplay(requests.detail.length === 2, "direct replay did not load the exact detail", requests.detail);
      await page.keyboard.press("Escape");
      await dialog.waitFor({ state: "hidden" });
      assertReplay(await directTrigger.evaluate((element) => document.activeElement === element), "direct replay did not restore focus");
      assertReplay(consoleIssues.length === 0, `${device.name} logged browser errors`, consoleIssues);

      reports.push({
        device: device.name,
        layout,
        currentRequests: requests.list.filter((request) => request.conversationId === replayConversationId).length,
        allRequests: requests.list.filter((request) => request.conversationId === null).length,
        detailRequests: requests.detail.length,
      });
    } finally {
      await context.close();
    }
  }

  console.log("Agent replay checks passed: " + reports.map((report) => (
    `${report.device}[${report.layout},current=${report.currentRequests},all=${report.allRequests},detail=${report.detailRequests}]`
  )).join(" | "));
}

async function captureDenseStates(page, device, targetRoot) {
  const directory = resolve(targetRoot, "desktop-1440-data");
  mkdirSync(directory, { recursive: true });

  await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
  await page.locator(".header-search input").fill("");
  await page.locator(".screen-panel-container .run-btn").click();
  await page.locator(".stock-row").first().waitFor();
  await page.screenshot({ path: resolve(directory, "screen-dense.png"), fullPage: false });

  await page.goto(deviceUrl(device, "#sectionObserve"), { waitUntil: "networkidle" });
  await page.locator("#observeCode").fill("600519.SH");
  await page.locator(".observe-run-btn").click();
  await page.locator(".observe-result").waitFor();
  await page.screenshot({ path: resolve(directory, "observe-dense.png"), fullPage: false });

  await page.goto(deviceUrl(device, "#sectionBacktest"), { waitUntil: "networkidle" });
  await page.getByLabel("运行回测").click();
  await page.locator(".backtest-result").waitFor();
  await page.screenshot({ path: resolve(directory, "backtest-dense.png"), fullPage: false });

  return ["screen-dense.png", "observe-dense.png", "backtest-dense.png"];
}

let localServer = null;
if (serveBuild) {
  const started = await startBuiltAppServer();
  localServer = started.server;
  baseUrl = started.url;
}

const browser = await chromium.launch();
try {
  if (checkOnly || checkSearchOverlay) {
    const device = devices.find((item) => item.name === "desktop-1440");
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      colorScheme: "dark",
    });
    const page = await context.newPage();
    const consoleIssues = [];
    page.on("console", (message) => {
      if (message.type() === "error") consoleIssues.push(message.text());
    });
    page.on("pageerror", (error) => consoleIssues.push(error.message));
    await installHarnessState(page);
    const shortcutChecks = await runShortcutChecks(page, device);
    const searchOverlayChecks = checkSearchOverlay ? await runSearchOverlayChecks(page, device) : null;
    if (checkSearchOverlay) console.log('Desktop search overlay checks passed: ' + JSON.stringify(searchOverlayChecks));
    const diagnostics = await pageDiagnostics(page, "shortcuts");
    assertPageDiagnostics(diagnostics, device.name);
    if (consoleIssues.length) {
      throw new Error("Desktop shortcut checks logged errors: " + consoleIssues.join(" | "));
    }
    console.log("Desktop shortcut checks passed: " + JSON.stringify(shortcutChecks));
    await context.close();
    await runAgentReplayChecks(browser);
  } else if (headerSettingsOnly) {
    mkdirSync(outputRoot, { recursive: true });
    const headerBaselines = await captureHeaderBaselines(browser, outputRoot);
    writeFileSync(
      resolve(outputRoot, "header-settings-report.json"),
      JSON.stringify({ baseUrl, generatedAt: new Date().toISOString(), headerBaselines }, null, 2) + "\n",
    );
    console.log("Header/settings screenshots written to " + outputRoot);
  } else {
    mkdirSync(outputRoot, { recursive: true });
    const report = {
      baseUrl,
      generatedAt: new Date().toISOString(),
      devices: [],
      denseStates: [],
      headerBaselines: [],
    };

    for (const device of devices) {
      const consoleIssues = [];
      const context = await browser.newContext({
        viewport: { width: device.width, height: device.height },
        screen: { width: device.width, height: device.height },
        deviceScaleFactor: device.dpr,
        hasTouch: device.mobile,
        isMobile: device.mobile,
        colorScheme: device.theme,
      });
      const page = await context.newPage();
      await installHarnessState(page);
      page.on("console", (message) => {
        if (message.type() === "warning" || message.type() === "error") {
          consoleIssues.push(message.type() + ": " + message.text());
        }
      });
      page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));
      const deviceReport = { device, routes: [], interaction: null, shortcuts: null, consoleIssues };

      for (const route of routes) {
        await page.goto(deviceUrl(device, route.hash), { waitUntil: "networkidle" });
        await page.locator("#root").waitFor({ state: "visible" });
        if (route.ready) await page.locator(route.ready).waitFor({ state: "visible" });
        if (route.name === "agent" && device.mobile) {
          await page.locator(".agent-workspace.rail-collapsed").waitFor();
          await page.waitForTimeout(260);
        }
        await page.mouse.move(device.width / 2, Math.min(180, device.height / 3));
        await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
        if (route.prepare) await route.prepare(page);
        const directory = resolve(outputRoot, device.name);
        mkdirSync(directory, { recursive: true });
        await page.screenshot({
          path: resolve(directory, route.name + ".png"),
          fullPage: false,
        });
        const diagnostics = await pageDiagnostics(page, route.name);
        assertPageDiagnostics(diagnostics, device.name);
        deviceReport.routes.push({
          ...diagnostics,
          sidebar: await boxFor(page, ".sidebar"),
          workspace: await boxFor(page, ".agent-workspace"),
          rail: await boxFor(page, ".agent-rail"),
          stage: await boxFor(page, ".agent-chat-stage"),
          settings: await boxFor(page, ".llm-settings-body"),
        });
      }

      await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
      const secondScreenTab = page.locator(".screen-panel-tabs .panel-tab").nth(1);
      await secondScreenTab.click();
      deviceReport.interaction = {
        action: "activate second screen tab",
        activeLabel: await page.locator(".screen-panel-tabs .panel-tab.active").innerText(),
      };

      if (device.name === "desktop-1440") {
        deviceReport.shortcuts = await runShortcutChecks(page, device);
        report.denseStates = await captureDenseStates(page, device, outputRoot);
      }

      report.devices.push(deviceReport);
      await context.close();
    }

    report.headerBaselines = await captureHeaderBaselines(browser, outputRoot);

    writeFileSync(resolve(outputRoot, "report.json"), JSON.stringify(report, null, 2) + "\n");
    console.log("UI screenshots written to " + outputRoot);
  }
} finally {
  await browser.close();
  if (localServer) {
    await new Promise((resolveClose) => localServer.close(resolveClose));
  }
}
