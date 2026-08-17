import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);
const stylesDirectory = new URL("../styles/", import.meta.url);
const pagesCss = readFileSync(new URL("pages.css", stylesDirectory), "utf8");
const componentsCss = readFileSync(new URL("components.css", stylesDirectory), "utf8");
const responsiveCss = readFileSync(new URL("responsive.css", stylesDirectory), "utf8");
const allCriteriaCss = `${componentsCss}\n${pagesCss}\n${responsiveCss}`;

describe("screen criteria layout contract", () => {
  it("styles every criteria container class used by the shared form", () => {
    for (const selector of [
      ".custom-screen-controls",
      ".custom-screen-criteria",
      ".criteria-field-group",
      ".criteria-field-grid",
      ".criteria-toggle-row",
      ".criteria-num-field",
    ]) {
      expect(allCriteriaCss).toContain(selector);
    }
  });

  it("keeps criteria group headers split into title and description rows", () => {
    expect(componentsCss).toMatch(
      /\.criteria-field-group > header\s*\{[^}]*display:\s*grid[^}]*gap:\s*3px/,
    );
    expect(componentsCss).toMatch(
      /\.criteria-field-group > header > strong\s*\{[^}]*font-size:\s*var\(--fs-label\)/,
    );
    expect(componentsCss).toMatch(
      /\.criteria-field-group > header > span\s*\{[^}]*font-size:\s*var\(--fs-caption\)/,
    );
  });

  it("keeps custom screen criteria in desktop columns with a mobile collapse", () => {
    expect(pagesCss).toMatch(
      /@media \(min-width: 1100px\)[\s\S]*?\.custom-screen-criteria\s*\{[^}]*grid-template-columns:\s*1\.1fr 1\.6fr 1\.4fr/,
    );
    expect(responsiveCss).toMatch(
      /@media \(max-width: 768px\)[\s\S]*?\.custom-screen-criteria\s*\{[^}]*grid-template-columns:\s*1fr/,
    );
    expect(responsiveCss).toMatch(
      /@media \(max-width: 768px\)[\s\S]*?\.criteria-field-grid\s*\{[^}]*grid-template-columns:\s*repeat\(2, minmax\(0, 1fr\)\)/,
    );
  });

  it("gives adaptive advanced filters disclosure affordance and open state", () => {
    expect(pagesCss).toMatch(/\.adaptive-advanced\s*\{[^}]*border:\s*1px solid var\(--line\)/);
    expect(pagesCss).toMatch(/\.adaptive-advanced > summary::before\s*\{[^}]*content:\s*"›"/);
    expect(pagesCss).toMatch(/\.adaptive-advanced\[open\] > summary::before\s*\{[^}]*transform:\s*rotate\(90deg\)/);
  });

  it("overrides shared touch-height controls for dense desktop criteria forms", () => {
    expect(pagesCss).toMatch(
      /\.screen-panel-controls \.criteria-field-grid \.form-row input,[\s\S]*?\.screen-panel-controls \.criteria-field-grid \.form-row select\s*\{[^}]*min-height:\s*var\(--control-height\)/,
    );
    expect(pagesCss).toMatch(
      /\.screen-panel-controls \.criteria-num-field input\s*\{[^}]*width:\s*132px/,
    );
    expect(responsiveCss).toMatch(
      /@media \(max-width: 768px\)[\s\S]*?\.criteria-field-grid \.form-row input,[\s\S]*?\.adaptive-advanced > summary\s*\{[^}]*min-height:\s*var\(--touch-comfort\)/,
    );
  });
});
