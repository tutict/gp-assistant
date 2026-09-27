import assert from "node:assert/strict";
import { test } from "node:test";

import { auditColorContract } from "./check-color-contract.mjs";

const passingThemes = `
  :root {
    --bg: #111111; --surface: #1f1f1f; --text: #ffffff;
    --text-secondary: #d0d0d0; --text-tertiary: #b8b8b8;
    --accent: #ff8a80; --success: #72e6a7; --warning: #ffd166;
    --error: #ff8f87; --rise: var(--error); --fall: var(--success);
    --chart-canvas: #111111; --chart-line-1: #58a6ff;
  }
  [data-theme="light"] {
    --bg: #ffffff; --surface: #f4f4f4; --text: #111111;
    --text-secondary: #3f3f3f; --text-tertiary: #4b4b4b;
    --accent: #8f170f; --success: #176b43; --warning: #754c00;
    --error: #8f170f; --rise: var(--error); --fall: var(--success);
    --chart-canvas: #ffffff; --chart-line-1: #1f6fd6;
  }
`;

test("accepts complete semantic color tokens with sufficient text and chart contrast", () => {
  const issues = auditColorContract({
    tokensSource: passingThemes,
    styleSources: [
      { file: "components.css", source: ".button { color: var(--text); background: var(--accent); }" },
    ],
  });

  assert.deepEqual(issues, []);
});

test("reports color tokens missing an explicit light-theme mapping", () => {
  const issues = auditColorContract({
    tokensSource: `
      :root { --bg: #111; --surface: #222; --text: #fff; --chart-line-1: #58a6ff; }
      [data-theme="light"] { --bg: #fff; --surface: #f8f8f8; --text: #111; }
    `,
  });

  assert.ok(issues.some((issue) => issue.code === "LIGHT_THEME_COLOR_TOKEN_MISSING" && issue.message.includes("--chart-line-1")));
});

test("treats color-mix values as color tokens that require a light-theme mapping", () => {
  const issues = auditColorContract({
    tokensSource: `
      :root { --bg: #111; --surface: #222; --text: #fff; --line-soft: color-mix(in srgb, var(--text) 8%, transparent); }
      [data-theme="light"] { --bg: #fff; --surface: #f8f8f8; --text: #111; }
    `,
  });

  assert.ok(issues.some((issue) => issue.code === "LIGHT_THEME_COLOR_TOKEN_MISSING" && issue.message.includes("--line-soft")));
});

test("reports direct paint literals outside the token file but ignores decorative shadows", () => {
  const issues = auditColorContract({
    tokensSource: passingThemes,
    styleSources: [
      { file: "components.css", source: ".error { color: #d83d35; box-shadow: 0 0 8px rgba(0, 0, 0, .2); }" },
    ],
  });

  assert.equal(issues.length, 1);
  assert.equal(issues[0].code, "RAW_COLOR_LITERAL");
  assert.equal(issues[0].file, "components.css");
});

test("reports semantic text colors that fail the 4.5:1 contract", () => {
  const issues = auditColorContract({
    tokensSource: `
      :root { --bg: #ffffff; --surface: #ffffff; --text: #777777; }
      [data-theme="light"] { --bg: #ffffff; --surface: #ffffff; --text: #777777; }
    `,
  });

  assert.ok(issues.some((issue) => issue.code === "COLOR_CONTRAST_TOO_LOW" && issue.message.includes("--text") && issue.message.includes("4.5:1")));
});

test("checks chart series against the chart canvas at the non-text 3:1 threshold", () => {
  const issues = auditColorContract({
    tokensSource: `
      :root { --bg: #ffffff; --surface: #ffffff; --text: #111111; --chart-canvas: #ffffff; --chart-line-1: #cccccc; }
      [data-theme="light"] { --bg: #ffffff; --surface: #ffffff; --text: #111111; --chart-canvas: #ffffff; --chart-line-1: #cccccc; }
    `,
  });

  assert.ok(issues.some((issue) => issue.code === "COLOR_CONTRAST_TOO_LOW" && issue.message.includes("--chart-line-1") && issue.message.includes("3:1")));
});
