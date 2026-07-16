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
  it("renders all six volatility indicators for the selected symbol", () => {
    const html = renderToStaticMarkup(<BacktestResultView result={result} />);

    expect(html).toContain("波动率快照");
    expect(html).toContain("ATR14");
    expect(html).toContain("布林带");
    expect(html).toContain("唐奇安通道");
    expect(html).toContain("凯尔特纳通道");
    expect(html).toContain("Chaikin 波动率");
    expect(html).toContain("RVI14");
    expect(html).toContain("300750.SZ");
    expect(html).toContain("12.50%");
    expect(html).not.toContain("日内波幅扩张");
  });

  it("shows precise unavailable reasons instead of hiding the module", () => {
    const html = renderToStaticMarkup(<BacktestResultView result={unavailableResult} />);

    expect(html).toContain("波动率快照");
    expect(html).toContain("区间末收盘价无效");
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
});
