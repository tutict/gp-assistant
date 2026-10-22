export type MarketId = "cn" | "hk";

export type SecurityKind = "ordinary_share";

export interface SecurityId {
  market: MarketId;
  code: string;
  kind: SecurityKind;
}

export type CurrencyCode = string;

export interface Money {
  amount: number;
  currency: CurrencyCode;
}

export interface Quantity {
  amount: number;
  unit: string;
}

export interface CoverageSummary {
  requested_count: number;
  available_count: number;
  missing_count: number;
  start_date: string | null;
  end_date: string | null;
  missing_fields: string[];
}

export type CapabilityStatus = "available" | "limited" | "unavailable";

export type CapabilityId =
  | "security_directory"
  | "observe"
  | "financial_snapshot"
  | "screening"
  | "backtest"
  | "news_research"
  | "sentiment"
  | "research_agent";

export interface MarketCapability {
  market: MarketId;
  id: CapabilityId;
  status: CapabilityStatus;
  reason: string | null;
  coverage: CoverageSummary | null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isMarketId(value: unknown): value is MarketId {
  return value === "cn" || value === "hk";
}

function isCapabilityId(value: unknown): value is CapabilityId {
  return (
    value === "security_directory" ||
    value === "observe" ||
    value === "financial_snapshot" ||
    value === "screening" ||
    value === "backtest" ||
    value === "news_research" ||
    value === "sentiment" ||
    value === "research_agent"
  );
}

function isCapabilityStatus(value: unknown): value is CapabilityStatus {
  return value === "available" || value === "limited" || value === "unavailable";
}

function isOptionalString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNonnegativeSafeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isCoverageSummary(value: unknown): value is CoverageSummary {
  return (
    isRecord(value) &&
    isNonnegativeSafeInteger(value.requested_count) &&
    isNonnegativeSafeInteger(value.available_count) &&
    isNonnegativeSafeInteger(value.missing_count) &&
    isOptionalString(value.start_date) &&
    isOptionalString(value.end_date) &&
    Array.isArray(value.missing_fields) &&
    value.missing_fields.every((field) => typeof field === "string")
  );
}

export function isMarketCapability(value: unknown): value is MarketCapability {
  return (
    isRecord(value) &&
    isMarketId(value.market) &&
    isCapabilityId(value.id) &&
    isCapabilityStatus(value.status) &&
    isOptionalString(value.reason) &&
    (value.coverage === null || isCoverageSummary(value.coverage))
  );
}

export function parseSecurityId(
  market: MarketId,
  code: string,
  kind: SecurityKind,
): SecurityId | null {
  const pattern = market === "hk" ? /^\d{5}$/ : /^\d{6}$/;
  if (!pattern.test(code) || kind !== "ordinary_share") {
    return null;
  }

  return { market, code, kind };
}
