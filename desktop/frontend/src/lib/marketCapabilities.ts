import {
  CAPABILITY_IDS as CAPABILITY_ID_SET,
  isMarketCapability,
  type CapabilityId,
  type MarketCapability,
  type MarketId,
} from "./marketDomain";

export const MARKET_CAPABILITY_IDS = Object.keys(CAPABILITY_ID_SET) as (keyof typeof CAPABILITY_ID_SET)[];

export type MarketResearchView = "screen" | "observe" | "backtest" | "news" | "agent";

const MARKET_CAPABILITY_IDS_BY_VIEW: Record<MarketResearchView, CapabilityId[]> = {
  screen: ["security_directory", "screening"],
  observe: ["observe", "financial_snapshot"],
  backtest: ["backtest"],
  news: ["news_research", "sentiment"],
  agent: ["research_agent"],
};

export function marketCapabilityIdsForView(view: MarketResearchView): CapabilityId[] {
  return [...MARKET_CAPABILITY_IDS_BY_VIEW[view]];
}

export interface MarketTransition {
  market: MarketId;
  clearSelection: true;
  clearObserveRequest: true;
  clearNewsRequest: true;
  clearBacktestRequest: true;
  clearCapabilities: true;
}

export function marketTransition(current: MarketId, next: MarketId): MarketTransition | null {
  if (current === next) return null;
  return {
    market: next,
    clearSelection: true,
    clearObserveRequest: true,
    clearNewsRequest: true,
    clearBacktestRequest: true,
    clearCapabilities: true,
  };
}

export function shouldApplyMarketCapabilities(
  responseMarket: MarketId,
  currentMarket: MarketId,
  responseRequestId: number,
  currentRequestId: number,
): boolean {
  return responseMarket === currentMarket && responseRequestId === currentRequestId;
}

export function parseMarketCapabilities(
  value: unknown,
  expectedMarket?: MarketId,
): MarketCapability[] | null {
  if (!Array.isArray(value) || value.length !== MARKET_CAPABILITY_IDS.length) {
    return null;
  }

  const capabilities: MarketCapability[] = [];
  for (let index = 0; index < value.length; index += 1) {
    if (!Object.hasOwn(value, index) || !isMarketCapability(value[index])) {
      return null;
    }
    capabilities.push(value[index]);
  }

  const market = capabilities[0].market;
  if (expectedMarket !== undefined && market !== expectedMarket) {
    return null;
  }
  if (
    capabilities.some(
      (capability, index) =>
        capability.market !== market || capability.id !== MARKET_CAPABILITY_IDS[index],
    )
  ) {
    return null;
  }

  return capabilities;
}
