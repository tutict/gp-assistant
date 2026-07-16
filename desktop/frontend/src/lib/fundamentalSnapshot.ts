import type { FinancialIndicatorItem, FinancialIndicatorSection, StockItem } from "../types";
import { formatNumber, formatRatioPercent, metricOrMissing } from "./format";

export type FundamentalMetricTone = "positive" | "negative" | "warning" | "neutral";

export interface FundamentalSnapshotMetric {
  label: string;
  value: string;
  tone?: FundamentalMetricTone;
}

export interface FundamentalSnapshotData {
  quoteTime: string;
  financialPeriod: string;
  primary: FundamentalSnapshotMetric[];
  details: FundamentalSnapshotMetric[];
}

export function buildFundamentalSnapshotData(
  stock: StockItem,
  financial?: FinancialIndicatorSection | null,
): FundamentalSnapshotData {
  const items = financial?.items || [];
  const lookup = buildFinancialLookup(items);
  const totalShares = positiveNumber(stock.total_shares);
  const circulatingShares = positiveNumber(stock.circulating_shares);
  const eps = formatEpsValue(financialMetricAny(
    lookup,
    ["latest_eps", "eps", "最新每股收益", "每股收益"],
    financialMetricAny(
      lookup,
      ["estimated_eps", "每股收益(估算)", "每股收益(计算)"],
      metricOrMissing(formatNumber(stock.latest_eps ?? stock.eps)),
    ),
  ));
  const itemPeriod = items.find((item) => item.period && String(item.period).trim())?.period;
  const details: FundamentalSnapshotMetric[] = [
    { label: "市盈率(TTM)", value: metricOrMissing(formatNumber(stock.pe)) },
    { label: "市净率(最新)", value: metricOrMissing(formatNumber(stock.pb)) },
    { label: "每股净资产", value: financialMetricAny(lookup, ["latest_bps", "estimated_bps", "每股净资产", "每股净资产(估算)"]) },
    { label: "营业总收入", value: financialMetricAny(lookup, ["operating_revenue", "total_operating_revenue", "revenue", "营业总收入", "营业收入"]) },
    { label: "总营收同比", value: financialMetricAny(lookup, ["operating_revenue_yoy", "revenue_growth_rate", "total_operating_revenue_yoy", "总营收同比", "营业收入同比"]) },
    { label: "归母净利润", value: financialMetricAny(lookup, ["net_profit_parent", "parent_net_profit", "np_parent_company_owners", "归母净利润"]) },
    { label: "归母净利同比", value: financialMetricAny(lookup, ["net_profit_parent_yoy", "parent_net_profit_yoy", "归母净利同比", "归母净利润同比"]) },
    { label: "扣非净利润", value: financialMetricAny(lookup, ["deducted_net_profit", "deducted_net_profit_billion", "扣非净利润"]) },
    { label: "扣非净利同比", value: financialMetricAny(lookup, ["deducted_net_profit_growth_rate", "扣非净利同比", "扣非净利润增长率", "扣非增长"]) },
    { label: "毛利率", value: financialMetricAny(lookup, ["gross_margin", "gross_profit_margin", "毛利率"]) },
    { label: "净利率", value: financialMetricAny(lookup, ["net_margin", "net_profit_margin", "deducted_net_profit_margin", "净利率", "扣非净利率"]) },
    { label: "净资产收益率", value: metricOrMissing(formatRatioPercent(stock.roe)) },
    { label: "资产负债率", value: financialMetricAny(lookup, ["asset_liability_ratio", "debt_to_asset_ratio", "资产负债率"]) },
    { label: "商誉净资产比", value: financialMetricAny(lookup, ["goodwill_to_net_assets", "goodwill_net_asset_ratio", "商誉净资产比"]) },
    { label: "质押总股本比", value: financialMetricAny(lookup, ["pledged_share_ratio", "pledge_total_share_ratio", "质押总股本比"]) },
    { label: "股息率", value: metricOrMissing(formatRatioPercent(stock.dividend_yield)) },
    { label: "股利支付率(静)", value: financialMetricAny(lookup, ["dividend_payout_ratio", "static_dividend_payout_ratio", "股利支付率(静)", "股利支付率"]) },
  ].map((item) => ({ ...item, tone: fundamentalMetricTone(item.label, item.value) }));

  return {
    quoteTime: String(stock.quote_time || "时间未知"),
    financialPeriod: String(financial?.period || itemPeriod || "报告期未知"),
    primary: [
      { label: "每股收益", value: eps },
      { label: "总股本", value: formatShareCount(totalShares) },
      { label: "流通股", value: formatShareCount(circulatingShares) },
    ],
    details,
  };
}

function buildFinancialLookup(items: FinancialIndicatorItem[]): Map<string, string> {
  const lookup = new Map<string, string>();
  for (const item of items) {
    const value = item.value == null || item.value === "" ? "" : String(item.value);
    if (!value) continue;
    if (item.metric_key) lookup.set(item.metric_key, value);
    if (item.label) lookup.set(item.label, value);
  }
  return lookup;
}

function financialMetricAny(lookup: Map<string, string>, keys: string[], fallback = "暂无"): string {
  for (const key of keys) {
    const value = lookup.get(key);
    if (value != null && value.trim() && value.trim() !== "--") return value;
  }
  return fallback;
}

function finiteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function positiveNumber(value: unknown): number | null {
  const number = finiteNumber(value);
  return number != null && number > 0 ? number : null;
}

function formatShareCount(value: unknown): string {
  const shares = positiveNumber(value);
  return shares == null ? "暂无" : `${formatNumber(shares / 100_000_000)}亿股`;
}

function formatEpsValue(value: string): string {
  const trimmed = value.trim();
  if (!trimmed || trimmed === "暂无" || trimmed.includes("元")) return trimmed || "暂无";
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? `${formatNumber(parsed)}元` : trimmed;
}

function fundamentalMetricTone(label: string, value: string): FundamentalMetricTone {
  if (!label.includes("同比")) return "neutral";
  const parsed = Number(String(value).replace(/[%亿万元,s]/g, ""));
  if (!Number.isFinite(parsed) || parsed === 0) return "neutral";
  return parsed > 0 ? "positive" : "negative";
}
