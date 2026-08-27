import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const researchCss = readFileSync(new URL("../styles/research.css", import.meta.url), "utf8");
const newsPanel = readFileSync(
  new URL("../components/panels/NewsRagPanel.tsx", import.meta.url),
  "utf8",
);
const screenshotHarness = readFileSync(
  new URL("../../../../scripts/ui-screenshot.mjs", import.meta.url),
  "utf8",
);
const frontendPackage = readFileSync(new URL("../../package.json", import.meta.url), "utf8");

function withoutComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

function ruleBody(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = source.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`));
  expect(match).toBeTruthy();
  return match?.[1] ?? "";
}

function sourceTierToneRules(): string[] {
  return [...withoutComments(researchCss).matchAll(/\.research-source-tier\.([a-z_]+)\s*\{([^}]*)\}/g)]
    .filter((match) => /\bcolor\s*:/.test(match[2]))
    .map((match) => match[1])
    .sort();
}

function mobileNewsCss(): string {
  const marker = "@media (max-width: 768px)";
  const start = researchCss.indexOf(marker);
  expect(start).toBeGreaterThanOrEqual(0);
  const next = researchCss.indexOf("@media (max-width: 420px)", start);
  expect(next).toBeGreaterThan(start);
  return withoutComments(researchCss.slice(start, next));
}

describe("news page CSS contract", () => {
  it("keeps research.css typography fully tokenized", () => {
    expect(withoutComments(researchCss)).not.toMatch(/font-size:\s*\d+(?:\.\d+)?px/);
    expect(ruleBody(researchCss, ".research-context h1")).toContain("font-size: var(--fs-headline)");
  });

  it("keeps the event empty state actionable", () => {
    expect(researchCss).toContain(".research-empty {");
    expect(researchCss).toContain(".research-empty-actions button");
    expect(newsPanel).toContain('className="research-empty"');
    expect(newsPanel).toContain('className="research-empty-primary"');
    expect(newsPanel).toContain('className="research-empty-ghost"');
    expect(newsPanel).toContain("onRefresh");
    expect(newsPanel).toContain("onKnowledge");
  });

  it("keeps mobile topbar buttons visually lighter than the touch target", () => {
    const mobile = mobileNewsCss();
    expect(mobile).not.toMatch(/(?:width|height|min-width|min-height):\s*44px/);
    expect(ruleBody(mobile, ".research-actions button")).toContain("border: 0");
    expect(ruleBody(mobile, ".research-icon-button")).toContain("border: 0");
    const touchTarget = "calc(var(--touch-dense) + var(--space-2) + var(--space-1))";
    expect(ruleBody(mobile, ".research-actions button")).toContain(touchTarget);
    expect(ruleBody(mobile, ".research-icon-button")).toContain(touchTarget);
    expect(mobile).toMatch(/\.research-actions button::before,[\s\S]*?inset:\s*var\(--space-1\)/);
  });

  it("keeps the full risk boundary permanently rendered", () => {
    expect(newsPanel).toContain('<p className="research-risk-boundary">仅供研究，不构成投资建议。</p>');
    expect(newsPanel).not.toContain("!mobile && <p className=\"research-risk-boundary\"");
  });

  it("keeps source badges neutral and tokenized", () => {
    expect(sourceTierToneRules()).toEqual([]);
    expect(researchCss).not.toContain("var(--score)");
    expect(researchCss).toContain(".research-badge {");
  });
});

describe("news page screenshot contract", () => {
  it("registers empty/data desktop and phone news baselines plus mobile overlays", () => {
    expect(screenshotHarness).toContain("--news-page-only");
    expect(screenshotHarness).toContain("--check-news-page");
    expect(screenshotHarness).toContain("captureNewsPageBaselines");
    expect(screenshotHarness).toContain("installFixedResearchClock");
    expect(frontendPackage).toContain("--check-news-page");
    for (const device of [
      "desktop-1440-dark",
      "desktop-1440-light",
      "desktop-1920-dark",
      "desktop-1920-light",
      "phone-390-dark",
      "phone-390-light",
    ]) {
      expect(screenshotHarness).toContain(`name: "${device}"`);
    }
    for (const marker of [
      'name: "empty"',
      'name: "data"',
      'name: "phone-inbox-open"',
      'name: "phone-evidence-open"',
      'name: "event-expanded"',
      'name: "evidence-history"',
      'name: "delete-confirmation"',
      'name: "skeleton-loading"',
      "news-page-report.json",
    ]) {
      expect(screenshotHarness).toContain(marker);
    }
  });
});
