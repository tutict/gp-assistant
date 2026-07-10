import { describe, expect, it } from "vitest";
import {
  defaultTrendStartDateInputValue,
  formatRatioPercent,
  hasMarketSuffix,
  trendScreenStartDateInputValue,
} from "./format";

describe("formatRatioPercent", () => {
  it("formats core ratio fields as visible percentages", () => {
    expect(formatRatioPercent(0.1234)).toBe("12.34%");
    expect(formatRatioPercent(-0.0123)).toBe("-1.23%");
  });

  it("keeps already-percent values readable", () => {
    expect(formatRatioPercent(12.34)).toBe("12.34%");
  });
});

describe("hasMarketSuffix", () => {
  it("treats fully qualified stock codes as complete", () => {
    expect(hasMarketSuffix("000100.SZ")).toBe(true);
    expect(hasMarketSuffix("SZ000100")).toBe(true);
  });

  it("keeps bare digits incomplete so market confirmation can still show", () => {
    expect(hasMarketSuffix("000100")).toBe(false);
  });
});

describe("trend screen date window", () => {
  it("defaults to ninety weekdays of lookback", () => {
    expect(defaultTrendStartDateInputValue("2026-07-09")).toBe("2026-03-05");
  });

  it("expands short windows while preserving longer requested ranges", () => {
    expect(trendScreenStartDateInputValue("2026-06-25", "2026-07-09")).toBe("2026-03-05");
    expect(trendScreenStartDateInputValue("2020-01-01", "2026-07-09")).toBe("2020-01-01");
  });
});
