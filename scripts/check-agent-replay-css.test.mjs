import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pages = readFileSync(resolve(repoRoot, "desktop", "frontend", "src", "styles", "pages.css"), "utf8");
const responsive = readFileSync(resolve(repoRoot, "desktop", "frontend", "src", "styles", "responsive.css"), "utf8");

function extractRuleBlock(css, selector) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`(?:^|\\n)\\s*${escapedSelector}\\s*\\{([^{}]*)\\}`, "m"));

  assert.ok(match, `Expected ${selector} rule block`);
  return match[1];
}

test("agent replay styles cover the actual drawer and replay controls", () => {
  const drawer = extractRuleBlock(pages, ".agent-run-drawer");

  assert.match(pages, /\.agent-thread-toolbar\s*\{/);
  assert.match(pages, /grid-template-rows:\s*auto\s+minmax\(0,\s*1fr\)\s+auto/);
  assert.match(drawer, /position:\s*absolute\s*;/);
  assert.match(drawer, /inset:\s*0\s+0\s+0\s+auto\s*;/);
  assert.match(drawer, /width:\s*clamp\(440px,\s*44vw,\s*640px\)\s*;/);
  assert.match(drawer, /grid-template-rows:\s*auto\s+minmax\(0,\s*1fr\)\s*;/);
  assert.match(pages, /\.agent-run-drawer-header\s*\{/);
  assert.match(pages, /\.agent-run-list\s*,\s*\.agent-run-detail\s*\{/);
  assert.match(pages, /\.agent-run-status\[data-status="failed"\]/);
  assert.match(pages, /\.agent-message-replay\s*\{/);
});

test("agent replay drawer has full-screen mobile geometry above the rail", () => {
  const drawer = extractRuleBlock(responsive, ".agent-run-drawer");

  assert.match(drawer, /position:\s*fixed\s*;/);
  assert.match(drawer, /inset:\s*0\s*;/);
  assert.match(drawer, /width:\s*100%\s*;/);
  assert.match(drawer, /max-width:\s*none\s*;/);
  assert.match(drawer, /z-index:\s*calc\(var\(--z-drawer\)\s*\+\s*1\)\s*;/);
  assert.match(responsive, /\.agent-run-drawer-header\s*\{[^}]*safe-area-inset-top/s);
  assert.match(responsive, /\.agent-run-list\s*,\s*\.agent-run-detail\s*\{[^}]*safe-area-inset-bottom/s);
  assert.match(responsive, /\.agent-thread-history\s*\{[^}]*display:\s*none/s);
  assert.match(responsive, /\.agent-mobile-history\s*\{[^}]*display:\s*inline-grid/s);
});
