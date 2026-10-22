import { describe, expect, it } from "vitest";
import {
  MARKET_CAPABILITY_IDS,
  parseMarketCapabilities,
} from "./marketCapabilities";

describe("market capability contract", () => {
  it("declares all registry IDs in stable order", () => {
    expect(MARKET_CAPABILITY_IDS).toEqual([
      "security_directory",
      "observe",
      "financial_snapshot",
      "screening",
      "backtest",
      "news_research",
      "sentiment",
      "research_agent",
    ]);
  });

  it("accepts only a complete, ordered registry for one market", () => {
    const capabilities = MARKET_CAPABILITY_IDS.map((id) => ({
      market: "hk",
      id,
      status: "unavailable",
      reason: "adapter missing",
      coverage: null,
    }));

    expect(parseMarketCapabilities(capabilities)).toEqual(capabilities);
    expect(parseMarketCapabilities(capabilities, "cn")).toBeNull();
    expect(parseMarketCapabilities(capabilities.slice(1))).toBeNull();
    const sparseCapabilities = new Array(MARKET_CAPABILITY_IDS.length);
    expect(parseMarketCapabilities(sparseCapabilities)).toBeNull();
    expect(
      parseMarketCapabilities(
        capabilities.map((capability, index) =>
          index === 7 ? { ...capability, market: "cn" } : capability,
        ),
      ),
    ).toBeNull();
  });
});
