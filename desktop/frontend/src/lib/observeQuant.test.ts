import { describe, expect, it } from "vitest";
import type { TrendIndicatorPoint } from "../types";
import { calculateObserveQuant } from "./observeQuant";

function point(index: number, overrides: Partial<TrendIndicatorPoint> = {}): TrendIndicatorPoint {
  const close = 100 + index * 0.5;
  return {
    date: `2026-01-${String(index + 1).padStart(2, "0")}`,
    open: close - 0.2,
    high: close + 0.8,
    low: close - 0.8,
    close,
    volume: 10_000_000,
    volume_price_heat: 50,
    accumulation_strength: 50,
    accumulation_index: 0,
    trend_heat: 50,
    rebound_signal: 50,
    ...overrides,
  };
}

describe("calculateObserveQuant", () => {
  it("returns four explicit insufficient conclusions when history is too short", () => {
    const result = calculateObserveQuant([point(0), point(1), point(2)]);

    expect(result.conclusions).toHaveLength(4);
    expect(result.conclusions.every((item) => item.state.includes("不足"))).toBe(true);
    expect(result.details.some(([label]) => label === "趋势效率公式")).toBe(true);
  });

  it("identifies a smooth upward path as efficient", () => {
    const series = Array.from({ length: 30 }, (_, index) => point(index));
    const result = calculateObserveQuant(series);
    const trend = result.conclusions.find((item) => item.key === "trend_efficiency");

    expect(trend?.state).toBe("上行趋势顺畅");
    expect(result.details.find(([label]) => label === "趋势效率 ER")?.[1]).toBe("100%");
  });

  it("detects when the fund proxy strengthens faster than price", () => {
    const series = Array.from({ length: 40 }, (_, index) => {
      const heat = 48 + Math.sin(index) * 3;
      return point(index, {
        close: 100 + index * 0.08,
        high: 101 + index * 0.08,
        low: 99 + index * 0.08,
        volume_price_heat: index === 39 ? 98 : heat,
        accumulation_strength: index === 39 ? 96 : heat,
        accumulation_index: index === 39 ? 22 : 0,
        trend_heat: index === 39 ? 94 : heat,
        rebound_signal: index === 39 ? 90 : heat,
      });
    });
    const result = calculateObserveQuant(series);
    const divergence = result.conclusions.find((item) => item.key === "fund_divergence");

    expect(divergence?.state).toBe("资金强于价格");
  });

  it("raises volatility and liquidity risk for expanding ranges on thin volume", () => {
    const series = Array.from({ length: 70 }, (_, index) => {
      const late = index >= 48;
      const close = late ? 124 + Math.sin(index) * (index - 46) : 100 + index * 0.5;
      const range = late ? 1 + (index - 47) * 0.45 : 0.8;
      return point(index, {
        close,
        open: close - range * 0.2,
        high: close + range,
        low: close - range,
        volume: late ? 20_000 : 10_000_000,
      });
    });
    const result = calculateObserveQuant(series);
    const volatility = result.conclusions.find((item) => item.key === "volatility_state");
    const liquidity = result.conclusions.find((item) => item.key === "liquidity_risk");

    expect(["波动扩张", "高波动"]).toContain(volatility?.state);
    expect(liquidity?.state).toBe("流动性风险偏高");
  });
});
