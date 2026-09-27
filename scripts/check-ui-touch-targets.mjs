#!/usr/bin/env node

import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const postcss = frontendRequire("postcss");

export const TOUCH_THRESHOLDS = Object.freeze({ primary: 48, secondary: 44, dense: 32 });

const platformDensitySelector = /(?:^|[.\s])(?:android-(?:phone|tablet|compact|bottom-nav|landscape|portrait)|mobile-tauri)\b/i;
const mobileMedia = /max-width\s*:\s*(?:[0-7]\d\d|768)px/i;
const nativeControl = /(?:^|[\s>+~])(?:button|input|select|textarea|summary)(?=[:.#\[]|\s|[>+~]|$)/i;
const roleControl = /\[role\s*=\s*["']?(?:button|tab)["']?\]/i;
const touchTierAttribute = /\[data-touch-tier\s*=\s*["']?(?:primary|secondary|dense)["']?\]/i;
const controlClass = /(?:^|[-_])(?:button|btn|action|toggle|tab|nav-link|input|select|submit|retry|send|close|remove|clear|collapse)(?:[-_]?button)?$/i;
const secondaryClass = /(?:^|[-_])(?:icon-button|icon-btn|close-button|dismiss-button|favorite-button|expand-button|collapse-button|mobile-nav-toggle|mobile-nav-close|clear-btn|stock-row-action|source-toggle|theme-toggle|watchlist-remove|research-icon-button|research-mobile-close|research-evidence-close|screen-mobile-refresh-btn)$/i;
const denseClass = /(?:^|[-_])(?:chip|badge|tag|pill|dense)$/i;
const fixedChromeSelector = /(?:^|[.#_-])(?:app-)?(?:header|nav|navigation|toolbar|composer|bottom-bar|input-bar)(?:$|[.#_\s:[-])/i;

function issue(file, line, code, message, selector) {
  return { code, file, line, message, ...(selector ? { selector } : {}) };
}

function parseLength(value, customProperties, resolving = new Set()) {
  if (!value) return null;
  const normalized = value.trim();
  if (/^0(?:px)?$/i.test(normalized)) return 0;
  const direct = normalized.match(/^(-?\d+(?:\.\d+)?)px$/i);
  if (direct) return Number(direct[1]);

  const variable = normalized.match(/^var\(\s*(--[\w-]+)(?:\s*,\s*([^)]*))?\)$/i);
  if (variable) {
    const [, name, fallback] = variable;
    if (resolving.has(name)) return null;
    const source = customProperties.get(name);
    if (source === undefined) return fallback ? parseLength(fallback, customProperties, resolving) : null;
    const next = new Set(resolving);
    next.add(name);
    return parseLength(source, customProperties, next);
  }

  const maxMatch = normalized.match(/^max\((.*)\)$/i);
  if (maxMatch) {
    const values = maxMatch[1]
      .split(",")
      .map((part) => parseLength(part, customProperties, resolving))
      .filter((value) => value !== null);
    return values.length ? Math.max(...values) : null;
  }

  const clampMatch = normalized.match(/^clamp\((.*)\)$/i);
  if (clampMatch) return parseLength(clampMatch[1].split(",")[0], customProperties, resolving);
  return null;
}


function parseFontSize(value, customProperties, resolving = new Set()) {
  if (!value) return null;
  const normalized = value.trim();
  const direct = normalized.match(/^(-?\d+(?:\.\d+)?)px$/i);
  if (direct) return Number(direct[1]);
  const rem = normalized.match(/^(-?\d+(?:\.\d+)?)rem$/i);
  if (rem) return Number(rem[1]) * 16;
  const variable = normalized.match(/^var\(\s*(--[\w-]+)(?:\s*,\s*([^)]*))?\)$/i);
  if (!variable) return null;
  const [, name, fallback] = variable;
  if (resolving.has(name)) return null;
  const source = customProperties.get(name);
  if (source === undefined) return fallback ? parseFontSize(fallback, customProperties, resolving) : null;
  const next = new Set(resolving);
  next.add(name);
  return parseFontSize(source, customProperties, next);
}
function declarationsFor(rule) {

  const declarations = new Map();
  rule.walkDecls((declaration) => declarations.set(declaration.prop.toLowerCase(), declaration.value.trim()));
  return declarations;
}

function isMobileContext(rule) {
  let parent = rule.parent;
  while (parent) {
    if (parent.type === "atrule" && parent.name.toLowerCase() === "media" && mobileMedia.test(parent.params)) return true;
    parent = parent.parent;
  }
  return false;
}

function lastCompound(selector) {
  return selector.split(/\s+|>|\+|~/).at(-1)?.trim() ?? selector;
}

function isHiddenInput(selector, declarations) {
  return /\binput\s*\[type\s*=\s*["']?(?:checkbox|radio)["']?\]/i.test(selector)
    && (declarations.get("position") === "absolute"
      || declarations.get("opacity") === "0"
      || declarations.get("pointer-events") === "none");
}

function controlKind(selector) {
  const target = lastCompound(selector);
  const classes = [...target.matchAll(/\.([\w-]+)/g)].map((match) => match[1]);
  const hasControlClass = classes.some((name) => controlClass.test(name));
  if (!nativeControl.test(target) && !roleControl.test(target) && !touchTierAttribute.test(target) && !hasControlClass) return null;
  if (/criteria-toggle(?:$|[\s.#:[>+~])/i.test(selector)) return null;
  if (/research-actions|research-icon-button|sentiment-scrubber|sentiment-header-actions|llm-endpoint-mode/i.test(selector)) return "secondary";
  if (/\[data-touch-tier\s*=\s*["']dense["']?\]/i.test(target) || classes.some((name) => denseClass.test(name))) return "dense";
  if (/\[data-touch-tier\s*=\s*["']secondary["']?\]/i.test(target) || classes.some((name) => secondaryClass.test(name))) return "secondary";
  return "primary";
}

function isIconLike(selector) {
  const target = lastCompound(selector);
  return /icon-button|icon-btn|close-button|dismiss-button|favorite-button|expand-button|collapse-button|\[data-touch-tier\s*=\s*["'](?:secondary|dense)["']?\]/i.test(target);
}

function effectiveDimension(declarations, property, customProperties) {
  const values = [declarations.get(`min-${property}`), declarations.get(property)]
    .map((value) => parseLength(value, customProperties))
    .filter((value) => value !== null);
  return values.length ? Math.max(...values) : null;
}

function mergeDeclarations(target, source) {
  for (const [property, value] of source) target.set(property, value);
}

function collectMobileControls(parsedSources, customProperties) {
  const controls = new Map();
  for (const { file, root } of parsedSources) {
    root.walkRules((rule) => {
      if (!isMobileContext(rule)) return;
      const declarations = declarationsFor(rule);
      if (!["height", "min-height", "width", "min-width", "block-size", "min-block-size", "inline-size", "min-inline-size"]
        .some((property) => declarations.has(property))) return;
      for (const selector of postcss.list.comma(rule.selector).map((value) => value.trim()).filter(Boolean)) {
        if (isHiddenInput(selector, declarations)) continue;
        const kind = controlKind(selector);
        if (!kind) continue;
        const current = controls.get(selector) ?? {
          file,
          line: rule.source?.start?.line ?? 1,
          declarations: new Map(),
          kind,
        };
        mergeDeclarations(current.declarations, declarations);
        current.file = file;
        current.line = rule.source?.start?.line ?? current.line;
        current.kind = kind;
        controls.set(selector, current);
      }
    });
  }
  return controls;
}

function checkSafeAreaForFixedChrome(parsedSources, customProperties, issues) {
  const bySelector = new Map();
  for (const { file, root } of parsedSources) {
    root.walkRules((rule) => {
      for (const selector of postcss.list.comma(rule.selector).map((value) => value.trim())) {
        if (!bySelector.has(selector)) bySelector.set(selector, { file, line: rule.source?.start?.line ?? 1, declarations: new Map() });
        mergeDeclarations(bySelector.get(selector).declarations, declarationsFor(rule));
      }
    });
  }

  const resolveValue = (value, seen = new Set()) => {
    if (!value) return "";
    let resolved = value;
    for (const match of value.matchAll(/var\(\s*(--[\w-]+)/g)) {
      const name = match[1];
      if (seen.has(name)) continue;
      const next = new Set(seen);
      next.add(name);
      resolved += ` ${resolveValue(customProperties.get(name), next)}`;
    }
    return resolved;
  };

  for (const [selector, { file, line, declarations }] of bySelector) {
    if (/overlay|backdrop/i.test(selector) || !fixedChromeSelector.test(selector) || declarations.get("position") !== "fixed") continue;
    const bottom = ["bottom", "inset", "inset-block", "inset-block-end"].some((property) => declarations.has(property));
    const top = ["top", "inset", "inset-block", "inset-block-start"].some((property) => declarations.has(property));
    if (!bottom && !top) continue;
    const values = [...declarations.entries()]
      .filter(([property]) => /^(?:top|bottom|inset|inset-block|padding-block|padding-top|padding-bottom)$/i.test(property))
      .map(([, value]) => resolveValue(value))
      .join(" ");
    const missingEdge = bottom && !/safe-area-inset-bottom/i.test(values)
      ? "bottom"
      : top && !/safe-area-inset-top/i.test(values) ? "top" : null;
    if (missingEdge) issues.push(issue(file, line, "FIXED_INSET_NO_SAFE_AREA", `fixed ${missingEdge} chrome must account for the matching safe-area inset`, selector));
  }
}

export function auditTouchTargets(sources, { thresholds = TOUCH_THRESHOLDS } = {}) {
  const issues = [];
  const parsedSources = sources.map(({ file, source }) => ({ file, source, root: postcss.parse(source, { from: file }) }));
  const customProperties = new Map();
  for (const { root } of parsedSources) {
    root.walkDecls((declaration) => {
      if (declaration.prop.startsWith("--")) customProperties.set(declaration.prop, declaration.value.trim());
    });
  }

  for (const { file, root } of parsedSources) {
    root.walkRules((rule) => {
      const selectors = postcss.list.comma(rule.selector).map((selector) => selector.trim()).filter(Boolean);
      const declarations = declarationsFor(rule);
      const line = rule.source?.start?.line ?? 1;
      if (selectors.some((selector) => /^html(?:[.#:[].*)?$/.test(selector) && !/[\s>+~]/.test(selector)) && declarations.has("font-size")) {
        issues.push(issue(file, line, "HTML_FONT_SIZE_OVERRIDE", "html font-size must remain at the browser default", selectors[0]));
      }
      const fontSize = parseFontSize(declarations.get("font-size"), customProperties);
      if (fontSize !== null && fontSize < 10) {
        for (const selector of selectors) {
          issues.push(issue(file, line, "FONT_SIZE_TOO_SMALL", `font size ${fontSize}px is below the 10px Android readability floor`, selector));
        }
      }
      for (const property of ["-webkit-text-size-adjust", "text-size-adjust"]) {
        const value = declarations.get(property);
        if (value && value !== "100%") issues.push(issue(file, line, "TEXT_SIZE_ADJUST_OVERRIDE", `${property} must be 100%`, selectors[0]));
      }
      if (selectors.some((selector) => platformDensitySelector.test(selector))) {
        issues.push(issue(file, line, "PLATFORM_DENSITY_SELECTOR", "platform-specific density selectors are forbidden", selectors[0]));
      }
    });
  }

  for (const [selector, { file, line, declarations, kind }] of collectMobileControls(parsedSources, customProperties)) {
    const threshold = thresholds[kind];
    const height = Math.max(
      effectiveDimension(declarations, "height", customProperties) ?? -Infinity,
      effectiveDimension(declarations, "block-size", customProperties) ?? -Infinity,
    );
    const width = Math.max(
      effectiveDimension(declarations, "width", customProperties) ?? -Infinity,
      effectiveDimension(declarations, "inline-size", customProperties) ?? -Infinity,
    );
    const needsWidth = isIconLike(selector);
    if (!Number.isFinite(height) || (needsWidth && !Number.isFinite(width))) {
      issues.push(issue(file, line, "TOUCH_TARGET_SIZE_UNVERIFIABLE", `${kind} touch target needs a resolvable ${needsWidth ? "width and height" : "height"} of at least ${threshold}px`, selector));
      continue;
    }
    if (height < threshold || (needsWidth && width < threshold)) {
      const dimensions = [`height ${height}px`, needsWidth ? `width ${width}px` : null].filter(Boolean).join(", ");
      issues.push(issue(file, line, "TOUCH_TARGET_TOO_SMALL", `${kind} touch target is ${dimensions}; minimum is ${threshold}px`, selector));
    }
  }

  checkSafeAreaForFixedChrome(parsedSources, customProperties, issues);
  return issues;
}

export function loadStyleSources(stylesDirectory) {
  return readdirSync(stylesDirectory)
    .filter((file) => file.endsWith(".css"))
    .sort()
    .map((file) => ({ file, source: readFileSync(resolve(stylesDirectory, file), "utf8") }));
}

export function formatIssues(issues) {
  return issues.map(({ file, line, code, message, selector }) => `${file}:${line} [${code}] ${message}${selector ? ` (${selector})` : ""}`).join("\n");
}

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const stylesDirectory = option("--styles", fileURLToPath(new URL("../desktop/frontend/src/styles/", import.meta.url)));
  const issues = auditTouchTargets(loadStyleSources(stylesDirectory));
  if (issues.length) {
    console.error(formatIssues(issues));
    process.exitCode = 1;
  } else {
    console.log(`UI touch-target audit passed (${basename(stylesDirectory)}).`);
  }
}


