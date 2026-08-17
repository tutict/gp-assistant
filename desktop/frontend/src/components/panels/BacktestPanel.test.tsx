import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

let BacktestResultView: typeof import("./BacktestPanel").BacktestResultView;

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ BacktestResultView } = await import("./BacktestPanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

const result = {
  metrics: {
    total_return: 0.1,
    num_stocks: 1,
  },
  equity_curve: [],
  symbols: ["300750.SZ"],
  volatility_snapshots: [{
    symbol: "300750.SZ",
    name: "宁德时代",
    date: "2026-07-15",
    close: 268.5,
    atr: { period: 14, value: 8.2, percent_of_close: 3.054 },
    bollinger_bands: {
      period: 20,
      multiplier: 2,
      upper: 280,
      middle: 250,
      lower: 220,
      bandwidth_percent: 24,
      percent_b: 80.8333,
    },
    donchian_channel: {
      period: 20,
      upper: 285,
      middle: 250,
      lower: 215,
      width_percent: 28,
      position_percent: 76.4286,
    },
    keltner_channel: {
      ema_period: 20,
      atr_period: 10,
      multiplier: 2,
      upper: 276,
      middle: 250,
      lower: 224,
      width_percent: 20.8,
      position_percent: 85.5769,
    },
    chaikin_volatility: { ema_period: 10, roc_period: 10, value: 12.5 },
    rvi: { period: 14, value: 61.2 },
  }],
};

const unavailableResult = {
  ...result,
  volatility_snapshots: [{
    symbol: "300750.SZ",
    date: "2026-07-15",
    close: null,
    unavailable: [
      { indicator: "atr", reason: "区间末收盘价无效" },
      { indicator: "bollinger_bands", reason: "区间末收盘价无效" },
      { indicator: "donchian_channel", reason: "区间末收盘价无效" },
      { indicator: "keltner_channel", reason: "区间末收盘价无效" },
      { indicator: "chaikin_volatility", reason: "区间末收盘价无效" },
      { indicator: "rvi", reason: "区间末收盘价无效" },
    ],
  }],
};

describe("BacktestResultView volatility diagnostics", () => {
  it("identifies the portfolio curve, renders its benchmark, and scopes the symbol selector", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{
          ...result,
          equity_curve: [
            { date: "2026-07-01", equity: 1 },
            { date: "2026-07-02", equity: 1.08 },
          ],
          benchmark_curve: [
            { date: "2026-07-01", equity: 1 },
            { date: "2026-07-02", equity: 1.03 },
          ],
        }}
      />,
    );

    expect(html).toContain("组合净值曲线");
    expect(html).toContain("组合净值");
    expect(html).toContain("候选池基准");
    expect(html).toContain("波动率标的");
    expect(html).toContain('aria-label="组合与基准净值曲线"');
  });

  it("renders all six volatility indicators for the selected symbol", () => {
    const html = renderToStaticMarkup(<BacktestResultView result={result} />);

    expect(html).toContain("波动率快照");
    expect(html).toContain("ATR14");
    expect(html).toContain("布林带");
    expect(html).toContain("唐奇安通道");
    expect(html).toContain("凯尔特纳通道");
    expect(html).toContain("Chaikin 波动率");
    expect(html).toContain("RVI14");
    expect(html).toContain("宁德时代（300750.SZ）");
    expect(html).toContain("300750.SZ");
    expect(html).toContain("12.50%");
    expect(html).toContain("一句话看懂");
    expect(html).toContain("为什么这么说");
    expect(html).toContain("策略可以怎么改");
    expect(html).toContain("这些判断怎么算的");
    expect(html).toContain("48–52（含端点）为方向均衡");
    expect(html).toContain("最近上涨方向的波动明显更多");
    expect(html).toContain('<ul class="symbol-strip"');
    expect(html).toContain('<li class="symbol-chip"');
    expect(html).not.toContain("日内波幅扩张");
  });

  it("shows precise unavailable reasons instead of hiding the module", () => {
    const html = renderToStaticMarkup(<BacktestResultView result={unavailableResult} />);

    expect(html).toContain("波动率快照");
    expect(html).toContain("区间末收盘价无效");
    expect(html).toContain("有效数据还不够");
    expect(html).not.toContain("历史数据不足");
  });

  it("renders an explicit empty state when no symbol snapshot is available", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{
          ...result,
          volatility_snapshots: [],
          volatility_message: "候选快照没有符合条件的标的。",
        }}
      />,
    );

    expect(html).toContain("无可用区间末标的");
    expect(html).toContain("候选快照没有符合条件的标的");
  });

  it("does not render an empty chart frame for a single equity point", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{ ...result, equity_curve: [{ date: "2026-07-17", equity: 1.12 }] }}
      />,
    );

    expect(html).not.toContain("backtest-primary-chart");
    expect(html).toContain("有效交易日不足，暂不绘制净值曲线");
  });

  it("shows the side-by-side legacy comparison and incomplete release gates", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{
          ...result,
          metrics: { ...result.metrics, strategy_mode: "adaptive_swing_v1", annualized_return: 0.12 },
          legacy_balanced_backtest: {
            ...result,
            metrics: { ...result.metrics, strategy_mode: "walk_forward", annualized_return: 0.10 },
          },
          adaptive_release_gate: {
            passed: false,
            checks: [
              {
                key: "annualized_return_delta",
                passed: true,
                actual: 0.02,
                requirement: "adaptive annualized return no worse than legacy by more than 1 percentage point",
              },
              {
                key: "cached_run_millis",
                passed: false,
                actual: null,
                requirement: "same-day cached run <= 2000 ms",
              },
            ],
          },
        }}
      />,
    );

    expect(html).toContain("自适应波段");
    expect(html).toContain("同样本旧 balanced 年化");
    expect(html).toContain("暂不切换默认");
    expect(html).toContain("缓存运行耗时");
    expect(html).toContain("待采集");
    expect(html).toContain('class="backtest-gate-status passed"');
    expect(html).toContain('class="backtest-gate-status failed"');
  });
});
