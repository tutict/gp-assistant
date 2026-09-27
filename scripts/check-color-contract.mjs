#!/usr/bin/env node

import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const postcss = frontendRequire("postcss");

const lightThemeSelector = '[data-theme="light"]';
const colorLiteral = /#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch)\s*\(|\bcolor\s*\(/i;
const colorFunction = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\s*\(/i;
const colorMixWithLiteral = /color-mix\([^)]*(?:#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch)\s*\()/i;
const exemptColorToken = /^--agent-send-/;
const contrastRoles = [
  ["--text", "--bg"],
  ["--text-secondary", "--surface"],
  ["--text-tertiary", "--surface"],
  ["--accent-text", "--surface"],
  ["--success", "--surface"],
  ["--warning", "--surface"],
  ["--error", "--surface"],
  ["--rise", "--surface"],
  ["--fall", "--surface"],
  ["--chart-line-1", "--chart-canvas", 3],
  ["--chart-line-2", "--chart-canvas", 3],
  ["--chart-line-3", "--chart-canvas", 3],
  ["--chart-line-4", "--chart-canvas", 3],
  ["--chart-line-5", "--chart-canvas", 3],
  ["--chart-benchmark", "--chart-canvas", 3],
];

function issue(file, line, code, message) {
  return { code, file, line, message };
}

function lineNumber(source, index) {
  return source.slice(0, index).split("\n").length;
}

function parseThemeDeclarations(source) {
  const root = postcss.parse(source, { from: "tokens.css" });
  const themes = new Map([[":root", new Map()], [lightThemeSelector, new Map()]]);
  root.walkRules((rule) => {
    const selector = rule.selector.trim();
    const theme = selector === ":root" ? ":root" : selector === lightThemeSelector ? lightThemeSelector : null;
    if (!theme) return;
    const declarations = themes.get(theme);
    rule.walkDecls(/^--/, (declaration) => declarations.set(declaration.prop, declaration.value.trim()));
  });
  return themes;
}

function isDirectColor(value) {
  return colorLiteral.test(value) || /\b(?:transparent|currentcolor)\b/i.test(value);
}

function referencedTokens(value) {
  return [...value.matchAll(/var\(\s*(--[\w-]+)/gi)].map((match) => match[1]);
}

function colorTokenNames(declarations) {
  const resolved = new Map();
  const resolving = new Set();
  const isColor = (name) => {
    if (resolved.has(name)) return resolved.get(name);
    if (resolving.has(name)) return false;
    const value = declarations.get(name) ?? "";
    if (isDirectColor(value)) {
      resolved.set(name, true);
      return true;
    }
    resolving.add(name);
    const result = referencedTokens(value).some((reference) => isColor(reference));
    resolving.delete(name);
    resolved.set(name, result);
    return result;
  };

  return [...declarations.keys()]
    .filter((name) => !exemptColorToken.test(name) && isColor(name))
    .sort();
}

function resolveTokenValue(value, declarations, resolving = new Set()) {
  if (!value) return null;
  const normalized = value.trim();
  const variable = normalized.match(/^var\(\s*(--[\w-]+)(?:\s*,\s*([^)]*))?\)$/i);
  if (!variable) return normalized;
  const [, name, fallback] = variable;
  if (resolving.has(name)) return null;
  const next = new Set(resolving);
  next.add(name);
  return resolveTokenValue(declarations.get(name) ?? fallback, declarations, next);
}

function parseHex(value) {
  const match = value.trim().match(/^#([0-9a-f]+)$/i);
  if (!match || ![3, 4, 6, 8].includes(match[1].length)) return null;
  const raw = match[1].length <= 4
    ? [...match[1]].map((digit) => digit + digit).join("")
    : match[1];
  const alpha = raw.length === 8 ? Number.parseInt(raw.slice(6), 16) / 255 : 1;
  if (alpha < 1) return null;
  return [
    Number.parseInt(raw.slice(0, 2), 16),
    Number.parseInt(raw.slice(2, 4), 16),
    Number.parseInt(raw.slice(4, 6), 16),
  ];
}

function parseRgb(value) {
  const match = value.trim().match(/^rgba?\(\s*([^)]*)\)$/i);
  if (!match) return null;
  const parts = match[1].trim().split(/\s*,\s*|\s*\/\s*|\s+/).filter(Boolean).map((part) => part.trim());
  if (parts.length < 3) return null;
  const alpha = parts[3] === undefined ? 1 : Number(parts[3]);
  if (!Number.isFinite(alpha) || alpha < 1) return null;
  const channels = parts.slice(0, 3).map((part) => {
    if (part.endsWith("%")) return Number(part.slice(0, -1)) * 2.55;
    return Number(part);
  });
  if (channels.some((channel) => !Number.isFinite(channel))) return null;
  return channels.map((channel) => Math.max(0, Math.min(255, channel)));
}

function parseOpaqueColor(value) {
  if (!value || /\b(?:transparent|currentcolor)\b/i.test(value)) return null;
  return parseHex(value) ?? parseRgb(value);
}

function relativeLuminance(rgb) {
  return rgb.reduce((sum, channel, index) => {
    const normalized = channel / 255;
    const linear = normalized <= 0.03928
      ? normalized / 12.92
      : ((normalized + 0.055) / 1.055) ** 2.4;
    return sum + linear * [0.2126, 0.7152, 0.0722][index];
  }, 0);
}

export function contrastRatio(foreground, background) {
  const foregroundColor = Array.isArray(foreground) ? foreground : parseOpaqueColor(foreground);
  const backgroundColor = Array.isArray(background) ? background : parseOpaqueColor(background);
  if (!foregroundColor || !backgroundColor) return null;
  const lighter = Math.max(relativeLuminance(foregroundColor), relativeLuminance(backgroundColor));
  const darker = Math.min(relativeLuminance(foregroundColor), relativeLuminance(backgroundColor));
  return (lighter + 0.05) / (darker + 0.05);
}

function auditTokenContrast(themes, issues) {
  for (const [theme, declarations] of themes) {
    for (const [foregroundName, backgroundName, minimum = 4.5] of contrastRoles) {
      const foreground = parseOpaqueColor(resolveTokenValue(declarations.get(foregroundName), declarations));
      const background = parseOpaqueColor(resolveTokenValue(declarations.get(backgroundName), declarations));
      const ratio = contrastRatio(foreground, background);
      if (ratio !== null && ratio < minimum) {
        issues.push(issue(
          "tokens.css",
          1,
          "COLOR_CONTRAST_TOO_LOW",
          `${theme} ${foregroundName} against ${backgroundName} is ${ratio.toFixed(2)}:1; minimum is ${minimum}:1`,
        ));
      }
    }
  }
}

function auditRawColors(styleSources, issues) {
  for (const { file, source } of styleSources) {
    const root = postcss.parse(source, { from: file });
    root.walkDecls((declaration) => {
      if (declaration.prop.startsWith("--") || /^(?:box|text)-shadow$|^filter$/i.test(declaration.prop)) return;
      const value = declaration.value;
      if ((colorMixWithLiteral.test(value) && !/color-mix\([^)]*var\(/i.test(value)) || colorLiteral.test(value)) {
        issues.push(issue(
          file,
          declaration.source?.start?.line ?? lineNumber(source, source.indexOf(value)),
          "RAW_COLOR_LITERAL",
          `raw color literal is not allowed outside semantic tokens: ${value.trim()}`,
        ));
      }
    });
  }
}

export function auditColorContract({ tokensSource, styleSources = [] }) {
  const issues = [];
  const themes = parseThemeDeclarations(tokensSource);
  const rootDeclarations = themes.get(":root");
  const lightDeclarations = themes.get(lightThemeSelector);
  const colorTokens = colorTokenNames(rootDeclarations);

  for (const name of colorTokens) {
    if (!lightDeclarations.has(name)) {
      issues.push(issue(
        "tokens.css",
        1,
        "LIGHT_THEME_COLOR_TOKEN_MISSING",
        `${name} is a color token in :root and must have an explicit light-theme mapping`,
      ));
    }
  }

  auditRawColors(styleSources, issues);
  auditTokenContrast(themes, issues);
  return issues;
}

export function loadStyleSources(stylesDirectory) {
  return readdirSync(stylesDirectory)
    .filter((file) => file.endsWith(".css"))
    .sort()
    .map((file) => ({
      file,
      source: readFileSync(resolve(stylesDirectory, file), "utf8"),
    }));
}

export function formatIssues(issues) {
  return issues.map(({ file, line, code, message }) => `${file}:${line} [${code}] ${message}`).join("\n");
}

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const stylesDirectory = option(
    "--styles",
    fileURLToPath(new URL("../desktop/frontend/src/styles/", import.meta.url)),
  );
  const tokensPath = option(
    "--tokens",
    fileURLToPath(new URL("../desktop/frontend/src/styles/tokens.css", import.meta.url)),
  );
  const tokensSource = readFileSync(tokensPath, "utf8");
  const issues = auditColorContract({ tokensSource, styleSources: loadStyleSources(stylesDirectory) });
  if (issues.length) {
    console.error(formatIssues(issues));
    process.exitCode = 1;
  } else {
    console.log(`Color contract audit passed (${basename(stylesDirectory)}).`);
  }
}




