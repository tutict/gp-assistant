import { describe, expect, it } from "vitest";
import type { VolatilitySnapshot } from "../types";
import { buildVolatilityInterpretation } from "./volatilityInterpretation";

const weakContractingSnapshot: VolatilitySnapshot = {
  symbol: "000553.SZ",
  date: "2026-07-17",
  close: 5.15,
  atr: { period: 14, value: 0.25, percent_of_close: 4.82 },
  bollinger_bands: { period: 20, multiplier: 2, upper: 6.2, middle: 5.67, lower: 5.14, percent_b: -6.82, bandwidth_percent: 16.14 },
  donchian_channel: { period: 20, upper: 6.1, middle: 5.59, lower: 5.08, position_percent: 6.86, width_percent: 18.25 },
  keltner_channel: { ema_period: 20, atr_period: 10, multiplier: 2, upper: 6.18, middle: 5.67, lower: 5.16, position_percent: -1.3, width_percent: 17.92 },
  chaikin_volatility: { ema_period: 10, roc_period: 10, value: -12.73 },
  rvi: { period: 14, value: 35.96 },
};

describe("buildVolatilityInterpretation", () => {
  it("turns lower-channel, downward and contracting readings into an actionable diagnosis", () => {
    const result = buildVolatilityInterpretation(weakContractingSnapshot);

    expect(result.returnedCount).toBe(6);
    expect(result.summary).toContain("贴近近期价格区间底部");
    expect(result.summary).toContain("最近下跌方向的波动明显更多");
    expect(result.summary).toContain("每天的高低价差明显变小");
    expect(result.summary).toContain("当前波动偏大");
    expect(result.summary).toContain("不代表已经止跌");
    expect(result.evidence.join(" ")).toContain("布林 %B -6.82%");
    expect(result.adjustments.join(" ")).toContain("RVI）回到中线 50");
    expect(result.adjustments.join(" ")).toContain("误触发次数");
  });

  it("recognizes upper-channel expansion and upward directional volatility", () => {
    const result = buildVolatilityInterpretation({
      ...weakContractingSnapshot,
      atr: { period: 14, value: 8.2, percent_of_close: 3.05 },
      bollinger_bands: { ...weakContractingSnapshot.bollinger_bands!, percent_b: 80.83 },
      donchian_channel: { ...weakContractingSnapshot.donchian_channel!, position_percent: 76.43 },
      keltner_channel: { ...weakContractingSnapshot.keltner_channel!, position_percent: 85.58 },
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: 12.5 },
      rvi: { period: 14, value: 61.2 },
    });

    expect(result.summary).toContain("贴近近期价格区间顶部");
    expect(result.summary).toContain("最近上涨方向的波动明显更多");
    expect(result.summary).toContain("每天的高低价差明显变大");
    expect(result.summary).toContain("是否能延续仍需用后续样本验证");
    expect(result.adjustments.join(" ")).toContain("收盘越过顶部");
  });

  it("degrades to a data preparation conclusion when every indicator is unavailable", () => {
    const result = buildVolatilityInterpretation({ symbol: "000553.SZ", date: "2026-07-17" });

    expect(result.returnedCount).toBe(0);
    expect(result.summary).toContain("有效数据还不够");
    expect(result.adjustments).toEqual([
      "先补齐历史日线，再比较策略参数；不要根据缺失的数据调整规则。",
    ]);
  });

  it("keeps published threshold boundaries aligned with the conclusion", () => {
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      rvi: { period: 14, value: 60 },
    }).summary).toContain("最近上涨方向的波动明显更多");
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      rvi: { period: 14, value: 40 },
    }).summary).toContain("最近下跌方向的波动明显更多");
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: 10 },
    }).summary).toContain("每天的高低价差明显变大");
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: -10 },
    }).summary).toContain("每天的高低价差明显变小");
  });

  it("does not emit an empty summary for partially populated indicator objects", () => {
    const result = buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      bollinger_bands: {
        period: 20,
        multiplier: 2,
        upper: 6.2,
        middle: 5.67,
        lower: 5.14,
      },
    });

    expect(result.returnedCount).toBe(1);
    expect(result.summary).toContain("还不足以得出可信的结论");
    expect(result.adjustments[0]).toContain("先检查数据是否完整");
  });
});
