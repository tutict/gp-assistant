import { describe, expect, it } from "vitest";
import { normalizeMobileFinancialSnapshotDetails } from "./mobileFinancialSnapshot";

describe("normalizeMobileFinancialSnapshotDetails", () => {
  it("converts explicitly declared ratios into the financial DTO percentage-point contract", () => {
    const actual = normalizeMobileFinancialSnapshotDetails({
      roe: 0.0007,
      operating_revenue_yoy: 0.00804352,
      dividend_yield: 0.0095,
    }, { roe: "ratio", operating_revenue_yoy: "ratio", dividend_yield: "ratio" });
    expect(actual.roe).toBeCloseTo(0.07, 12);
    expect(actual.operating_revenue_yoy).toBeCloseTo(0.804352, 12);
    expect(actual.dividend_yield).toBeCloseTo(0.95, 12);
  });

  it("does not shrink a legitimate ratio greater than one", () => {
    expect(normalizeMobileFinancialSnapshotDetails({ roe: 1.2 }, { roe: "ratio" }).roe).toBe(120);
  });

  it("rejects a present percentage field when its explicit unit declaration is missing", () => {
    expect(() => normalizeMobileFinancialSnapshotDetails({ roe: 0.07 }, {})).toThrow(/单位/);
  });

  it("preserves detailed Android financial metrics and their independent periods", () => {
    expect(normalizeMobileFinancialSnapshotDetails({
      operating_revenue_billion: "434.778212",
      operating_revenue_yoy: 8.370839,
      parent_net_profit_billion: 15.564526,
      parent_net_profit_yoy: 53.712048,
      gross_margin: 12.497484,
      net_margin: 1.399071,
      roe: 2.47,
      asset_liability_ratio: 65.027185,
      goodwill_to_net_assets: 17.9576,
      pledged_share_ratio: 0.74,
      dividend_yield: 1.82,
      dividend_payout_ratio: 38.58,
      goodwill_period: "2026一季报",
      pledged_share_period: "2026-07-10",
      dividend_period: "2025-12-31",
    })).toEqual({
      operating_revenue_billion: 434.778212,
      operating_revenue_yoy: 8.370839,
      parent_net_profit_billion: 15.564526,
      parent_net_profit_yoy: 53.712048,
      gross_margin: 12.497484,
      net_margin: 1.399071,
      roe: 2.47,
      asset_liability_ratio: 65.027185,
      goodwill_to_net_assets: 17.9576,
      pledged_share_ratio: 0.74,
      dividend_yield: 1.82,
      dividend_payout_ratio: 38.58,
      goodwill_period: "2026一季报",
      pledged_share_period: "2026-07-10",
      dividend_period: "2025-12-31",
    });
  });

  it("drops empty and invalid fields instead of manufacturing values", () => {
    expect(normalizeMobileFinancialSnapshotDetails({
      roe: "",
      dividend_yield: "not-a-number",
      pledged_share_period: "   ",
    })).toEqual({});
  });
});
