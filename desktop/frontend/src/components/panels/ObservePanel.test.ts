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
});
