import { describe, expect, it } from "vitest";
import { convertRate, financialSnapshotUnits } from "./rateUnits";

describe("explicit rate units", () => {
  it("preserves the economic value of small growth and ratios above one", () => {
    expect(convertRate(0.804352, "percentage_points", "ratio")).toBeCloseTo(0.00804352, 12);
    expect(convertRate(-0.5, "percentage_points", "ratio")).toBe(-0.005);
    expect(convertRate(1.2, "ratio", "percentage_points")).toBe(120);
    expect(convertRate(0, "ratio", "percentage_points")).toBe(0);
    expect(convertRate(Number.POSITIVE_INFINITY, "ratio", "percentage_points")).toBeNull();
  });

  it("rejects unknown declarations including inherited object property names", () => {
    expect(() => financialSnapshotUnits({ metric_units: { toString: "ratio" } })).toThrow(/单位/);
    expect(() => financialSnapshotUnits({ metric_units: { roe: "unknown" } })).toThrow(/单位/);
    expect(() => financialSnapshotUnits({ financials: {} })).toThrow(/单位/);
  });

  it("uses the known v3 contract and preserves an explicit ratio declaration", () => {
    expect(financialSnapshotUnits({ schema_version: "mobile-financial-snapshot/v3" }).roe).toBe("percentage_points");
    expect(financialSnapshotUnits({ metric_units: { roe: "ratio" } }).roe).toBe("ratio");
  });

  it.each([false, true])("rejects undeclared v4 units even when bundled-source trust is %s", (knownBundledSource) => {
    const snapshot = {
      schema_version: "mobile-financial-snapshot/v4",
      financials: { "000001.SZ": { roe: 0.12 } },
    };
    const original = structuredClone(snapshot);
    expect(() => financialSnapshotUnits(snapshot, knownBundledSource)).toThrow(/缺少明确单位/);
    expect(snapshot).toEqual(original);
  });

  it("retains the trusted legacy bundled-source mapping", () => {
    expect(financialSnapshotUnits({ financials: { "000001.SZ": { roe: 0.12 } } }, true).roe)
      .toBe("percentage_points");
  });
});
