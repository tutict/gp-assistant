import { describe, expect, it } from "vitest";

describe("A-share chart palette", () => {
  it("keeps positive MACD red and negative MACD green on Android", async () => {
    const nodeFs = "node:fs";
    const { readFileSync } = await import(nodeFs);
    const pagesCss = readFileSync(new URL("../styles/pages.css", import.meta.url), "utf8");
    const responsiveCss = readFileSync(new URL("../styles/responsive.css", import.meta.url), "utf8");

    expect(pagesCss).toMatch(/\.kline-chart \.macd-up\s*\{[^}]*color:\s*var\(--rise\);[^}]*fill:\s*var\(--rise\);/s);
    expect(pagesCss).toMatch(/\.kline-chart \.macd-down\s*\{[^}]*color:\s*var\(--fall\);[^}]*fill:\s*var\(--fall\);/s);
    expect(pagesCss).toMatch(/\.kline-chart \.macd-flat\s*\{[^}]*color:\s*var\(--flat\);[^}]*fill:\s*var\(--flat\);/s);
    expect(responsiveCss).not.toMatch(/\.macd-(?:up|down)\s*\{[^}]*(?:color|fill):/s);
    expect(responsiveCss).toMatch(/\.kline-current-line\s*\{[^}]*stroke:\s*currentColor;/s);
  });
});
