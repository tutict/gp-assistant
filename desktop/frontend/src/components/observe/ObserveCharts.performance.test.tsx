import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ObserveResult, TrendIndicatorPoint } from "../../types";

function buildSeries(length = 2_000): TrendIndicatorPoint[] {
  const start = Date.UTC(2020, 0, 1);
  return Array.from({ length }, (_, index) => {
    const close = 10 + index * 0.01 + Math.sin(index / 9);
    return {
      date: new Date(start + index * 86_400_000).toISOString().slice(0, 10),
      open: close - 0.1, high: close + 0.5, low: close - 0.5, close,
      volume: 1_000_000 + index * 100,
      volume_price_heat: 50, accumulation_strength: 50, accumulation_index: 0,
      trend_heat: 50, rebound_signal: 50,
    };
  });
}

describe("observation interaction computation", () => {
  let renderer: ReactTestRenderer | undefined;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("window", { location: { href: "http://localhost/" } });
    vi.stubGlobal("document", {
      addEventListener: () => undefined, removeEventListener: () => undefined,
      documentElement: { classList: { toggle: () => undefined, remove: () => undefined } },
    });
  });

  afterEach(async () => {
    await act(async () => renderer?.unmount());
    renderer = undefined;
    vi.unstubAllGlobals();
  });

  it("changes the viewport without rereading the complete input history", async () => {
    const { TrendCharts } = await import("./ObserveCharts");
    let reads = 0;
    const series = buildSeries().map((point) => ({ ...point, get close() { reads += 1; return point.close; } }));
    await act(async () => { renderer = create(<TrendCharts series={series} />); });
    reads = 0;

    await act(async () => renderer!.root.findByProps({ "aria-label": "放大K线" }).props.onClick());

    expect(renderer!.root.findAllByProps({ className: "kline-body" }).length).toBeLessThan(96);
    expect(reads).toBe(0);
  });

  it("shows a crosshair without recalculating indicators over the complete history", async () => {
    const { TrendCharts } = await import("./ObserveCharts");
    await act(async () => { renderer = create(<TrendCharts series={buildSeries()} />); });
    const chart = renderer!.root.find((node) => typeof node.type === "function" && Array.isArray(node.props.allBars));
    let reads = 0;
    for (const bar of chart.props.allBars) {
      const close = bar.close;
      Object.defineProperty(bar, "close", { enumerable: true, get: () => { reads += 1; return close; } });
    }
    const target = {
      getBoundingClientRect: () => ({ left: 0, width: 1180 }),
      setPointerCapture: () => undefined,
      hasPointerCapture: () => true,
      releasePointerCapture: () => undefined,
    };
    const pointer = { pointerId: 1, clientX: 200, clientY: 100, currentTarget: target, preventDefault: () => undefined };
    const svg = renderer!.root.find((node) => node.type === "svg" && typeof node.props.onPointerDown === "function");

    await act(async () => { svg.props.onPointerDown(pointer); svg.props.onPointerUp(pointer); });

    expect(renderer!.root.findAllByProps({ className: "kline-inspector" })).toHaveLength(1);
    // The 96 visible candles may be read again; the 2,000 historical candles must not be.
    expect(reads).toBeLessThan(1_000);
  });

  it("refreshes prices and indicators when a new history snapshot arrives", async () => {
    const { TrendCharts } = await import("./ObserveCharts");
    const series = buildSeries();
    await act(async () => { renderer = create(<TrendCharts series={series} />); });
    const updated = [...series.slice(0, -1), { ...series.at(-1)!, open: 87, high: 89, low: 86, close: 88 }];

    await act(async () => renderer!.update(<TrendCharts series={updated} />));

    const priceGroup = renderer!.root.find((node) => node.type === "g" && String(node.props.className).startsWith("kline-last-price "));
    const price = priceGroup.findByType("text");
    expect(price.children.join("")).toContain("88.00");
  });

  it("opens professional details without rerunning full-history quantitative analysis", async () => {
    const { ObserveResultView } = await import("../panels/ObservePanel");
    let reads = 0;
    const series = buildSeries().map((point) => ({ ...point, get close() { reads += 1; return point.close; } }));
    const stock = { code: "600519.SH", name: "贵州茅台", industry: "白酒", price: 30 };
    const result: ObserveResult = {
      source: "tdx", stock,
      trend: { stock, signal: { code: stock.code, date: "2026-10-30", close: 30, k: 50, d: 50, j: 50 }, series },
    };
    await act(async () => { renderer = create(<ObserveResultView result={result} mobileRuntime />); });
    reads = 0;

    await act(async () => renderer!.root.findByProps({ className: "observe-detail-disclosure" }).props.onToggle({ currentTarget: { open: true } }));

    expect(renderer!.root.findByProps({ className: "observe-detail-disclosure" }).props.open).toBe(true);
    expect(reads).toBeLessThan(20);
  });
});

describe("mobile chart instrument toolbar", () => {
  it("groups period and chart actions inside one instrument rail", async () => {
    const { TrendCharts } = await import("./ObserveCharts");
    const markup = renderToStaticMarkup(<TrendCharts series={buildSeries(120)} />);
    const toolbarIndex = markup.indexOf('class="kline-instrument-toolbar"');
    const periodsIndex = markup.indexOf('class="kline-period-tabs"', toolbarIndex);
    const actionsIndex = markup.indexOf('class="kline-nav"', periodsIndex);
    const toolbarEnd = markup.indexOf('</div>', actionsIndex);

    expect(toolbarIndex).toBeGreaterThanOrEqual(0);
    expect(periodsIndex).toBeGreaterThan(toolbarIndex);
    expect(actionsIndex).toBeGreaterThan(periodsIndex);
    expect(toolbarEnd).toBeGreaterThan(actionsIndex);
    expect(markup).toContain('aria-label="K线周期"');
    expect(markup).toContain('aria-label="K线视图工具"');
    expect(markup).toContain('aria-label="放大K线"');
    expect(markup).toContain('aria-label="缩小K线"');
    expect(markup).toContain('aria-label="回到最新K线"');
    expect(markup).toMatch(/aria-label="(?:全屏|横屏全屏)"/);
  });
});
