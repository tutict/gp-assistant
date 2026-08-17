import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ObserveResult } from "../types";
import { reasonLabel } from "./format";
type ObservePanelModule = typeof import("../components/panels/ObservePanel");

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const stylesDirectory = new URL("../styles/", import.meta.url);
const pagesCss = readFileSync(new URL("pages.css", stylesDirectory), "utf8");
const responsiveCss = readFileSync(new URL("responsive.css", stylesDirectory), "utf8");
const nativeCore = readFileSync(new URL("../../../../native/gp-core/src/lib.rs", import.meta.url), "utf8");
let ObserveResultView: ObservePanelModule["ObserveResultView"];
let formatObserveTime: ObservePanelModule["formatObserveTime"];
let patternSignalLabel: ObservePanelModule["patternSignalLabel"];
let riskFlagLabel: ObservePanelModule["riskFlagLabel"];
let signalTypeLabel: ObservePanelModule["signalTypeLabel"];

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({
    ObserveResultView,
    formatObserveTime,
    patternSignalLabel,
    riskFlagLabel,
    signalTypeLabel,
  } = await import("../components/panels/ObservePanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

afterEach(() => {
  vi.restoreAllMocks();
});

function observeCssSegment(): string {
  const start = pagesCss.indexOf("/* Observe summary: unified fact bands and neutral rhythm. */");
  const end = pagesCss.indexOf(".observe-detail-disclosure {", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return pagesCss.slice(start, end).replace(/\/\*[\s\S]*?\*\//g, "");
}

function observeDetailCssSegment(): string {
  const start = pagesCss.indexOf(".observe-detail-disclosure {");
  const end = pagesCss.indexOf("/* Observe capital quant:", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return pagesCss.slice(start, end).replace(/\/\*[\s\S]*?\*\//g, "");
}

function cssRule(css: string, selector: string): string {
  const start = css.lastIndexOf(selector);
  expect(start).toBeGreaterThanOrEqual(0);
  const end = css.indexOf("}", start);
  expect(end).toBeGreaterThan(start);
  return css.slice(start, end + 1);
}

function nativeStringSet(startMarker: string, endMarker: string): string[] {
  const start = nativeCore.indexOf(startMarker);
  const end = nativeCore.indexOf(endMarker, start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return [...nativeCore.slice(start, end).matchAll(/(?:push|return|default)[^\n]*?"([a-z][a-z0-9_:-]+)"/g)]
    .map((match) => match[1])
    .filter((value, index, values) => values.indexOf(value) === index);
}

function baseObserveResult(overrides: Partial<ObserveResult> = {}): ObserveResult {
  const stock = {
    code: "000001.SZ",
    name: "平安银行",
    industry: "银行",
    price: 10.12,
    quote_time: "20260814161448",
    pe: 6.5,
    pb: 0.7,
    roe: 0.11,
  };
  const signal = {
    code: stock.code,
    date: "20260814",
    close: 10.12,
    previous_close: 10,
    close_change: 0.12,
    close_change_pct: 0.012,
    support: 9.8,
    resistance: 10.8,
    swl: 10.05,
    sws: 9.92,
    swl_above_sws: true,
    status: "neutral",
    signal_type: "trend_continuation",
    risk_flags: ["low_volume"],
    pattern_signals: ["bottom_accumulation"],
    reasons: ["signal_type:trend_continuation", "ma_bull_stack"],
    quant_score: 72,
    quant_score_max: 100,
    pattern_score: 66,
    pattern_score_max: 100,
  };
  return {
    source: "contract-test",
    stock,
    trend: {
      stock,
      signal,
      series: [
        { date: "20260813", close: 10, volume: 1000, swl: 9.95, sws: 9.9 },
        { date: "20260814", close: 10.12, volume: 1200, swl: 10.05, sws: 9.92 },
      ],
    },
    capital_evidence: {
      stock_code: stock.code,
      as_of_trade_date: "20260814",
      items: [
        { category: "fund_flow_status", title: "接口暂不可用", metrics: { 状态: "暂不可用" } },
        {
          category: "fund_flow",
          title: "量价资金代理",
          source: "Tauri/Rust 本地量价资金代理",
          confidence: "中",
          score: 55,
          metrics: {
            证据类型: "本地日线量价代理",
            推断方向: "中性",
            量价热度: "55",
            吸筹强度: "52",
            趋势热度: "58",
          },
        },
      ],
      sections: [],
    },
    ...overrides,
  };
}

function renderObserveSummary(result: ObserveResult): string {
  const html = renderToStaticMarkup(createElement(ObserveResultView, { result }));
  const start = html.indexOf('<section class="observe-text-metrics observe-decision-summary"');
  const disclaimer = html.indexOf('class="observe-metric-analysis-disclaimer"', start);
  const end = html.indexOf("</p></section>", disclaimer) + "</p></section>".length;
  expect(start).toBeGreaterThanOrEqual(0);
  expect(disclaimer).toBeGreaterThan(start);
  expect(end).toBeGreaterThan(disclaimer);
  return html.slice(start, end);
}

describe("observe summary layout contract", () => {
  it("keeps the observe summary CSS tokenized and limited to the two dot markers", () => {
    const segment = observeCssSegment();
    expect(segment).not.toMatch(/font-size:\s*\d+(?:\.\d+)?px/);
    expect(segment).not.toMatch(/box-shadow/);

    const beforeSelectors = [...segment.matchAll(/([^{}]+)::before\s*\{/g)]
      .map((match) => match[1].trim().replace(/\s+/g, " "));
    expect(beforeSelectors).toEqual([".observe-verdict", ".observe-special-quant-item > span"]);
  });

  it("keeps the next-signal label neutral", () => {
    expect(cssRule(pagesCss, ".observe-next-signal > span")).not.toContain("var(--accent)");
    expect(cssRule(pagesCss, ".observe-next-signal > p")).toContain("-webkit-line-clamp: unset");
  });

  it("keeps professional details on the existing disclosure treatment", () => {
    const detail = observeDetailCssSegment();
    expect(detail).toContain(".observe-detail-disclosure > summary > span::before");
    expect(detail).not.toContain(".observe-detail-disclosure > summary > span::after");
    expect(detail).toContain("min-height: 40px");
    expect(detail).toContain("min-height: 34px");
    expect(detail).toContain("column-gap: 22px");
  });

  it("uses one bottom boundary for the capital lanes", () => {
    expect(cssRule(pagesCss, ".capital-quant-lanes")).not.toContain("border-top");
  });

  it("keeps mobile observe bands collapsed without platform classes", () => {
    expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.observe-decision-grid\s*\{[^}]*grid-template-columns:\s*1fr/);
    expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.observe-special-quant\s*\{[^}]*grid-template-columns:\s*repeat\(2, minmax\(0, 1fr\)\)/);
    expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.observe-key-metrics\s*\{[^}]*grid-template-columns:\s*repeat\(4, minmax\(0, 1fr\)\)/);
    expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.capital-quant-lanes\s*\{[^}]*grid-template-columns:\s*1fr/);
    expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.capital-main-flow \.capital-quant-metrics\s*\{[^}]*grid-template-columns:\s*repeat\(3, minmax\(0, 1fr\)\)/);
    expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.observe-next-signal\s*\{[^}]*grid-template-columns:\s*1fr/);
  });
});

describe("observe summary copy contract", () => {
  it("formats compact quote timestamps for display", () => {
    expect(formatObserveTime("20260814161448")).toBe("2026-08-14 16:14");
    expect(formatObserveTime("20260814")).toBe("2026-08-14");
    expect(formatObserveTime("2026-08-14 16:14:48")).toBe("2026-08-14 16:14");
  });

  it("covers backend enum labels and warns without leaking unknown values", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    expect(signalTypeLabel("trend_continuation")).toBe("趋势延续");
    expect(signalTypeLabel("risk_warning")).toBe("风险预警");
    expect(riskFlagLabel("macd_bearish_divergence")).toBe("MACD 顶背离");
    expect(patternSignalLabel("dragon_trend_volume")).toBe("趋势量能共振");
    expect(signalTypeLabel("unknown_signal_case")).toBe("未识别类型");
    expect(warn).toHaveBeenCalledWith("[observe-summary] unmapped signal_type: unknown_signal_case");
  });

  it("keeps every native trend enum in a translated frontend map", () => {
    const signalTypes = nativeStringSet("fn classify_trend_signal", "fn layered_reason_tags");
    const riskFlags = [
      ...nativeStringSet("fn trend_layer_risk_flags", "fn closes_from_points"),
      ...nativeStringSet("fn trend_risk_flags_from_bar", "fn closes_from_points"),
    ].filter((value, index, values) => values.indexOf(value) === index);
    const patternSignals = nativeStringSet("fn pattern_signals", "fn trend_status");
    const reasons = nativeStringSet("fn trend_reasons", "fn pattern_score");

    for (const value of ["trend_continuation", "trend_reversal", "range_bound", "breakout_attempt"])
      expect(signalTypeLabel(value)).not.toBe("未识别类型");
    for (const value of signalTypes) expect(signalTypeLabel(value)).not.toBe("未识别类型");
    for (const value of riskFlags) expect(riskFlagLabel(value)).not.toBe("未识别类型");
    for (const value of patternSignals) expect(patternSignalLabel(value)).not.toBe("未识别类型");
    for (const value of reasons) {
      expect(reasonLabel(value)).not.toBe(value);
    }
  });

  it("renders the summary without snake_case, raw timestamps, or SWL/SWS copy", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const result = baseObserveResult({
      trend: {
        ...baseObserveResult().trend!,
        signal: {
          ...baseObserveResult().trend!.signal,
          signal_type: "unknown_signal_case",
          risk_flags: ["unknown_risk_case"],
          pattern_signals: ["unknown_pattern_case"],
          reasons: ["unknown_reason_case"],
        },
      },
    });

    const summary = renderObserveSummary(result);
    expect(summary).not.toMatch(/\b[a-z]+_[a-z_]+\b/);
    expect(summary).not.toContain("20260814161448");
    expect(summary).not.toMatch(/SWL|SWS/);
    expect(warn).toHaveBeenCalled();
  });

  it("folds unavailable main-fund-flow metrics into a single notice and note details", () => {
    const summary = renderObserveSummary(baseObserveResult());
    expect(summary).toContain("主力资金接口暂不可用，以下为本地量价代理估算。");
    expect(summary).toContain("口径说明");
    expect(summary).not.toContain("主力净流入额</dt><dd>暂缺</dd>");
  });

  it("does not claim a proxy when capital evidence has no proxy item", () => {
    const result = baseObserveResult({
      capital_evidence: { stock_code: "000001.SZ", items: [], sections: [] },
    });
    const summary = renderObserveSummary(result);
    expect(summary).toContain("当前没有可用的量价代理估算。");
    expect(summary).not.toContain("以下为本地量价代理估算");
    expect(summary).not.toContain("扣非增长 --");
  });

  it("reads the native financial keys in the summary analysis", () => {
    const result = baseObserveResult({
      financial_indicators: {
        period: "2026Q2",
        source: "Tauri/Rust",
        items: [
          { metric_key: "latest_eps", label: "最新每股收益", value: "1.67元" },
          { label: "扣非净利润增长率", value: "20.59%" },
        ],
      },
    });
    const summary = renderObserveSummary(result);
    expect(summary).toContain("财务侧 EPS 1.67元、扣非增长 20.59%。");
    expect(summary).not.toContain("EPS 暂缺");
    expect(summary).not.toContain("扣非增长 暂缺");
  });
});
