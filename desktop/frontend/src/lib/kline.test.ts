import { describe, expect, it } from "vitest";
import { aggregateBars, computeKdj, computeMacd, toDailyBars, type KlineBar } from "./kline";
import type { TrendIndicatorPoint } from "../types";

const bars: KlineBar[] = [
  { date: "2026-01-02", open: 10, high: 12, low: 9, close: 11, volume: 100 },
  { date: "2026-01-05", open: 11, high: 13, low: 10, close: 12, volume: 110 },
  { date: "2026-01-06", open: 12, high: 14, low: 11, close: 13, volume: 120 },
  { date: "2026-02-02", open: 13, high: 16, low: 12, close: 15, volume: 130 },
];

describe("kline helpers", () => {
  it("normalizes daily trend points into complete OHLC bars only", () => {
    const series: TrendIndicatorPoint[] = [
      { date: "2026-01-02", open: 10, high: 12, low: 9, close: 11, volume: 100 },
      { date: "2026-01-05", high: 13, low: 10, close: 12, volume: 110 },
    ];

    expect(toDailyBars(series)).toEqual([bars[0]]);
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
});
