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
const pagesCss = styles.find(({ file }) => file === "pages.css")?.css || "";
const componentsCss = styles.find(({ file }) => file === "components.css")?.css || "";
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
      "--fs-caption: 11px",
      "--touch-comfort: 44px",
      "--touch-dense: 32px",
      "--nav-height: 60px",
    ]) {
      expect(tokensCss).toContain(token);
    }
  });

  it("uses readable mobile navigation, tabs, and form controls", () => {
    expect(responsiveCss).toMatch(
      /@media \(max-width: 768px\)[\s\S]*?\.nav-link\s*\{[^}]*font-size:\s*var\(--fs-caption\)/,
    );
    expect(responsiveCss).toMatch(
      /@media \(max-width: 768px\)[\s\S]*?\.panel-tab\s*\{[^}]*min-height:\s*var\(--touch-comfort\)[^}]*font-size:\s*var\(--fs-data\)/,
    );
    expect(responsiveCss).toMatch(
      /\.form-row input,[\s\S]*?\.stock-code-input input\s*\{[^}]*min-height:\s*var\(--touch-comfort\)/,
    );
  });

  it("keeps mobile stock insight metrics readable and summaries intact", () => {
    expect(responsiveCss).toMatch(
      /\.stock-insight-board\s*\{[^}]*display:\s*block/,
    );
    expect(responsiveCss).toMatch(
      /\.stock-insight-board \.score-strip\s*\{[^}]*grid-template-columns:\s*minmax\(0, 3fr\) minmax\(88px, 1fr\)[^}]*gap:\s*4px[^}]*margin-top:\s*0/,
    );
    expect(responsiveCss).toMatch(
      /\.stock-insight-board \.score-strip-primary\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\)/,
    );
    expect(pagesCss).toMatch(/\.text-no-wrap\s*\{[^}]*white-space:\s*nowrap/);
  });

  it("does not reserve an empty control-panel row above the mobile screen run button", () => {
    expect(responsiveCss).toMatch(
      /\.screen-panel-controls,\s*\.screen-panel-run-card\s*\{[^}]*min-height:\s*0[^}]*margin-top:\s*0/,
    );
  });

  it("shows all five mobile screen modes in a two-row grid", () => {
    expect(responsiveCss).toMatch(
      /\.screen-panel-container > \.screen-panel-tabs\s*\{[^}]*display:\s*grid[^}]*grid-template-columns:\s*repeat\(6, minmax\(0, 1fr\)\)[^}]*overflow-x:\s*visible[^}]*scroll-snap-type:\s*none/,
    );
    expect(responsiveCss).toMatch(
      /\.screen-panel-container > \.screen-panel-tabs \.panel-tab:nth-child\(-n \+ 3\)\s*\{[^}]*grid-column:\s*span 2/,
    );
    expect(responsiveCss).toMatch(
      /\.screen-panel-container > \.screen-panel-tabs \.panel-tab:nth-child\(n \+ 4\)\s*\{[^}]*grid-column:\s*span 3/,
    );
  });

  it("keeps agent conversation history as frameless list rows", () => {
    expect(componentsCss).toMatch(
      /\.agent-history-item\s*\{[^}]*border:\s*0[^}]*border-bottom:\s*1px solid var\(--line-soft\)[^}]*border-radius:\s*0/,
    );
    expect(componentsCss).toMatch(
      /\.agent-history-item\.active\s*\{[^}]*box-shadow:\s*inset 2px 0 var\(--agent-accent\)/,
    );
    expect(componentsCss).toMatch(
      /\.agent-history-main\s*\{[^}]*outline:\s*none[^}]*-webkit-tap-highlight-color:\s*transparent/,
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
