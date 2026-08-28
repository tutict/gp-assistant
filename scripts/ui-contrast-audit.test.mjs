import assert from "node:assert/strict";
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
