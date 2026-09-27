import assert from "node:assert/strict";
import { test } from "node:test";

import { auditTouchTargets } from "./check-ui-touch-targets.mjs";

function audit(source) {
  return auditTouchTargets([{ file: "fixture.css", source }]);
}

test("reports primary and secondary controls below their mobile touch thresholds", () => {
  const issues = audit(`
    :root { --touch-primary: 48px; --touch-secondary: 44px; }
    @media (max-width: 768px) {
      .run-button { min-height: 40px; min-width: 120px; }
      .icon-button { width: 32px; height: 32px; }
    }
  `);

  assert.deepEqual(
    issues.filter((issue) => issue.code === "TOUCH_TARGET_TOO_SMALL").map((issue) => issue.selector),
    [".run-button", ".icon-button"],
  );
  assert.match(issues[0].message, /48px/);
  assert.match(issues[1].message, /44px/);
});

test("accepts desktop sizing when mobile overrides meet primary and secondary thresholds", () => {
  const issues = audit(`
    :root {
      --touch-primary: 48px;
      --touch-secondary: 44px;
      --touch-dense: 32px;
    }
    .run-button { min-height: 40px; }
    .icon-button { width: 32px; height: 32px; }
    @media (max-width: 768px) {
      .run-button { min-height: var(--touch-primary); min-width: 120px; }
      .icon-button { width: var(--touch-secondary); height: var(--touch-secondary); }
      .status-chip[data-touch-tier="dense"] { min-height: var(--touch-dense); min-width: 32px; }
    }
  `);

  assert.deepEqual(issues, []);
});

test("rejects text smaller than the Android readability floor", () => {
  const issues = audit(`
    .tiny-label { font-size: 9px; }
    .rem-label { font-size: 0.5rem; }
    .normal-label { font-size: 12px; }
  `);

  assert.deepEqual(
    issues.filter((issue) => issue.code === "FONT_SIZE_TOO_SMALL").map((issue) => issue.selector),
    [".tiny-label", ".rem-label"],
  );
});
test("rejects unsafe global scaling and platform-specific density selectors", () => {
  const issues = audit(`
    html { font-size: 62.5%; }
    body { -webkit-text-size-adjust: 90%; }
    html.android-phone .nav-link { min-height: 48px; }
  `);

  assert.deepEqual(
    issues.map((issue) => issue.code),
    ["HTML_FONT_SIZE_OVERRIDE", "TEXT_SIZE_ADJUST_OVERRIDE", "PLATFORM_DENSITY_SELECTOR"],
  );
});

test("requires fixed top and bottom app chrome to account for safe-area insets", () => {
  const issues = audit(`
    .drawer-backdrop { position: fixed; inset: 0; }
    .mobile-nav-overlay { position: fixed; inset: 0; }
    .app-header { position: fixed; top: 0; }
    .mobile-nav { position: fixed; bottom: 0; min-height: 48px; }
    .safe-nav { position: fixed; bottom: env(safe-area-inset-bottom); min-height: 48px; }
  `);

  assert.deepEqual(
    issues.filter((issue) => issue.code === "FIXED_INSET_NO_SAFE_AREA").map((issue) => issue.selector),
    [".app-header", ".mobile-nav"],
  );
});

test("does not classify labels, layout groups, child decoration, or hidden inputs as touch targets", () => {
  const issues = audit(`
    @media (max-width: 768px) {
      .agent-run-status-label { min-height: 12px; }
      .panel-controls { min-height: 24px; }
      .close-button span { height: 9px; }
      .toggle-chip input[type="checkbox"] { position: absolute; width: 1px; height: 1px; opacity: 0; }
      .close-button:hover { color: red; }
    }
  `);

  assert.deepEqual(issues, []);
});

