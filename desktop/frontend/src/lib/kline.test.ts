import { describe, expect, it } from "vitest";
import { aggregateBars, buildMacdScale, computeKdj, computeMacd, marketDirection, toDailyBars, type KlineBar } from "./kline";
import type { TrendIndicatorPoint } from "../types";

const bars: KlineBar[] = [
  { date: "2026-01-02", open: 10, high: 12, low: 9, close: 11, volume: 100 },
  { date: "2026-01-05", open: 11, high: 13, low: 10, close: 12, volume: 110 },
  { date: "2026-01-06", open: 12, high: 14, low: 11, close: 13, volume: 120 },
  { date: "2026-02-02", open: 13, high: 16, low: 12, close: 15, volume: 130 },
];

describe("kline helpers", () => {
  it("classifies A-share chart direction without treating flat values as gains", () => {
    expect(marketDirection(10.01, 10)).toBe("up");
    expect(marketDirection(9.99, 10)).toBe("down");
    expect(marketDirection(10, 10)).toBe("flat");
    expect(marketDirection(10, null)).toBe("flat");
  });

  it("normalizes daily trend points into complete OHLC bars only", () => {
    const series: TrendIndicatorPoint[] = [
      { date: "2026-01-02", open: 10, high: 12, low: 9, close: 11, volume: 100 },
      { date: "2026-01-05", high: 13, low: 10, close: 12, volume: 110 },
    ];

    expect(toDailyBars(series)).toEqual([bars[0]]);
  });

  it("sorts upstream daily bars chronologically before indicator calculation", () => {
    const series = [bars[2], bars[0], bars[1]] as TrendIndicatorPoint[];

    expect(toDailyBars(series).map((bar) => bar.date)).toEqual([
      "2026-01-02",
      "2026-01-05",
      "2026-01-06",
    ]);
  });

  it("aggregates weekly and monthly bars from full daily history", () => {
    expect(aggregateBars(bars, "weekly")).toEqual([
      bars[0],
      { date: "2026-01-06", open: 11, high: 14, low: 10, close: 13, volume: 230 },
      bars[3],
    ]);

    expect(aggregateBars(bars, "monthly")).toEqual([
      { date: "2026-01-06", open: 10, high: 14, low: 9, close: 13, volume: 330 },
      bars[3],
    ]);
  });

  it("computes one KDJ point per aggregated bar", () => {
    const kdj = computeKdj(aggregateBars(bars, "monthly"));

    expect(kdj).toHaveLength(2);
    expect(kdj[1].date).toBe("2026-02-02");
    expect(Number.isFinite(kdj[1].k)).toBe(true);
    expect(Number.isFinite(kdj[1].d)).toBe(true);
    expect(Number.isFinite(kdj[1].j)).toBe(true);
  });

  it("computes MACD values aligned to each bar", () => {
    const macd = computeMacd(bars);

    expect(macd).toHaveLength(bars.length);
    expect(macd[3].date).toBe("2026-02-02");
    expect(Number.isFinite(macd[3].dif)).toBe(true);
    expect(Number.isFinite(macd[3].dea)).toBe(true);
    expect(Number.isFinite(macd[3].macd)).toBe(true);
    expect(macd[3].dif).toBeGreaterThan(0);
    expect(macd[3].dea).toBeGreaterThan(0);
  });

  it("matches the standard Chinese-market MACD formula", () => {
    const fixture = [10, 11, 12].map((close, index) => ({
      date: `2026-01-0${index + 1}`,
      open: close,
      high: close,
      low: close,
      close,
      volume: 100,
    }));

    const macd = computeMacd(fixture, 2, 3, 2);
    expect(macd[1].dif).toBeCloseTo(1 / 6, 10);
    expect(macd[1].dea).toBeCloseTo(1 / 9, 10);
    expect(macd[1].macd).toBeCloseTo(1 / 9, 10);
    expect(macd[2].dif).toBeCloseTo(11 / 36, 10);
    expect(macd[2].dea).toBeCloseTo(13 / 54, 10);
    expect(macd[2].macd).toBeCloseTo(7 / 54, 10);
  });

  it("uses one shared vertical scale for DIF, DEA, and MACD histogram", () => {
    const macd = computeMacd(bars, 2, 3, 2);
    const scale = buildMacdScale(macd, 100, 80);

    expect(scale.y(0)).toBe(scale.zeroY);
    expect(scale.y(0.1)).toBeLessThan(scale.zeroY);
    expect(scale.y(-0.1)).toBeGreaterThan(scale.zeroY);
  });
});
