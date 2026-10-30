import { parseLooseNumber } from "./format";
import { convertRate, FINANCIAL_RATE_UNITS } from "./rateUnits";
import type { RateUnit } from "./rateUnits";

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
  "deducted_net_profit_margin",
  "deducted_net_profit_growth_rate",
] as const;

const DETAIL_PERIOD_FIELDS = [
  "goodwill_period",
  "pledged_share_period",
  "dividend_period",
] as const;

export function normalizeMobileFinancialSnapshotDetails(
  source: Record<string, unknown>,
  units: Readonly<Record<string, RateUnit>> = FINANCIAL_RATE_UNITS,
): Record<string, unknown> {
  const details: Record<string, unknown> = {};
  for (const field of DETAIL_NUMBER_FIELDS) {
    const value = parseLooseNumber(source[field]);
    if (value === null) continue;
    if (field in FINANCIAL_RATE_UNITS) {
      const unit = units[field];
      if (unit !== "ratio" && unit !== "percentage_points") {
        throw new Error(`财务指标 ${field} 缺少明确单位`);
      }
      const converted = convertRate(value, unit, "percentage_points");
      if (converted !== null) details[field] = converted;
    } else {
      details[field] = value;
    }
  }
  for (const field of DETAIL_PERIOD_FIELDS) {
    const value = String(source[field] || "").trim();
    if (value) details[field] = value;
  }
  return details;
}
