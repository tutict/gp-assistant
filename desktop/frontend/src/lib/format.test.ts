import { describe, expect, it } from "vitest";
import { formatRatioPercent } from "./format";

describe("formatRatioPercent", () => {
  it("formats core ratio fields as visible percentages", () => {
    expect(formatRatioPercent(0.1234)).toBe("12.34%");
    expect(formatRatioPercent(-0.0123)).toBe("-1.23%");
  });

  it("keeps already-percent values readable", () => {
    expect(formatRatioPercent(12.34)).toBe("12.34%");
  });
});
