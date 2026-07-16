import { describe, expect, it } from "vitest";
import { normalizeMobileFinancialSnapshotDetails } from "./mobileFinancialSnapshot";

describe("normalizeMobileFinancialSnapshotDetails", () => {
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
