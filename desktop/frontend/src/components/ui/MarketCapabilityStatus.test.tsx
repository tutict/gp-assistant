import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MarketCapability } from "../../lib/marketDomain";
import { MarketCapabilityStatus } from "./MarketCapabilityStatus";

describe("MarketCapabilityStatus", () => {
  it("renders market-scoped source time, freshness, verification and quality evidence", () => {
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

    expect(html).toContain("tencent · quote");
    expect(html).toContain("新鲜度：新鲜");
    expect(html).toContain("核验：unverified");
    expect(html).toContain("抓取：2026-10-27 08:00 UTC");
    expect(html).toContain("质量标记：provider_rights_unverified");
    expect(html).toContain("A 股观察显示本地行情和历史数据；数据授权与逐证券历史新鲜度尚未核验。");
  });
});
