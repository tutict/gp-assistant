import { describe, expect, it } from "vitest";
import {
  MARKET_CAPABILITY_IDS,
  marketTransition,
  parseMarketCapabilities,
  shouldApplyMarketCapabilities,
} from "./marketCapabilities";

describe("market capability contract", () => {
  it("clears market-scoped state only when market changes", () => {
    expect(marketTransition("cn", "cn")).toBeNull();
    expect(marketTransition("cn", "hk")).toEqual({
      market: "hk",
      clearSelection: true,
      clearObserveRequest: true,
      clearNewsRequest: true,
      clearBacktestRequest: true,
      clearCapabilities: true,
    });
    expect(marketTransition("hk", "cn")?.market).toBe("cn");
  });

  it("ignores a late capability response from the previous market or request", () => {
    expect(shouldApplyMarketCapabilities("hk", "cn", 4, 5)).toBe(false);
    expect(shouldApplyMarketCapabilities("cn", "cn", 4, 5)).toBe(false);
    expect(shouldApplyMarketCapabilities("cn", "cn", 5, 5)).toBe(true);
  });

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
