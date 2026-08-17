#!/usr/bin/env node

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../desktop/frontend/src/", import.meta.url));
const stylesDirectory = join(root, "styles");
const whitelist = new Set([
  "active",
  "archived",
  "assistant",
  "collapsed",
  "current",
  "dialog",
  "empty",
  "error",
  "fall",
  "fresh",
  "hidden",
  "loading",
  "muted",
  "negative",
  "neutral",
  "open",
  "positive",
  "proxy",
  "ready",
  "refreshing",
  "rise",
  "selected",
  "spin",
  "success",
  "toggle",
  "unavailable",
  "user",
  "warn",
  "warning",
]);
const legacyUnstyled = new Set([
  "agent-history-empty",
  "agent-history-group",
  "agent-history-group-label",
  "agent-structured-result",
  "compact-selection-explain",
  "empty-list",
  "indicator-cross-layer",
  "institution",
  "kdj-guide-line",
  "kline-axis-label",
  "kline-empty-state",
  "kline-grid-line",
  "kline-inspector",
  "kline-kdj-layer",
  "kline-line-panel",
  "kline-macd-layer",
  "kline-plot-bg",
  "kline-section-border",
  "kline-subpanel-bg",
  "kline-subpanel-title",
  "kline-volume-layer",
  "left",
  "llm-api-key-field",
  "llm-endpoint-field",
  "llm-settings-header",
  "observe-action",
  "observe-panel-container",
  "observe-panel-result",
  "quote-eps",
  "quote-pb",
  "quote-pe",
  "raw-json-hint",
  "refresh-option",
  "refresh-option-boolean",
  "refresh-option-label",
  "right",
  "screen-mobile-tools-toggle",
  "screen-result-actions",
]);

const cssClasses = new Set();
for (const file of readdirSync(stylesDirectory).filter((item) => item.endsWith(".css"))) {
  const css = readFileSync(join(stylesDirectory, file), "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "");
  for (const match of css.matchAll(/\.(-?[_a-zA-Z]+[_a-zA-Z0-9-]*)/g)) {
    cssClasses.add(match[1]);
  }
}

function* walk(directory) {
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      yield* walk(path);
    } else if (path.endsWith(".tsx") && !path.endsWith(".test.tsx")) {
      yield path;
    }
  }
}

function classTokens(value) {
  if (/[${}`]/.test(value)) return [];
  return value
    .split(/\s+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function collectStaticClassNames(source) {
  const classes = [];
  for (const match of source.matchAll(/className\s*=\s*"([^"]+)"/g)) {
    classes.push(...classTokens(match[1]));
  }
  for (const match of source.matchAll(/className\s*=\s*\{\s*["']([^"']+)["']\s*\}/g)) {
    classes.push(...classTokens(match[1]));
  }
  return classes;
}

const unstyled = new Map();
for (const file of walk(root)) {
  const source = readFileSync(file, "utf8");
  for (const className of collectStaticClassNames(source)) {
    if (whitelist.has(className) || legacyUnstyled.has(className) || cssClasses.has(className)) continue;
    if (!unstyled.has(className)) {
      unstyled.set(className, relative(root, file).replace(/\\/g, "/"));
    }
  }
}

if (unstyled.size > 0) {
  console.error("Unstyled classes found in static className strings:");
  for (const [className, file] of [...unstyled].sort(([left], [right]) => left.localeCompare(right))) {
    console.error(`  .${className} <- desktop/frontend/src/${file}`);
  }
  process.exit(1);
}

console.log(`Unstyled class guard passed (${cssClasses.size} CSS classes checked)`);
