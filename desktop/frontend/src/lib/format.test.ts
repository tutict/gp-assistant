import { describe, expect, it } from "vitest";
import {
  defaultTrendStartDateInputValue,
  formatRatioPercent,
  hasMarketSuffix,
  normalizeStockCode,
  normalizeBacktestDateRange,
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

  it("accepts explicitly identified Hong Kong stock codes", () => {
    expect(hasMarketSuffix("00700.HK")).toBe(true);
    expect(hasMarketSuffix("HK00700")).toBe(true);
  });
});

describe("normalizeStockCode", () => {
  it("normalizes Hong Kong codes and pads a bare code only in HK context", () => {
    expect(normalizeStockCode("00700.HK")).toBe("00700.HK");
    expect(normalizeStockCode("HK00700")).toBe("00700.HK");
    expect(normalizeStockCode("700", "HK")).toBe("00700.HK");
    expect(normalizeStockCode("700")).toBe("");
  });

  it("keeps mainland normalization intact and rejects invalid HK codes", () => {
    expect(normalizeStockCode("600000.SH")).toBe("600000.SH");
    expect(normalizeStockCode("000001.SZ")).toBe("000001.SZ");
    expect(normalizeStockCode("920001.BJ")).toBe("920001.BJ");
    expect(normalizeStockCode("123456.HK")).toBe("");
    expect(normalizeStockCode("1234567.HK")).toBe("");
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

describe("backtest date range", () => {
  it("repairs persisted future and reversed dates against the current day", () => {
    expect(normalizeBacktestDateRange("2027-01-01", "2026-11-12", "2026-10-16")).toEqual({
      startDate: "2020-01-01",
      endDate: "2026-10-16",
    });
  });

  it("keeps a valid historical range unchanged", () => {
    expect(normalizeBacktestDateRange("2026-01-05", "2026-06-27", "2026-10-16")).toEqual({
      startDate: "2026-01-05",
      endDate: "2026-06-27",
    });
  });
});
