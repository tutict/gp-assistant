import { describe, expect, it } from "vitest";
import { metricOrMissing } from "../../lib/format";
import { buildFundamentalSnapshotData } from "../../lib/fundamentalSnapshot";

describe("metricOrMissing", () => {
  it("keeps available indicator values", () => {
    expect(metricOrMissing("20.72")).toBe("20.72");
    expect(metricOrMissing(" 1.67 ")).toBe("1.67");
  });

  it("uses 暂无 for missing indicators", () => {
    expect(metricOrMissing("")).toBe("暂无");
    expect(metricOrMissing("   ")).toBe("暂无");
    expect(metricOrMissing("--")).toBe("暂无");
  });
});

describe("buildFundamentalSnapshotData", () => {
  it("puts EPS, total shares and circulating shares in the primary strip", () => {
    const snapshot = buildFundamentalSnapshotData(
      {
        code: "600941.SH",
        name: "中国移动",
        price: 93.01,
        total_shares: 21_683_696_323,
        circulating_shares: 902_767_867,
      },
      {
        period: "2026一季报",
        items: [{ metric_key: "latest_eps", label: "最新每股收益", value: "1.67元", period: "2026一季报" }],
      },
    );

    expect(snapshot.financialPeriod).toBe("2026一季报");
    expect(snapshot.primary).toEqual([
      { label: "每股收益", value: "1.67元" },
      { label: "总股本", value: "216.84亿股" },
      { label: "流通股", value: "9.03亿股" },
    ]);
  });

  it("does not guess share counts from market cap when exact counts are absent", () => {
    const snapshot = buildFundamentalSnapshotData({
      code: "000001.SZ",
      name: "示例",
      price: 10,
      market_cap_billion: 100,
      circulating_market_cap_billion: 60,
    });

    expect(snapshot.primary[1].value).toBe("暂无");
    expect(snapshot.primary[2].value).toBe("暂无");
  });

  it("renders a complete fundamental summary when detailed snapshot metrics are available", () => {
    const snapshot = buildFundamentalSnapshotData(
      {
        code: "000100.SZ",
        name: "TCL科技",
        price: 4.95,
        pe: 20.72,
        pb: 1.72,
        total_shares: 20_800_862_447,
        circulating_shares: 20_118_326_408,
      },
      {
        period: "2026Q1",
        items: [
          { metric_key: "latest_eps", label: "每股收益", value: "0.069元" },
          { metric_key: "latest_bps", label: "每股净资产", value: "3.014元" },
          { metric_key: "operating_revenue", label: "营业总收入", value: "434.78亿" },
          { metric_key: "operating_revenue_yoy", label: "总营收同比", value: "8.37%" },
          { metric_key: "parent_net_profit", label: "归母净利润", value: "15.56亿" },
          { metric_key: "parent_net_profit_yoy", label: "归母净利同比", value: "53.71%" },
          { metric_key: "deducted_net_profit_billion", label: "扣非净利润", value: "11.55亿" },
          { metric_key: "deducted_net_profit_growth_rate", label: "扣非净利同比", value: "20.59%" },
          { metric_key: "gross_margin", label: "毛利率", value: "12.50%" },
          { metric_key: "net_margin", label: "净利率", value: "1.40%" },
          { metric_key: "roe", label: "净资产收益率", value: "2.47%" },
          { metric_key: "asset_liability_ratio", label: "资产负债率", value: "65.03%" },
          { metric_key: "goodwill_to_net_assets", label: "商誉净资产比", value: "17.96%" },
          { metric_key: "pledged_share_ratio", label: "质押总股本比", value: "1.41%" },
          { metric_key: "dividend_yield", label: "股息率", value: "1.82%" },
          { metric_key: "dividend_payout_ratio", label: "股利支付率(静)", value: "38.58%" },
        ],
      },
    );

    expect(snapshot.primary.map((item) => item.value)).not.toContain("暂无");
    expect(snapshot.details.map((item) => item.value)).not.toContain("暂无");
    expect(snapshot.details.find((item) => item.label === "净资产收益率")?.value).toBe("2.47%");
    expect(snapshot.details.find((item) => item.label === "股息率")?.value).toBe("1.82%");
  });
});
