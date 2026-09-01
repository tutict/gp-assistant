#!/usr/bin/env node

import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const postcss = frontendRequire("postcss");
const defaultTokensPath = fileURLToPath(
  new URL("../desktop/frontend/src/styles/tokens.css", import.meta.url),
);
// Agent send colors are intentionally independent accents; all other color tokens,
// including chart tokens, must have an explicit light-theme mapping.
const exemptColorToken = /^--agent-send-/;
const directColorValue = /(?:#[0-9a-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color|color-mix)\(|\b(?:transparent|currentcolor)\b)/i;

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
}

export function themeDeclarations(source) {
  const root = postcss.parse(source);
  const declarations = new Map();
  for (const selector of [":root", '[data-theme="light"]']) {
    const values = new Map();
    const rule = root.nodes.find(
      (node) => node.type === "rule" && node.selector.trim() === selector,
    );
    if (!rule) throw new Error(`Missing top-level theme block: ${selector}`);
    rule.walkDecls(/^--/, (declaration) => values.set(declaration.prop, declaration.value.trim()));
    declarations.set(selector, values);
  }
  return declarations;
}

export function colorTokenNames(rootDeclarations) {
  const resolved = new Map();
  const resolving = new Set();
  const isColor = (name) => {
    if (resolved.has(name)) return resolved.get(name);
    if (resolving.has(name)) return false;
    const value = rootDeclarations.get(name) || "";
    if (directColorValue.test(value)) {
      resolved.set(name, true);
      return true;
    }
    resolving.add(name);
    const references = [...value.matchAll(/var\(\s*(--[\w-]+)/g)].map((match) => match[1]);
    const result = references.some((reference) => isColor(reference));
    resolving.delete(name);
    resolved.set(name, result);
    return result;
  };

  return [...rootDeclarations.keys()]
    .filter((name) => !exemptColorToken.test(name) && isColor(name))
    .sort();
}

export function missingLightColorTokens(source) {
  const themes = themeDeclarations(source);
  const root = themes.get(":root");
  const light = themes.get('[data-theme="light"]');
  return colorTokenNames(root).filter((name) => !light.has(name));
}

export function runThemeParity(tokensPath = defaultTokensPath) {
  const source = readFileSync(tokensPath, "utf8");
  const themes = themeDeclarations(source);
  const colorTokens = colorTokenNames(themes.get(":root"));
  const missing = colorTokens.filter(
    (name) => !themes.get('[data-theme="light"]').has(name),
  );
  if (missing.length > 0) {
    console.error(
      `Theme parity guard failed: ${missing.length} color token(s) lack light overrides:\n`
        + missing.map((name) => `  ${name}`).join("\n"),
    );
    return 1;
  }
  console.log(`Theme parity guard passed (${colorTokens.length} color tokens)`);
  return 0;
}

const isDirect = process.argv[1]
  && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isDirect) {
  process.exitCode = runThemeParity(resolve(option("--tokens", defaultTokensPath)));
}
