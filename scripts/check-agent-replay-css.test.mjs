import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const pages = readFileSync("desktop/frontend/src/styles/pages.css", "utf8");
const responsive = readFileSync("desktop/frontend/src/styles/responsive.css", "utf8");

test("agent replay styles cover the actual drawer and replay controls", () => {
  assert.match(pages, /\.agent-thread-toolbar\s*\{/);
  assert.match(pages, /grid-template-rows:\s*auto\s+minmax\(0,\s*1fr\)\s+auto/);
  assert.match(pages, /\.agent-run-drawer\s*\{/);
  assert.match(pages, /position:\s*absolute/);
  assert.match(pages, /width:\s*clamp\(440px,\s*44vw,\s*640px\)/);
  assert.match(pages, /\.agent-run-drawer-header\s*\{/);
  assert.match(pages, /\.agent-run-list\s*,\s*\.agent-run-detail\s*\{/);
  assert.match(pages, /\.agent-run-status\.failed/);
  assert.match(pages, /\.agent-run-status\[data-status="failed"\]/);
  assert.match(pages, /\.agent-message-replay\s*\{/);
});

test("agent replay drawer has full-screen mobile geometry above the rail", () => {
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*position:\s*fixed/s);
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*inset:\s*0/s);
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*width:\s*100%/s);
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*max-width:\s*none/s);
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*z-index:\s*calc\(var\(--z-drawer\)\s*\+\s*1\)/s);
  assert.match(responsive, /\.agent-run-drawer-header\s*\{[^}]*safe-area-inset-top/s);
  assert.match(responsive, /\.agent-run-list\s*,\s*\.agent-run-detail\s*\{[^}]*safe-area-inset-bottom/s);
  assert.match(responsive, /\.agent-thread-history\s*\{[^}]*display:\s*none/s);
  assert.match(responsive, /\.agent-mobile-history\s*\{[^}]*display:\s*inline-grid/s);
});
