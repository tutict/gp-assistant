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
  if (checkOnly) {
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
    const diagnostics = await pageDiagnostics(page, "shortcuts");
    assertPageDiagnostics(diagnostics, device.name);
    if (consoleIssues.length) {
      throw new Error("Desktop shortcut checks logged errors: " + consoleIssues.join(" | "));
    }
    console.log("Desktop shortcut checks passed: " + JSON.stringify(shortcutChecks));
    await context.close();
  } else {
    mkdirSync(outputRoot, { recursive: true });
    const report = { baseUrl, generatedAt: new Date().toISOString(), devices: [], denseStates: [] };

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

    writeFileSync(resolve(outputRoot, "report.json"), JSON.stringify(report, null, 2) + "\n");
    console.log("UI screenshots written to " + outputRoot);
  }
} finally {
  await browser.close();
  if (localServer) {
    await new Promise((resolveClose) => localServer.close(resolveClose));
  }
}
