import { parseLooseNumber } from "./format";

export type RateUnit = "ratio" | "percentage_points";
export const METRIC_CONTRACT_VERSION = 2;

export const STOCK_RATE_UNITS: Readonly<Record<string, RateUnit>> = {
  roe: "ratio",
  dividend_yield: "ratio",
  change_pct: "ratio",
  turnover_rate: "ratio",
  deducted_net_profit_margin: "percentage_points",
  deducted_net_profit_growth_rate: "percentage_points",
};

export const FINANCIAL_RATE_UNITS: Readonly<Record<string, RateUnit>> = {
  operating_revenue_yoy: "percentage_points",
  parent_net_profit_yoy: "percentage_points",
  gross_margin: "percentage_points",
  net_margin: "percentage_points",
  roe: "percentage_points",
  asset_liability_ratio: "percentage_points",
  goodwill_to_net_assets: "percentage_points",
  pledged_share_ratio: "percentage_points",
  dividend_yield: "percentage_points",
  dividend_payout_ratio: "percentage_points",
  deducted_net_profit_margin: "percentage_points",
  deducted_net_profit_growth_rate: "percentage_points",
};

export function convertRate(value: unknown, from: RateUnit, to: RateUnit): number | null {
  const number = parseLooseNumber(value);
  if (number === null) return null;
  if (from === to) return number;
  const ratio = from === "ratio" ? number : number / 100;
  const converted = to === "ratio" ? ratio : ratio * 100;
  return Number.isFinite(converted) ? converted : null;
}

export function financialSnapshotUnits(
  snapshot: Record<string, unknown>,
  knownBundledSource = false,
): Readonly<Record<string, RateUnit>> {
  const declared = snapshot.metric_units;
  if (declared !== undefined) {
    if (!declared || typeof declared !== "object" || Array.isArray(declared)) {
      throw new Error("财务快照单位声明无效");
    }
    const units: Record<string, RateUnit> = {};
    for (const [field, unit] of Object.entries(declared)) {
      if (!Object.hasOwn(FINANCIAL_RATE_UNITS, field) || (unit !== "ratio" && unit !== "percentage_points")) {
        throw new Error(`财务快照指标 ${field} 的单位无效`);
      }
      units[field] = unit;
    }
    return units;
  }
  const schema = snapshot.schema_version;
  if (schema === "mobile-financial-snapshot/v3" || (knownBundledSource && schema !== "mobile-financial-snapshot/v4")) {
    return FINANCIAL_RATE_UNITS;
  }
  throw new Error("财务快照缺少明确单位；请提供 metric_units 或已知 v3 快照版本");
}
