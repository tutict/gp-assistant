import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readdirSync, readFileSync } = await import(nodeFs);

const stylesUrl = new URL("../styles/", import.meta.url);
const pages = readFileSync(new URL("../styles/pages.css", import.meta.url), "utf8");
const screenshotHarness = readFileSync(new URL("../../../../scripts/ui-screenshot.mjs", import.meta.url), "utf8");
const allStyles = readdirSync(stylesUrl)
  .filter((file: string) => file.endsWith(".css"))
  .map((file: string) => readFileSync(new URL(`../styles/${file}`, import.meta.url), "utf8"))
  .join("\n");

function ruleBody(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return source.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

describe("refresh toolbar CSS contract", () => {
  it("removes the obsolete data status selectors from stylesheets", () => {
    for (const selector of [
      ".screen-toolbar-main",
      ".screen-toolbar-side",
      ".data-source-status",
      ".screen-status-row",
      ".data-source-main",
      ".data-source-tools",
      ".data-source-actions",
      ".status-item",
    ]) {
      expect(allStyles).not.toContain(selector);
    }
  });

  it("does not reference the removed surface token", () => {
    expect(allStyles).not.toContain("--surface-1");
  });

  it("keeps the log toggle neutral", () => {
    expect(ruleBody(pages, ".refresh-log-toggle")).not.toContain("var(--accent)");
  });

  it("uses a single-line flex toolbar with an auto-pushing status summary", () => {
    expect(ruleBody(pages, ".screen-toolbar-card.screen-toolbar-compact")).toMatch(/display\s*:\s*flex/);
    expect(ruleBody(pages, ".screen-toolbar-status")).toMatch(/margin-right\s*:\s*auto/);
  });

  it("registers the requested desktop toolbar screenshot states", () => {
    for (const device of ["desktop-1440-dark", "desktop-1440-light", "desktop-1920-dark", "desktop-1920-light"]) {
      expect(screenshotHarness).toContain(`name: "${device}"`);
    }
    expect(screenshotHarness).toContain("refresh-toolbar");
    expect(screenshotHarness).toContain("collapsed.png");
    expect(screenshotHarness).toContain("log-expanded.png");
    expect(screenshotHarness).toContain("maintenance-open.png");
  });
});
