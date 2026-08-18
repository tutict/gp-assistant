import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const pagesCss = readFileSync(new URL("../styles/pages.css", import.meta.url), "utf8");
const responsiveCss = readFileSync(new URL("../styles/responsive.css", import.meta.url), "utf8");
const backtestPanel = readFileSync(
  new URL("../components/panels/BacktestPanel.tsx", import.meta.url),
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

function ruleBodies(source: string, selectorFragment: string): string[] {
  return [...withoutComments(source).matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((match) => match[1].includes(selectorFragment))
    .map((match) => match[2]);
}

function ruleBody(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = withoutComments(source).match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`));
  expect(match).toBeTruthy();
  return match?.[1] ?? "";
}

function backtestRuleBodies(source: string): string[] {
  const selectorPattern = /(?:backtest-(?:context|run-button|controls|result|primary-chart|chart-empty|comparison|holdings|fold|param-strip|gate-status)|equity-chart|volatility|symbol-(?:strip|chip)|metric-strip|backtest-result\s*>\s*\.(?:notes|raw-json))/;
  return [...withoutComments(source).matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((match) => selectorPattern.test(match[1]))
    .map((match) => match[2]);
}

describe("backtest page CSS contract", () => {
  it("uses a chart-series token for the portfolio curve", () => {
    const equityChart = ruleBody(pagesCss, ".equity-chart");
    expect(equityChart).not.toContain("--equity-portfolio: var(--score)");
    expect(equityChart).not.toContain("--equity-portfolio: var(--accent)");
    expect(equityChart).toContain("--equity-portfolio: var(--chart-line-1)");
  });

  it("uses typography tokens throughout backtest and volatility rules", () => {
    const relevantCss = [...backtestRuleBodies(pagesCss), ...backtestRuleBodies(responsiveCss)].join("\n");
    expect(relevantCss).not.toMatch(/font-size:\s*\d+(?:\.\d+)?(?:px|rem)/);
  });

  it("keeps every backtest-owned selector inside the typography audit", () => {
    const probe = [
      ".backtest-gate-status { font-size: 11px; }",
      ".symbol-chip { font-size: 11px; }",
      ".backtest-result > .notes p { font-size: 11px; }",
      ".backtest-result > .raw-json > summary { font-size: 11px; }",
    ].join("\n");
    expect(backtestRuleBodies(probe)).toHaveLength(4);
  });

  it("keeps the interpretation summary neutral", () => {
    const summary = ruleBody(pagesCss, ".volatility-interpretation-summary");
    const colors = [...summary.matchAll(/(?:^|;)\s*color\s*:\s*([^;]+)/g)]
      .map((match) => match[1].trim());
    expect(colors).toEqual(["var(--text)"]);
  });

  it("removes card chrome from the comparison stat band", () => {
    const comparisonRules = ruleBodies(pagesCss, ".backtest-comparison").join("\n");
    expect(comparisonRules).not.toContain("border-radius");
    expect(ruleBody(pagesCss, ".backtest-comparison")).toContain("display: flex");
  });

  it("stacks comparison labels above their values", () => {
    expect(ruleBody(pagesCss, ".backtest-comparison > div > span")).toContain("display: block");
    expect(ruleBody(pagesCss, ".backtest-comparison > div > strong")).toContain("display: block");
  });

  it("uses status colors for release gates instead of market-direction colors", () => {
    expect(ruleBody(pagesCss, ".backtest-gate-status.passed::before")).toContain("var(--success)");
    expect(ruleBody(pagesCss, ".backtest-gate-status.failed::before")).toContain("var(--error)");
  });

  it("keeps small light-theme labels on the accessible secondary text token", () => {
    for (const selector of [
      ".backtest-param-strip",
      ".backtest-param-strip b",
      ".backtest-result > .metric-strip .metric > span",
      ".equity-chart .chart-labels span:nth-child(2)",
      ".volatility-symbol-control > span",
      ".volatility-grid span",
      ".volatility-interpretation > header span",
      ".volatility-interpretation-grid h4",
      ".volatility-interpretation-note",
      ".volatility-interpretation-method",
    ]) {
      const bodies = ruleBodies(pagesCss, selector);
      expect(bodies.length).toBeGreaterThan(0);
      expect(bodies.join("\n")).toContain("color: var(--text-secondary)");
    }
    expect(ruleBodies(pagesCss, ".equity-chart-grid text").join("\n"))
      .toContain("fill: var(--text-secondary)");
  });

  it("keeps the return hero below the display typography tier", () => {
    const mobileHeroRules = ruleBodies(
      responsiveCss,
      ".backtest-result > .metric-strip .metric-hero > strong",
    ).join("\n");
    expect(mobileHeroRules).not.toContain("font-size: var(--fs-display)");
    expect(mobileHeroRules).toContain("font-size: var(--fs-headline)");
    const desktopHero = ruleBody(pagesCss, ".backtest-result > .metric-strip .metric-hero > strong");
    expect(desktopHero).not.toContain("font-size: var(--fs-display)");
    expect(desktopHero).toContain("font-size: var(--fs-headline)");
  });
});

describe("backtest page markup contract", () => {
  it("marks the return hero, symbol chips, release status, and directional volatility values", () => {
    expect(backtestPanel).toContain('className="metric metric-hero"');
    expect(backtestPanel).toContain('className="symbol-chip"');
    expect(backtestPanel).toContain("backtest-gate-status");
    expect(backtestPanel).toContain("volatility-value");
    expect(backtestPanel).toContain('tone: chaikin?.value');
  });

  it("renders the symbol chips as a semantic list", () => {
    expect(backtestPanel).toContain('<ul className="symbol-strip"');
    expect(backtestPanel).toContain('<li className="symbol-chip"');
  });
});

describe("backtest page screenshot contract", () => {
  it("registers runtime color audits and dark/light desktop and phone baselines", () => {
    expect(screenshotHarness).toContain("--backtest-page-only");
    expect(screenshotHarness).toContain("captureBacktestPageBaselines");
    expect(screenshotHarness).toContain("assertBacktestRuntimeColors");
    expect(frontendPackage).toContain("ui:backtest");
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
    expect(screenshotHarness).toContain('selector: ".backtest-volatility"');
    expect(screenshotHarness).toContain("backtest-page-report.json");
    expect(screenshotHarness).toContain("adaptive_release_gate:");
    expect(screenshotHarness).toContain('styleFor(".backtest-gate-status.passed::before", "backgroundColor")');
    expect(screenshotHarness).toContain('styleFor(".backtest-gate-status.failed::before", "backgroundColor")');
    expect(screenshotHarness).toContain('styleFor(".metric-strip .metric > span")');
    expect(screenshotHarness).toContain('styleFor(".metric-strip .metric-hero > strong", "fontSize")');
    expect(screenshotHarness).toContain('chartLine1: resolvedToken("--chart-line-1")');
    expect(screenshotHarness).toContain('headline: getComputedStyle(document.documentElement).getPropertyValue("--fs-headline").trim()');
    expect(screenshotHarness).toContain('portfolioCurve: styleFor(".equity-chart-line.is-portfolio", "stroke")');
    expect(screenshotHarness).toContain("portfolioCurve: positive.tokens.chartLine1");
    expect(screenshotHarness).toContain("heroFontSize: positive.tokens.headline");
  });

  it("hides fixed mobile navigation before the full-page backtest capture", () => {
    const captureStart = screenshotHarness.indexOf("async function captureBacktestPageBaselines");
    const captureEnd = screenshotHarness.indexOf("async function captureObserveSummaryBaselines", captureStart);
    const captureSource = screenshotHarness.slice(captureStart, captureEnd);
    const hideNavigation = captureSource.indexOf('.sidebar { visibility: hidden !important; }');
    const fullPageCapture = captureSource.indexOf('"backtest.png"), fullPage: true');
    expect(hideNavigation).toBeGreaterThanOrEqual(0);
    expect(hideNavigation).toBeLessThan(fullPageCapture);
  });

  it("writes canonical backtest baselines to a stable tracked directory", () => {
    expect(frontendPackage).toContain("--output ./test-baselines");
    expect(frontendPackage).toContain("--stable-report");
  });
});
