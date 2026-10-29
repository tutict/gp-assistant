import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MarketCapability } from "../../lib/marketDomain";
import { MarketCapabilityPopover } from "./MarketCapabilityPopover";

describe("MarketCapabilityPopover", () => {
  it("keeps capability details out of layout until the user opens the summary", () => {
    const capabilities: MarketCapability[] = [
      {
        market: "cn", id: "news_research", status: "unavailable",
        reason: "No authorized market-scoped news source and reuse permission are registered",
        coverage: null, sources: [],
      },
      {
        market: "cn", id: "sentiment", status: "unavailable",
        reason: "A-share sentiment evidence coverage and permitted model-processing scope are not registered",
        coverage: null, sources: [],
      },
    ];

    const html = renderToStaticMarkup(
      <MarketCapabilityPopover
        market="cn"
        label="A 股数据状态"
        capabilities={capabilities}
        capabilityIds={["news_research", "sentiment"]}
      />,
    );

    expect(html).toContain("<details");
    expect(html).not.toContain("open=");
    expect(html).toContain("2 项未开放");
    expect(html).toContain("A 股新闻与 RAG");
    expect(html).toContain("A 股情绪");
    expect(html).toContain("market-capability-popover__content");
  });
});
