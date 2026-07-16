import { parseLooseNumber } from "./format";

const DETAIL_NUMBER_FIELDS = [
  "operating_revenue_billion",
  "operating_revenue_yoy",
  "parent_net_profit_billion",
  "parent_net_profit_yoy",
  "gross_margin",
  "net_margin",
  "roe",
  "asset_liability_ratio",
  "goodwill_to_net_assets",
  "pledged_share_ratio",
  "dividend_yield",
  "dividend_payout_ratio",
] as const;

const DETAIL_PERIOD_FIELDS = [
  "goodwill_period",
  "pledged_share_period",
  "dividend_period",
] as const;

export function normalizeMobileFinancialSnapshotDetails(
  source: Record<string, unknown>,
): Record<string, unknown> {
  const details: Record<string, unknown> = {};
  for (const field of DETAIL_NUMBER_FIELDS) {
    const value = parseLooseNumber(source[field]);
    if (value !== null) details[field] = value;
  }
  for (const field of DETAIL_PERIOD_FIELDS) {
    const value = String(source[field] || "").trim();
    if (value) details[field] = value;
  }
  return details;
}
