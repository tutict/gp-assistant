import { describe, expect, it } from "vitest";
import { formatRatioPercent, hasMarketSuffix } from "./format";

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
