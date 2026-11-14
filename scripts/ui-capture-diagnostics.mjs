import { mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import {
  alignBacktestResultScreenshot, assertPageDiagnostics, comparePngFiles, disableResearchMotion,
  installFixedResearchClock, installHarnessState, pageDiagnostics, startBuiltAppServer,
} from "./ui-screenshot.mjs";

const frontendRequire = createRequire(new URL("../desktop/frontend/package.json", import.meta.url));
const { chromium } = frontendRequire("playwright");
const option = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const method = option("--method", "wrapper");
if (!["wrapper", "metrics-wrapper", "cdp", "fonts-ready", "full-chromium", "full-chromium-zh"].includes(method)) {
  throw new Error(`Unknown diagnostic method: ${method}`);
}
const output = resolve(option("--output", "artifacts/ui-capture-diagnostics"));
mkdirSync(output, { recursive: true });
const baseline = resolve("desktop/frontend/test-baselines/backtest-page/desktop-1920-dark/backtest.png");
const readState = (page) => page.evaluate(() => ({
  scroll: { x: scrollX, y: scrollY },
  viewport: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio },
  outer: { width: outerWidth, height: outerHeight },
  screen: { width: screen.width, height: screen.height, availWidth: screen.availWidth, availHeight: screen.availHeight },
  document: { width: document.documentElement.scrollWidth, height: document.documentElement.scrollHeight },
  focusedElement: document.activeElement?.className || document.activeElement?.tagName,
  fonts: document.fonts.status,
}));
async function windowBounds(browser, context, page) {
  const pageSession = await context.newCDPSession(page);
  const rootSession = await browser.newBrowserCDPSession();
  try {
    const { targetInfo } = await pageSession.send("Target.getTargetInfo");
    const window = await rootSession.send("Browser.getWindowForTarget", { targetId: targetInfo.targetId });
    return { window, bounds: await rootSession.send("Browser.getWindowBounds", { windowId: window.windowId }) };
  } catch (error) {
    return { unavailable: String(error) };
  } finally {
    await pageSession.detach();
    await rootSession.detach();
  }
}
const { server, url } = await startBuiltAppServer();
const launchOptions = method.startsWith("full-chromium") ? { channel: "chromium" } : {};
const report = { method, node: process.version, viewportEmulated: true, launchOptions };
let browser;
try {
  browser = await chromium.launch(launchOptions);
  report.browser = browser.version();
  const context = await browser.newContext({
    viewport: { width: 1920, height: 1080 }, screen: { width: 1920, height: 1080 },
    deviceScaleFactor: 1, hasTouch: false, isMobile: false, colorScheme: "dark", reducedMotion: "reduce",
    ...(method === "full-chromium-zh" ? { locale: "zh-CN" } : {}),
  });
  const page = await context.newPage();
  await installFixedResearchClock(page);
  await installHarnessState(page);
  await page.goto(url + "/#sectionBacktest", { waitUntil: "networkidle" });
  await disableResearchMotion(page);
  const parameters = page.locator(".backtest-parameter-groups");
  await parameters.waitFor({ state: "visible" });
  report.parameters = {
    box: await parameters.boundingBox(),
    dates: await parameters.locator("input[type=date]").evaluateAll((inputs) => inputs.map((input) => input.value)),
  };
  const parameterScreenshot = resolve(output, "backtest-parameters.png");
  await parameters.screenshot({ path: parameterScreenshot });
  report.parameterComparison = comparePngFiles(parameterScreenshot, baseline.replace("backtest.png", "backtest-parameters.png"));
  await page.getByLabel("运行回测").click();
  await page.locator(".backtest-result").waitFor({ state: "visible" });
  await page.getByRole("tab", { name: "结果概览", exact: true }).click();
  await page.waitForTimeout(180);
  report.pageDiagnostics = await pageDiagnostics(page, "backtest-page");
  assertPageDiagnostics(report.pageDiagnostics, "desktop-1920-dark");
  await page.addStyleTag({ content: "@media (max-width: 768px) { .sidebar { visibility: hidden !important; } }" });
  if (method === "fonts-ready") await page.evaluate(() => document.fonts.ready);
  report.anchor = await alignBacktestResultScreenshot(page);
  report.initial = await readState(page);
  const pageSession = await context.newCDPSession(page);
  if (method === "metrics-wrapper" || method === "cdp") {
    report.layoutMetrics = await pageSession.send("Page.getLayoutMetrics");
    report.afterMetrics = await readState(page);
  }
  const screenshot = resolve(output, "backtest.png");
  if (method === "cdp") {
    const { width, height } = report.initial.document;
    const result = await pageSession.send("Page.captureScreenshot", {
      format: "png", clip: { x: 0, y: 0, width, height, scale: 1 }, captureBeyondViewport: true,
    });
    writeFileSync(screenshot, Buffer.from(result.data, "base64"));
  } else {
    await page.screenshot({ path: screenshot, fullPage: true });
  }
  report.afterCapture = await readState(page);
  report.window = await windowBounds(browser, context, page);
  report.comparison = comparePngFiles(screenshot, baseline);
  report.scrollUnchanged = Math.abs(report.initial.scroll.y - report.afterCapture.scroll.y) <= 1;
  await context.close();
  await browser.close();
  browser = await chromium.launch(launchOptions);
  const nativeContext = await browser.newContext({ viewport: null });
  const nativePage = await nativeContext.newPage();
  report.nativeProbe = {
    viewportEmulated: false, state: await readState(nativePage),
    window: await windowBounds(browser, nativeContext, nativePage),
  };
  await nativeContext.close();
  if (!report.parameterComparison.pass || !report.comparison.pass || !report.scrollUnchanged) process.exitCode = 1;
} catch (error) {
  report.error = String(error);
  process.exitCode = 1;
} finally {
  if (browser) await browser.close();
  await new Promise((done) => server.close(done));
  writeFileSync(resolve(output, "capture-diagnostics.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
}
