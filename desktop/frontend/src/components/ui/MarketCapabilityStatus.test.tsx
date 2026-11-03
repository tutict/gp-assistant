import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MarketCapability } from "../../lib/marketDomain";
import { MarketCapabilityStatus } from "./MarketCapabilityStatus";

describe("MarketCapabilityStatus", () => {
  it("keeps technical evidence collapsed behind a readable summary", () => {
    const capability: MarketCapability = {
      market: "cn",
      id: "observe",
      status: "limited",
      reason: "A-share observation uses cached quote/history; provider rights and per-security history freshness are not evidenced",
      coverage: null,
      sources: [{
        source_id: "tencent",
        domain: "quote",
        market: "cn",
        fetched_at: String(Date.parse("2026-10-27T08:00:00Z")),
        observed_at: "2026-10-27",
        disclosed_at: null,
        report_period: null,
        freshness: "fresh",
        verification_status: "unverified",
        quality_flags: ["provider_rights_unverified"],
        coverage: null,
      }],
    };

    const html = renderToStaticMarkup(
      <MarketCapabilityStatus label="A 股观察" capability={capability} />,
    );

    expect(html).toContain("数据依据 · 1 个来源");
    expect(html).not.toContain("<details open=");
    expect(html).toContain("来源：腾讯行情");
    expect(html).toContain("核验：未核验");
    expect(html).toContain("授权待确认");
    expect(html).not.toContain("质量标记：provider_rights_unverified");
    expect(html).toContain("数据域：行情");
    expect(html).toContain("抓取：2026-10-27 08:00 UTC");
    expect(html).toContain("A 股观察显示本地行情和历史数据；数据授权与逐证券历史新鲜度尚未核验。");
  });

  it("shows compact coverage and keeps missing evidence out of the primary line", () => {
    const capability: MarketCapability = {
      market: "cn",
      id: "financial_snapshot",
      status: "limited",
      reason: "Cached financial snapshots exist for only part of the universe; provider rights and point-in-time publication dates are not evidenced",
      coverage: {
        requested_count: 5927,
        available_count: 4719,
        missing_count: 1208,
        start_date: null,
        end_date: null,
        missing_fields: ["provider_rights", "point_in_time_publication_date_missing"],
      },
      sources: [],
    };

    const html = renderToStaticMarkup(
      <MarketCapabilityStatus label="A 股财务" capability={capability} />,
    );

    expect(html).toContain("覆盖 4719/5927 · 待补齐 1208 条");
    expect(html).toContain("查看缺失项 · 2 项");
    expect(html).toContain("数据授权待确认");
    expect(html).not.toContain("缺失证据：provider_rights");
  });

  it("keeps raw capability errors behind a short user-facing message", () => {
    const html = renderToStaticMarkup(
      <MarketCapabilityStatus
        label="A 股观察"
        error={'Unexpected token \'<\', "<!doctype" is not valid JSON'}
      />,
    );

    expect(html).toContain("能力状态暂时无法读取，相关数据状态未知。");
    expect(html).toContain("<details class=\"market-capability-details\"><summary>错误详情</summary>");
    expect(html).toContain("Unexpected token");
  });
});
