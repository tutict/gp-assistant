import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readdirSync, readFileSync } = await import(nodeFs);
const stylesDirectory = new URL("../styles/", import.meta.url);
const styleFiles = readdirSync(stylesDirectory) as string[];
const styles: Array<{ file: string; css: string }> = styleFiles
  .filter((file: string) => file.endsWith(".css"))
  .map((file: string) => ({
    file,
    css: readFileSync(new URL(file, stylesDirectory), "utf8"),
  }));
const allCss = styles.map(({ file, css }) => `/* ${file} */\n${css}`).join("\n");
const responsiveCss = styles.find(({ file }) => file === "responsive.css")?.css || "";
const componentsCss = styles.find(({ file }) => file === "components.css")?.css || "";
const researchCss = styles.find(({ file }) => file === "research.css")?.css || "";
const tokensCss = styles.find(({ file }) => file === "tokens.css")?.css || "";

describe("mobile UI density contract", () => {
  it("does not use platform classes to select layout density", () => {
    expect(allCss).not.toMatch(
      /\.(?:android-(?:phone|tablet|compact|bottom-nav|landscape|portrait)|mobile-tauri)\b/,
    );
  });

  it("keeps root rem sizing stable and avoids text-only scaling", () => {
    expect(allCss).not.toMatch(/(?:^|})\s*html\s*\{[^}]*\bfont-size\s*:/s);
    for (const match of allCss.matchAll(/(?:-webkit-)?text-size-adjust\s*:\s*([^;}]+)/g)) {
      expect(match[1].trim()).toBe("100%");
    }
  });

  it("defines the shared typography and touch tokens", () => {
    for (const token of [
      "--fs-body: 14px",
      "--fs-data: 13px",
      "--fs-label: 12px",
      "--fs-caption: 12px",
      "--touch-comfort: 44px",
      "--touch-dense: 32px",
      "--nav-height: 60px",
    ]) {
      expect(tokensCss).toContain(token);
    }
  });

  it("keeps mobile hit areas independent of density", () => {
    expect(responsiveCss).toContain("min-height: 44px");
    expect(responsiveCss).toContain("min-width: 44px");
    const compact = tokensCss.match(/:root\[data-density="compact"\]\s*\{([^}]*)\}/)?.[1] || "";
    expect(compact).not.toContain("--fs-");
    expect(tokensCss).toContain("--control-height: 44px");
  });

  it("uses a labeled mobile stock list and semantic desktop table", () => {
    const stock = readFileSync(new URL("../components/StockList.tsx", import.meta.url), "utf8");
    const css = readFileSync(new URL("../styles/screen.css", import.meta.url), "utf8");
    expect(stock).toContain('<table className="stock-comparison-table">');
    expect(stock).toContain('<dl className="stock-mobile-metrics">');
    expect(stock).toContain('aria-expanded={expanded}');
    expect(css).toContain('flex-wrap: nowrap');
  });

  it("does not reserve an empty control-panel row above the mobile screen run button", () => {
    expect(responsiveCss).toMatch(
      /\.screen-panel-controls,\s*\.screen-panel-run-card\s*\{[^}]*min-height:\s*0[^}]*margin-top:\s*0/,
    );
  });

  it("keeps all five mobile modes reachable in a horizontal rail", () => {
    expect(responsiveCss).toMatch(/\.screen-panel-container > \.screen-panel-tabs\s*\{[^}]*display:\s*flex[^}]*overflow-x:\s*auto/);
  });

  it("keeps agent conversation history as frameless list rows", () => {
    expect(componentsCss).toMatch(
      /\.agent-history-item\s*\{[^}]*border:\s*0[^}]*border-bottom:\s*1px solid var\(--line-soft\)[^}]*border-radius:\s*0/,
    );
    expect(componentsCss).toMatch(
      /\.agent-history-item\.active\s*\{[^}]*box-shadow:\s*inset 2px 0 var\(--agent-accent\)/,
    );
    expect(componentsCss).toMatch(
      /\.agent-history-main\s*\{[^}]*-webkit-tap-highlight-color:\s*transparent/,
    );
    expect(componentsCss).toMatch(
      /\.agent-history-main:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--accent-strong\)/,
    );
  });

  it("keeps the research message stream as the scroll container", () => {
    expect(researchCss).toMatch(
      /\.research-stream-body\s*\{[^}]*height:\s*100%[^}]*min-height:\s*0[^}]*overflow-x:\s*hidden[^}]*overflow-y:\s*auto[^}]*overscroll-behavior:\s*contain/,
    );
  });

  it("rejects literal font sizes below the readability floor", () => {
    for (const { css } of styles) {
      for (const match of css.matchAll(/font-size:\s*([\d.]+)px/g)) {
        expect(Number(match[1])).toBeGreaterThanOrEqual(10);
      }
    }
  });

  it("keeps the responsive stylesheet within the maintainable limit", () => {
    expect(responsiveCss.split(/\r?\n/).length).toBeLessThanOrEqual(1200);
  });
});
