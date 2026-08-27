#!/usr/bin/env node

import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const stylesDirectory = fileURLToPath(
  new URL("../desktop/frontend/src/styles/", import.meta.url),
);
const styleFiles = readdirSync(stylesDirectory)
  .filter((file) => file.endsWith(".css"))
  .sort();
const errors = [];
const shellArgumentIndex = process.argv.indexOf("--shell");
const shellPath = shellArgumentIndex >= 0
  ? process.argv[shellArgumentIndex + 1]
  : `${stylesDirectory}/shell.css`;
const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const postcss = frontendRequire("postcss");
const shell = readFileSync(shellPath, "utf8");
const shellRoot = postcss.parse(shell, { from: "shell.css" });

function shellLine(node) {
  return node?.source?.start?.line ?? 1;
}

function shellError(node, message) {
  errors.push(`shell.css:${shellLine(node)} ${message}`);
}

function hasSelector(rule, selector) {
  return postcss.list
    .comma(rule.selector)
    .some((candidate) => candidate.replace(/\s+/g, " ").trim() === selector);
}

function hasDeclaration(rule, property, value) {
  return rule.nodes?.some(
    (node) => node.type === "decl" && node.prop === property && node.value.trim() === value,
  );
}

function isWithinMedia(rule) {
  let parent = rule.parent;
  while (parent) {
    if (parent.type === "atrule" && parent.name === "media") return true;
    parent = parent.parent;
  }
  return false;
}

const wideScreenMedia = [];
shellRoot.walkAtRules("media", (atRule) => {
  if (/^\(min-width\s*:\s*1600px\)$/.test(atRule.params.replace(/\s+/g, " ").trim())) {
    wideScreenMedia.push(atRule);
  }
});
const workbenchRule = wideScreenMedia
  .flatMap((atRule) => {
    const rules = [];
    atRule.walkRules((rule) => rules.push(rule));
    return rules;
  })
  .find((rule) => hasSelector(rule, ".workbench") && hasDeclaration(
    rule,
    "padding-inline",
    "max(22px, calc((100vw - 1560px) / 2))",
  ));
if (!workbenchRule) {
  shellError(wideScreenMedia[0] ?? shellRoot, ".workbench must declare wide-screen centering");
}

for (const selector of [".panel-controls", ".backtest-controls"]) {
  let rule;
  shellRoot.walkRules((candidate) => {
    if (!rule && !isWithinMedia(candidate)
      && hasSelector(candidate, selector)
      && hasDeclaration(candidate, "max-width", "1200px")) {
      rule = candidate;
    }
  });
  if (!rule) shellError(shellRoot, `${selector} must declare max-width: 1200px`);
}

for (const selector of [".notes p", ".evidence-list p", ".agent-final-reply"]) {
  let rule;
  shellRoot.walkRules((candidate) => {
    if (!rule && !isWithinMedia(candidate)
      && hasSelector(candidate, selector)
      && hasDeclaration(candidate, "max-width", "76ch")) {
      rule = candidate;
    }
  });
  if (!rule) shellError(shellRoot, `${selector} must declare max-width: 76ch`);
}
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
