export type MarketId = "cn" | "hk";

export type SecurityKind = "ordinary_share";

export interface SecurityId {
  market: MarketId;
  code: string;
  kind: SecurityKind;
}

export interface ObservationMetadata {
  source_id: string;
  fetched_at: string | null;
  observed_at: string | null;
  disclosed_at: string | null;
  report_period: string | null;
  verification_status: string;
  quality_flags: string[];
  missing_reason: string | null;
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

export const CAPABILITY_IDS = {
  security_directory: true,
  observe: true,
  financial_snapshot: true,
  screening: true,
  backtest: true,
  news_research: true,
  sentiment: true,
  research_agent: true,
} as const;

export type CapabilityId = keyof typeof CAPABILITY_IDS;

export interface MarketCapability {
  market: MarketId;
  id: CapabilityId;
  status: CapabilityStatus;
  reason: string | null;
  coverage: CoverageSummary | null;
}

export function marketCapabilityStatusLabel(
  status: CapabilityStatus,
  unavailableLabel = "不可用",
): string {
  switch (status) {
    case "available": return "可用";
    case "limited": return "部分可用";
    case "unavailable": return unavailableLabel;
  }
}

export function marketCapabilityLabel(id: CapabilityId): string {
  const labels: Record<CapabilityId, string> = {
    security_directory: "证券目录",
    observe: "证券观察",
    financial_snapshot: "财务快照",
    screening: "筛选",
    backtest: "回测",
    news_research: "新闻与 RAG",
    sentiment: "情绪",
    research_agent: "研究 Agent",
  };
  return labels[id];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isMarketId(value: unknown): value is MarketId {
  return value === "cn" || value === "hk";
}

function isCapabilityId(value: unknown): value is CapabilityId {
  return typeof value === "string" && Object.hasOwn(CAPABILITY_IDS, value);
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
