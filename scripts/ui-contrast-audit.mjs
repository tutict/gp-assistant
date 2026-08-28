#!/usr/bin/env node

import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const defaultUrl = "http://127.0.0.1:4173";
const auditRoutes = [
  { name: "screen", hash: "#sectionScreen", ready: ".screen-panel-container" },
  { name: "observe", hash: "#sectionObserve", ready: ".observe-panel-container" },
  { name: "backtest", hash: "#sectionBacktest", ready: ".backtest-context" },
  { name: "news", hash: "#sectionNewsRag", ready: ".research-workspace" },
  { name: "agent", hash: "#sectionAgent", ready: ".agent-workspace" },
  { name: "settings", hash: "#sectionAgent", ready: ".agent-workspace", settings: true },
];
const auditDevices = [
  { name: "desktop-1440", width: 1440, height: 900, dpr: 1, mobile: false },
  { name: "phone-390", width: 390, height: 844, dpr: 3, mobile: true },
];

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

export function parseCssColor(value) {
  if (value.trim().toLowerCase() === "transparent") return { r: 0, g: 0, b: 0, a: 0 };
  const numbers = value.match(/-?\d*\.?\d+/g)?.map(Number) || [];
  if (numbers.length < 3) throw new Error(`Unsupported computed color: ${value}`);
  const srgbScale = /^color\(srgb\b/i.test(value) ? 255 : 1;
  return {
    r: clamp(numbers[0] * srgbScale, 0, 255),
    g: clamp(numbers[1] * srgbScale, 0, 255),
    b: clamp(numbers[2] * srgbScale, 0, 255),
    a: clamp(numbers[3] ?? 1, 0, 1),
  };
}

export function compositeColor(foreground, background) {
  const alpha = foreground.a + background.a * (1 - foreground.a);
  if (alpha === 0) return { r: 0, g: 0, b: 0, a: 0 };
  return {
    r: (foreground.r * foreground.a + background.r * background.a * (1 - foreground.a)) / alpha,
    g: (foreground.g * foreground.a + background.g * background.a * (1 - foreground.a)) / alpha,
    b: (foreground.b * foreground.a + background.b * background.a * (1 - foreground.a)) / alpha,
    a: alpha,
  };
}

function relativeLuminance(color) {
  const channel = (value) => {
    const normalized = value / 255;
    return normalized <= 0.04045
      ? normalized / 12.92
      : ((normalized + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b);
}

export function contrastRatio(foreground, background) {
  const lighter = Math.max(relativeLuminance(foreground), relativeLuminance(background));
  const darker = Math.min(relativeLuminance(foreground), relativeLuminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

export function requiredContrast({ fontSize, fontWeight }) {
  return fontSize >= 18 || (fontSize >= 14 && fontWeight >= 700) ? 3 : 4.5;
}

function gradientSamples(value) {
  const stops = (value.match(/rgba?\([^)]*\)/gi) || []).map(parseCssColor);
  const samples = [...stops];
  for (let index = 0; index < stops.length - 1; index += 1) {
    for (const progress of [0.25, 0.5, 0.75]) {
      samples.push({
        r: stops[index].r + (stops[index + 1].r - stops[index].r) * progress,
        g: stops[index].g + (stops[index + 1].g - stops[index].g) * progress,
        b: stops[index].b + (stops[index + 1].b - stops[index].b) * progress,
        a: stops[index].a + (stops[index + 1].a - stops[index].a) * progress,
      });
    }
  }
  return samples;
}

function effectiveBackgrounds(layers, theme) {
  let backgrounds = [theme === "light"
    ? { r: 255, g: 255, b: 255, a: 1 }
    : { r: 0, g: 0, b: 0, a: 1 }];
  for (const rawLayer of [...layers].reverse()) {
    const layer = typeof rawLayer === "string"
      ? { color: rawLayer, image: "none" }
      : rawLayer;
    const solidBackgrounds = backgrounds.map(
      (background) => compositeColor(parseCssColor(layer.color), background),
    );
    const imageSamples = layer.image === "none" ? [] : gradientSamples(layer.image);
    backgrounds = imageSamples.length === 0
      ? solidBackgrounds
      : solidBackgrounds.flatMap((background) =>
        imageSamples.map((sample) => compositeColor(sample, background)));
  }
  return backgrounds;
}

export function evaluateContrastRecord(record, theme) {
  const parsedForeground = parseCssColor(record.color);
  const candidates = effectiveBackgrounds(record.backgroundLayers, theme).map((background) => {
    const foreground = compositeColor(
      { ...parsedForeground, a: parsedForeground.a * record.opacity },
      background,
    );
    return { foreground, background, ratio: contrastRatio(foreground, background) };
  });
  const worst = candidates.reduce((lowest, candidate) =>
    candidate.ratio < lowest.ratio ? candidate : lowest);
  const required = requiredContrast(record);
  return { ...record, ...worst, required };
}

function auditUrl(baseUrl, theme, hash) {
  const query = theme === "light" ? "?theme=light" : "";
  return `${baseUrl}/${query}${hash}`;
}

async function prepareSettings(page, mobile) {
  if (mobile) {
    const menu = page.locator(".agent-mobile-menu");
    if (await menu.isVisible()) {
      await menu.click();
      await page.locator(".agent-workspace:not(.rail-collapsed)").waitFor();
    }
  }
  const toggle = page.locator(".llm-settings-toggle");
  await toggle.scrollIntoViewIfNeeded();
  await toggle.click();
  await page.locator(".llm-settings-body").waitFor({ state: "visible" });
}

async function collectTextRecords(page) {
  return page.evaluate(() => {
    const selectorFor = (element) => {
      if (element.id) return `#${CSS.escape(element.id)}`;
      const segments = [];
      let current = element;
      while (current && current !== document.body && segments.length < 5) {
        let segment = current.tagName.toLowerCase();
        const classes = [...current.classList].filter((name) => !/^(?:active|selected|open|ready|unread)$/.test(name));
        if (classes.length) segment += `.${classes.slice(0, 2).map((name) => CSS.escape(name)).join(".")}`;
        const parent = current.parentElement;
        if (parent && parent.querySelectorAll(`:scope > ${current.tagName}`).length > 1) {
          segment += `:nth-of-type(${[...parent.children].filter((child) => child.tagName === current.tagName).indexOf(current) + 1})`;
        }
        segments.unshift(segment);
        current = parent;
      }
      return segments.join(" > ");
    };
    const visible = (element) => {
      const style = getComputedStyle(element);
      if (style.display === "none" || style.visibility === "hidden" || Number(style.opacity) === 0) return false;
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0
        && rect.right > 0 && rect.bottom > 0
        && rect.left < window.innerWidth && rect.top < window.innerHeight;
    };
    const elements = new Map();
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      const text = walker.currentNode.textContent?.replace(/\s+/g, " ").trim() || "";
      const element = walker.currentNode.parentElement;
      if (!text || !element || !visible(element)) continue;
      elements.set(element, `${elements.get(element) || ""} ${text}`.trim());
    }
    for (const control of document.querySelectorAll(
      'input:not([type]), input[type="text"], input[type="search"], input[type="email"], '
        + 'input[type="url"], input[type="tel"], input[type="number"], textarea, select',
    )) {
      if (!visible(control)) continue;
      const text = control.value || control.getAttribute("placeholder") || "";
      if (text.trim()) {
        const placeholder = !control.value && Boolean(control.getAttribute("placeholder"));
        elements.set(control, { text: text.trim(), placeholder });
      }
    }
    return [...elements].map(([element, entry]) => {
      const text = typeof entry === "string" ? entry : entry.text;
      const placeholder = typeof entry === "string" ? false : entry.placeholder;
      const style = getComputedStyle(element, placeholder ? "::placeholder" : null);
      const backgroundLayers = [];
      let opacity = 1;
      let current = element;
      while (current) {
        const currentStyle = getComputedStyle(current);
        backgroundLayers.push({
          color: currentStyle.backgroundColor,
          image: currentStyle.backgroundImage,
        });
        opacity *= Number(currentStyle.opacity);
        current = current.parentElement;
      }
      return {
        selector: selectorFor(element),
        text: text.slice(0, 20),
        color: style.color,
        backgroundLayers,
        opacity,
        fontSize: Number.parseFloat(style.fontSize),
        fontWeight: Number.parseInt(style.fontWeight, 10) || 400,
      };
    });
  });
}

export async function runContrastAudit({ baseUrl = defaultUrl, serve = false, fail = false } = {}) {
  const frontendRequire = createRequire(new URL("../desktop/frontend/package.json", import.meta.url));
  const { chromium } = frontendRequire("playwright");
  const { installHarnessState, startBuiltAppServer } = await import("./ui-screenshot.mjs");
  let server = null;
  if (serve) {
    const started = await startBuiltAppServer();
    server = started.server;
    baseUrl = started.url;
  }
  const violations = [];
  const browser = await chromium.launch();
  try {
    for (const device of auditDevices) {
      for (const theme of ["dark", "light"]) {
        const context = await browser.newContext({
          viewport: { width: device.width, height: device.height },
          screen: { width: device.width, height: device.height },
          deviceScaleFactor: device.dpr,
          hasTouch: device.mobile,
          isMobile: device.mobile,
          colorScheme: theme,
        });
        const page = await context.newPage();
        await installHarnessState(page);
        for (const route of auditRoutes) {
          await page.goto(auditUrl(baseUrl, theme, route.hash), { waitUntil: "networkidle" });
          await page.locator(route.ready).waitFor({ state: "visible" });
          if (route.settings) await prepareSettings(page, device.mobile);
          const records = await collectTextRecords(page);
          for (const record of records) {
            const result = evaluateContrastRecord(record, theme);
            if (result.ratio + 0.005 < result.required) {
              violations.push({
                route: route.name,
                theme,
                device: device.name,
                selector: result.selector,
                text: result.text,
                ratio: Number(result.ratio.toFixed(2)),
                required: result.required,
              });
            }
          }
        }
        await context.close();
      }
    }
  } finally {
    await browser.close();
    if (server) await new Promise((resolveClose) => server.close(resolveClose));
  }

  if (violations.length > 0) {
    const lines = violations.map((item) =>
      `${item.route}/${item.theme}/${item.device} ${item.ratio}:1 < ${item.required}:1 `
      + `${item.selector} "${item.text}"`);
    const message = `UI contrast audit found ${violations.length} violation(s):\n${lines.join("\n")}`;
    (fail ? console.error : console.warn)(message);
  } else {
    console.log("UI contrast audit passed (24 route/theme/viewport combinations)");
  }
  return { violations, exitCode: fail && violations.length > 0 ? 1 : 0 };
}

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
}

const isDirect = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isDirect) {
  const result = await runContrastAudit({
    baseUrl: option("--url", defaultUrl).replace(/\/$/, ""),
    serve: process.argv.includes("--serve"),
    fail: process.argv.includes("--fail"),
  });
  process.exitCode = result.exitCode;
}
