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
    expect(result.summary).toContain("价格在近期低位");
    expect(result.summary).toContain("最近下跌更明显");
    expect(result.summary).toContain("行情正在变平静");
    expect(result.summary).toContain("还不能说明已经止跌");
    expect(result.evidence.join(" ")).toContain("最近一天股价通常波动约 4.82%");
    expect(result.adjustments.join(" ")).toContain("重新回到正常范围");
    expect(result.adjustments.join(" ")).toContain("价格低就当成要反弹");
    expect(`${result.summary} ${result.evidence.join(" ")} ${result.adjustments.join(" ")}`).not.toMatch(/ATR|RVI|Chaikin|%B|EMA/);
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

    expect(result.summary).toContain("价格在近期高位");
    expect(result.summary).toContain("最近上涨更明显");
    expect(result.summary).toContain("更容易大起大落");
    expect(result.adjustments.join(" ")).toContain("真正突破并站稳");
  });

  it("degrades to a data preparation conclusion when every indicator is unavailable", () => {
    const result = buildVolatilityInterpretation({ symbol: "000553.SZ", date: "2026-07-17" });

    expect(result.returnedCount).toBe(0);
    expect(result.summary).toContain("历史行情还不完整");
    expect(result.adjustments).toEqual([
      "先补齐历史行情，再下结论；数据不全时不要急着改策略。",
    ]);
  });

  it("keeps published threshold boundaries aligned with the conclusion", () => {
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      rvi: { period: 14, value: 60 },
    }).summary).toContain("最近上涨更占优势");
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      rvi: { period: 14, value: 40 },
    }).summary).toContain("最近下跌更占优势");
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: 10 },
    }).summary).toContain("每天的涨跌幅度明显变大");
    expect(buildVolatilityInterpretation({
      symbol: "000553.SZ",
      date: "2026-07-17",
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: -10 },
    }).summary).toContain("每天的涨跌幅度明显变小");
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
    expect(result.summary).toContain("少量数字");
    expect(result.adjustments[0]).toContain("补齐历史行情");
  });
});
