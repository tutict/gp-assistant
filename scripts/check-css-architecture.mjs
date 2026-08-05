#!/usr/bin/env node

import { createRequire } from "node:module";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const postcss = frontendRequire("postcss");
const stylesDirectory = fileURLToPath(
  new URL("../desktop/frontend/src/styles/", import.meta.url),
);
const styleFiles = readdirSync(stylesDirectory)
  .filter((file) => file.endsWith(".css"))
  .sort();
const errors = [];
const parsed = new Map();

function lineFor(node) {
  return node.source?.start?.line ?? 1;
}

function addError(file, node, message) {
  errors.push(`${file}:${lineFor(node)} ${message}`);
}

function isWithin(rule, atRuleName) {
  let parent = rule.parent;
  while (parent) {
    if (parent.type === "atrule" && parent.name === atRuleName) return true;
    parent = parent.parent;
  }
  return false;
}

for (const file of styleFiles) {
  const source = readFileSync(`${stylesDirectory}/${file}`, "utf8");
  parsed.set(file, postcss.parse(source, { from: file }));
}

const globalRoot = parsed.get("global.css");
for (const statement of globalRoot.nodes.filter(
  (node) => node.type === "atrule" && node.name === "import",
)) {
  if (!/\blayer\s*\(/.test(statement.params)) {
    addError("global.css", statement, "every @import must declare layer(...)");
  }
}

const selectorFiles = new Map();
for (const [file, root] of parsed) {
  if (file === "tokens.css") continue;
  root.walkRules((rule) => {
    if (isWithin(rule, "media") || isWithin(rule, "keyframes")) return;
    for (const selector of postcss.list.comma(rule.selector)) {
      const normalized = selector.replace(/\s+/g, " ").trim();
      if (!normalized) continue;
      if (!selectorFiles.has(normalized)) selectorFiles.set(normalized, new Set());
      selectorFiles.get(normalized).add(file);
    }
  });
}

const duplicatedSelectors = [...selectorFiles]
  .filter(([, files]) => files.size > 1)
  .sort(([left], [right]) => left.localeCompare(right));
const duplicateRate = selectorFiles.size === 0
  ? 0
  : duplicatedSelectors.length / selectorFiles.size;
if (duplicateRate > 0.05) {
  const sample = duplicatedSelectors
    .slice(0, 20)
    .map(([selector, files]) => `${selector} (${[...files].join(", ")})`)
    .join("; ");
  errors.push(
    `cross-file selector duplication is ${(duplicateRate * 100).toFixed(1)}% `
      + `(${duplicatedSelectors.length}/${selectorFiles.size}); limit is 5%. ${sample}`,
  );
}

for (const [file, root] of parsed) {
  if (file === "tokens.css") continue;
  root.walkDecls((declaration) => {
    if (/#[0-9a-f]{3,8}\b/i.test(declaration.value)) {
      addError(file, declaration, `hex color must be a token: ${declaration.value}`);
    }
    if (declaration.prop !== "scrollbar-width" || declaration.value.trim() !== "none") {
      return;
    }
    const selector = declaration.parent?.selector || "";
    const componentException = /\.(?:panel-tabs|rag-tabs|agent-input)\b/.test(selector);
    let mobileOnly = false;
    let parent = declaration.parent;
    while (parent) {
      if (parent.type === "atrule" && parent.name === "media") {
        mobileOnly = /max-width\s*:\s*(?:768px|48rem)/.test(parent.params);
        break;
      }
      parent = parent.parent;
    }
    if (!componentException && !mobileOnly) {
      addError(file, declaration, "scrollbar-width:none is only allowed on mobile or approved horizontal scrollers");
    }
  });
}

if (errors.length > 0) {
  console.error(`CSS architecture guard failed:\n${errors.join("\n")}`);
  process.exit(1);
}

console.log(
  `CSS architecture guard passed (${styleFiles.length} stylesheets, `
    + `${(duplicateRate * 100).toFixed(1)}% cross-file duplication)`,
);
