import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const stylesDirectory = new URL("../styles/", import.meta.url);
const responsiveCss = readFileSync(new URL("responsive.css", stylesDirectory), "utf8");

describe("agent scroll contract", () => {
  it("applies the scroll lock to desktop widths above 1180px", () => {
    expect(responsiveCss).toMatch(
      /@media\s*\(min-width:\s*769px\)\s*\{\s*\.app\[data-active-view="agent"\]\s*\{/,
    );
  });

  it("keeps the desktop agent surface non-scrollable", () => {
    expect(responsiveCss).toContain(".app[data-active-view=\"agent\"] {");
    expect(responsiveCss).toContain("height: 100dvh;");
    expect(responsiveCss).toContain("overflow: hidden;");
    expect(responsiveCss).toContain(".app[data-active-view=\"agent\"] .workbench {");
    expect(responsiveCss).toContain("height: calc(100dvh - var(--header-height));");
    expect(responsiveCss).toContain(".app[data-active-view=\"agent\"] .agent-thread,");
    expect(responsiveCss).toContain(".app[data-active-view=\"agent\"] .agent-run-list,");
    expect(responsiveCss).toContain(".app[data-active-view=\"agent\"] .agent-run-detail {");
  });
});
