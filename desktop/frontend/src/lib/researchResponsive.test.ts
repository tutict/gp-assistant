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
