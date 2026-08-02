#!/usr/bin/env node

import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const { chromium } = frontendRequire("playwright");

const option = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const baseUrl = option("--url", "http://127.0.0.1:4173").replace(/\/$/, "");
const date = new Date().toISOString().slice(0, 10);
const defaultOutput = fileURLToPath(
  new URL(`../artifacts/ui-shots/${date}/`, import.meta.url),
);
const outputRoot = resolve(option("--output", defaultOutput));
const devices = [
  { name: "phone-360", width: 360, height: 780, dpr: 3, mobile: true },
  { name: "phone-390", width: 390, height: 844, dpr: 3, mobile: true },
  { name: "phone-430", width: 430, height: 932, dpr: 3, mobile: true },
  { name: "tablet-768", width: 768, height: 1024, dpr: 2, mobile: true },
  { name: "desktop-1440", width: 1440, height: 900, dpr: 1, mobile: false },
];
const routes = [
  { name: "screen", hash: "#sectionScreen" },
  { name: "observe", hash: "#sectionObserve" },
  { name: "backtest", hash: "#sectionBacktest" },
  { name: "news", hash: "#sectionNewsRag" },
  { name: "agent", hash: "#sectionAgent" },
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

mkdirSync(outputRoot, { recursive: true });
const browser = await chromium.launch();
const report = { baseUrl, generatedAt: new Date().toISOString(), devices: [] };

try {
  for (const device of devices) {
    const consoleIssues = [];
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: device.mobile,
      isMobile: device.mobile,
      colorScheme: "dark",
    });
    const page = await context.newPage();
    page.on("console", (message) => {
      if (message.type() === "warning" || message.type() === "error") {
        consoleIssues.push(message.type() + ": " + message.text());
      }
    });
    page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));
    const deviceReport = { device, routes: [], interaction: null, consoleIssues };

    for (const route of routes) {
      await page.goto(`${baseUrl}/${route.hash}`, { waitUntil: "networkidle" });
      await page.locator("#root").waitFor({ state: "visible" });
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
        path: resolve(directory, `${route.name}.png`),
        fullPage: false,
      });
      deviceReport.routes.push({
        ...await pageDiagnostics(page, route.name),
        sidebar: await boxFor(page, ".sidebar"),
        workspace: await boxFor(page, ".agent-workspace"),
        rail: await boxFor(page, ".agent-rail"),
        stage: await boxFor(page, ".agent-chat-stage"),
        settings: await boxFor(page, ".llm-settings-body"),
      });
    }

    await page.goto(baseUrl + "/#sectionScreen", { waitUntil: "networkidle" });
    const secondScreenTab = page.locator(".screen-panel-tabs .panel-tab").nth(1);
    await secondScreenTab.click();
    deviceReport.interaction = {
      action: "activate second screen tab",
      activeLabel: await page.locator(".screen-panel-tabs .panel-tab.active").innerText(),
    };

    report.devices.push(deviceReport);
    await context.close();
  }
} finally {
  await browser.close();
}

writeFileSync(resolve(outputRoot, "report.json"), JSON.stringify(report, null, 2) + "\n");
console.log(`UI screenshots written to ${outputRoot}`);
