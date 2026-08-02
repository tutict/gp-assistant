#!/usr/bin/env node

import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const stylesDirectory = fileURLToPath(
  new URL("../desktop/frontend/src/styles/", import.meta.url),
);
const styleFiles = readdirSync(stylesDirectory)
  .filter((file) => file.endsWith(".css"))
  .sort();
const errors = [];
const minimumFont = 10;
const minimumTouch = 30;
const platformClass =
  /\.(?:android-(?:phone|tablet|compact|bottom-nav|landscape|portrait)|mobile-tauri)\b/;
const interactiveSelector =
  /(?:\bbutton\b|\binput\b|\bselect\b|\btextarea\b|\bsummary\b|\[role\s*=\s*["']?(?:button|tab)|\.(?:icon-button|[\w-]*(?:btn|button|action|toggle|tab|close|remove|clear|collapse))\b)/;

function lineNumber(source, index) {
  return source.slice(0, index).split("\n").length;
}

function addError(file, source, index, message) {
  errors.push(`${file}:${lineNumber(source, index)} ${message}`);
}

for (const file of styleFiles) {
  const source = readFileSync(`${stylesDirectory}/${file}`, "utf8");
  const css = source.replace(
    /\/\*[\s\S]*?\*\//g,
    (comment) => comment.replace(/[^\n]/g, " "),
  );

  for (const match of css.matchAll(new RegExp(platformClass, "g"))) {
    addError(file, css, match.index, `platform density selector is forbidden: ${match[0]}`);
  }

  for (const match of css.matchAll(/(?:-webkit-)?text-size-adjust\s*:\s*([^;}]+)/g)) {
    if (match[1].trim() !== "100%") {
      addError(file, css, match.index, "text-size-adjust must be 100%");
    }
  }

  for (const match of css.matchAll(/(?:^|})\s*html\s*\{[^}]*\bfont-size\s*:/gs)) {
    addError(file, css, match.index, "html font-size must remain at the browser default");
  }

  for (const match of css.matchAll(/font-size:\s*([\d.]+)px/g)) {
    if (Number(match[1]) < minimumFont) {
      addError(file, css, match.index, `font size ${match[1]}px is below ${minimumFont}px`);
    }
  }

  for (const rule of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = rule[1].trim();
    if (!interactiveSelector.test(selector)) continue;
    for (const height of rule[2].matchAll(/min-height:\s*([\d.]+)px/g)) {
      if (Number(height[1]) < minimumTouch) {
        addError(
          file,
          css,
          rule.index,
          `interactive min-height ${height[1]}px is below ${minimumTouch}px in ${selector}`,
        );
      }
    }
  }

  for (const rule of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (!/\.(?:run-btn|save-btn|action-btn)\b/.test(rule[1])) continue;
    for (const shadow of rule[2].matchAll(/box-shadow:\s*([^;}]+)/g)) {
      if (shadow[1].trim() !== "none") {
        addError(file, css, rule.index, "primary actions must remain flat at rest");
      }
    }
  }
}

const responsive = readFileSync(`${stylesDirectory}/responsive.css`, "utf8");
if (responsive.split(/\r?\n/).length > 1200) {
  errors.push("responsive.css exceeds the 1200-line maintenance limit");
}

const tokens = readFileSync(`${stylesDirectory}/tokens.css`, "utf8");
for (const token of [
  "--fs-body: 14px",
  "--fs-data: 13px",
  "--fs-label: 12px",
  "--fs-caption: 11px",
  "--touch-comfort: 44px",
  "--touch-dense: 32px",
  "--nav-height: 60px",
]) {
  if (!tokens.includes(token)) errors.push(`tokens.css is missing ${token}`);
}

if (errors.length > 0) {
  console.error(`UI density guard failed:\n${errors.join("\n")}`);
  process.exit(1);
}

console.log(`UI density guard passed (${styleFiles.length} stylesheets checked)`);
