import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import {
  compositeColor,
  contrastRatio,
  parseCssColor,
  requiredContrast,
} from "./ui-contrast-audit.mjs";

test("parses browser rgb and rgba colors", () => {
  assert.deepEqual(parseCssColor("rgb(24, 33, 42)"), { r: 24, g: 33, b: 42, a: 1 });
  assert.deepEqual(parseCssColor("rgba(255, 255, 255, 0.6)"), {
    r: 255, g: 255, b: 255, a: 0.6,
  });
  assert.deepEqual(parseCssColor("color(srgb 0.995765 0.996706 0.997647)"), {
    r: 253.920075,
    g: 254.16003,
    b: 254.399985,
    a: 1,
  });
});

test("composites translucent text before measuring contrast", () => {
  const foreground = compositeColor(
    parseCssColor("rgba(255, 255, 255, 0.6)"),
    parseCssColor("rgb(13, 16, 20)"),
  );
  assert.deepEqual(foreground, { r: 158.2, g: 159.4, b: 161, a: 1 });
  assert.ok(Math.abs(contrastRatio(foreground, parseCssColor("rgb(13, 16, 20)")) - 7.23) < 0.02);
});

test("uses WCAG large-text thresholds", () => {
  assert.equal(requiredContrast({ fontSize: 18, fontWeight: 400 }), 3);
  assert.equal(requiredContrast({ fontSize: 14, fontWeight: 700 }), 3);
  assert.equal(requiredContrast({ fontSize: 14, fontWeight: 600 }), 4.5);
  assert.equal(requiredContrast({ fontSize: 17, fontWeight: 700 }), 3);
});



function hexColor(hex) {
  const value = hex.replace("#", "");
  const expanded = value.length === 3 ? value.split("").map((part) => part + part).join("") : value;
  const number = Number.parseInt(expanded, 16);
  return parseCssColor(`rgb(${(number >> 16) & 255}, ${(number >> 8) & 255}, ${number & 255})`);
}

test("keeps tertiary text and links above the body contrast floor", () => {
  const tokens = readFileSync(fileURLToPath(new URL("../desktop/frontend/src/styles/tokens.css", import.meta.url)), "utf8");
  const pairs = [
    ["#96a3b0", "#222a33"],
    ["#546270", "#edf1f5"],
    ["#ffb4ae", "#14191f"],
    ["#ffb4ae", "#0d1014"],
    ["#c6322c", "#ffffff"],
    ["#c6322c", "#f4f6f8"],
  ];
  for (const [foreground, background] of pairs) {
    assert.ok(tokens.includes(foreground), `${foreground} should be declared`);
    assert.ok(contrastRatio(hexColor(foreground), hexColor(background)) >= 4.5);
  }
});