import type { HkObserveResult, HkQuote, ObserveResult, StockItem, TrendIndicatorResult } from "../../types";
import type { ObservationMetadata } from "../../lib/marketDomain";
import { finiteHkNumber, formatHkFinancialMetric as metricText } from "../../lib/hkFormat";

function quoteName(quote: HkQuote | null | undefined, fallback: string): string {
  return quote?.name_zh || fallback || quote?.name || "港股";
}

function percentToRatio(value: number | null | undefined): number | undefined {
  return value == null || !Number.isFinite(value) ? undefined : value / 100;
}

function stockFromHk(result: HkObserveResult): StockItem {
  const quote = result.quote;
  const financial = result.financial;
  const currency = quote?.currency || result.security.currency;
  const financialMarketCap = financial?.market_cap?.currency.toUpperCase() === currency.toUpperCase()
    && financial.market_cap.unit === "currency_total" ? finiteHkNumber(financial.market_cap.value) : undefined;
  const financialDividendYield = financial?.dividend_yield?.unit === "percent"
    ? percentToRatio(financial.dividend_yield.value)
    : undefined;
  return {
    code: `${result.security.code}.HK`,
    name: quoteName(quote, result.security.name_zh || result.security.name),
    industry: "港股",
    price: finiteHkNumber(quote?.price),
    change_pct: finiteHkNumber(quote?.change_pct),
    pe: finiteHkNumber(quote?.pe) ?? finiteHkNumber(financial?.pe?.value),
    pb: finiteHkNumber(quote?.pb) ?? finiteHkNumber(financial?.pb?.value),
    roe: financial?.roe?.unit === "percent" ? percentToRatio(financial.roe.value) : undefined,
    latest_eps: finiteHkNumber(financial?.eps?.value),
    total_shares: finiteHkNumber(quote?.total_shares) ?? (financial?.total_shares?.unit === "shares" ? finiteHkNumber(financial.total_shares.value) : undefined),
    circulating_shares: finiteHkNumber(quote?.circulating_shares) ?? (financial?.circulating_shares?.unit === "shares" ? finiteHkNumber(financial.circulating_shares.value) : undefined),
    dividend_yield: percentToRatio(quote?.dividend_yield_percent) ?? financialDividendYield,
    market_cap: finiteHkNumber(quote?.market_cap_hkd) ?? financialMarketCap,
    quote_time: quote?.quote_time,
    quote_currency: currency,
  };
}

function financialItems(result: HkObserveResult) {
  const snapshot = result.financial;
  if (!snapshot) return [];
  const items = [
    ["latest_eps", "每股收益", snapshot.eps],
    ["latest_bps", "每股净资产", snapshot.additional_metrics?.bps],
    ["operating_revenue", "营业总收入", snapshot.revenue],
    ["net_profit_parent", "归母净利润", snapshot.net_profit],
    ["roe", "净资产收益率", snapshot.roe],
    ["pe", "市盈率(TTM)", snapshot.pe],
    ["pb", "市净率(最新)", snapshot.pb],
    ["dividend_yield", "股息率", snapshot.dividend_yield],
    ["asset_liability_ratio", "资产负债率", snapshot.additional_metrics?.asset_liability_ratio],
    ...Object.entries(snapshot.additional_metrics || {}).map(([key, metric]) => [key, additionalMetricLabels[key] || key, metric] as const),
  ] as const;
  const seen = new Set<string>();
  return items.filter(([key]) => {
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  }).map(([metric_key, label, metric]) => ({
    metric_key,
    label,
    value: metricText(metric),
    raw_value: finiteHkNumber(metric?.value) ?? null,
    reason: metric?.reason || null,
    period: snapshot.normalized_period,
  }));
}

function hkTrend(result: HkObserveResult, stock: StockItem): TrendIndicatorResult {
  const series = result.history.map((bar) => ({
    date: bar.date,
    open: bar.open,
    high: bar.high,
    low: bar.low,
    close: bar.close,
    volume: bar.volume_shares,
  }));
  const latest = series.at(-1);
  const previous = series.at(-2);
  const quote = finiteHkNumber(result.quote?.price) != null ? result.quote : null;
  const close = quote?.price ?? latest?.close;
  let previousClose = quote ? finiteHkNumber(quote.previous_close) : previous?.close;
  let change = quote ? finiteHkNumber(quote.change) : latest && previous ? latest.close - previous.close : undefined;
  let changePct = quote ? finiteHkNumber(quote.change_pct) : undefined;
  if (quote && previousClose == null) {
    if (change != null && quote.price - change > 0) previousClose = quote.price - change;
    else if (changePct != null && changePct > -1) previousClose = quote.price / (1 + changePct);
  }
  if (change == null && close != null && previousClose != null) change = close - previousClose;
  if (changePct == null && change != null && previousClose != null && previousClose > 0) changePct = change / previousClose;
  return {
    stock,
    series,
    signal: {
      code: stock.code,
      date: quote ? quote.quote_time || "" : latest?.date || "",
      close: close ?? Number.NaN,
      previous_close: previousClose,
      close_change: change,
      close_change_pct: changePct,
      status: "观察",
    },
  };
}

function provenance(result: HkObserveResult): ObservationMetadata[] {
  const values: ObservationMetadata[] = [];
  if (result.quote) values.push({ source_id: result.quote.source, fetched_at: result.quote.fetched_at || result.quote_updated_at || null, observed_at: result.quote.quote_time || null, disclosed_at: null, report_period: null, verification_status: "unverified", quality_flags: [], missing_reason: result.quote_error || result.quote.last_error || null });
  if (result.history.length) values.push({ source_id: result.history_source || "港股日 K 缓存", fetched_at: result.history_updated_at || null, observed_at: result.history.at(-1)?.date || null, disclosed_at: null, report_period: null, verification_status: "unverified", quality_flags: ["daily_history"], missing_reason: result.history_error || null });
  if (result.financial) values.push({ source_id: result.financial.candidate_source, fetched_at: result.candidate_updated_at || result.financial.candidate_fetched_at, observed_at: null, disclosed_at: result.financial.disclosed_at || null, report_period: result.financial.normalized_period, verification_status: result.financial.verification_status, quality_flags: ["candidate_financial"], missing_reason: null });
  if (result.financial?.supplemental_source) values.push({ source_id: result.financial.supplemental_source, fetched_at: result.financial.supplemental_fetched_at || null, observed_at: null, disclosed_at: result.financial.disclosed_at || null, report_period: result.financial.normalized_period, verification_status: result.financial.verification_status, quality_flags: ["supplemental_candidate_financial"], missing_reason: null });
  if (result.official_disclosure) values.push({ source_id: result.official_disclosure.source_name, fetched_at: result.official_checked_at || result.official_disclosure.checked_at, observed_at: null, disclosed_at: result.official_disclosure.disclosed_at || null, report_period: result.official_disclosure.report_period || null, verification_status: result.official_disclosure.status, quality_flags: ["official_filing_lookup"], missing_reason: result.official_disclosure.error || null });
  if (result.fx_rate) values.push({ source_id: result.fx_rate.source, fetched_at: result.fx_updated_at || result.fx_rate.fetched_at, observed_at: result.fx_rate.observed_at, disclosed_at: null, report_period: null, verification_status: "unverified", quality_flags: ["currency_reference_rate"], missing_reason: null });
  if (result.quarterly_update?.code === result.security.code) {
    const update = result.quarterly_update;
    values.push({source_id:update.source_name,fetched_at:update.fetched_at,observed_at:null,disclosed_at:update.disclosed_at,report_period:update.period,verification_status:"unverified",quality_flags:["quarterly_operating_update",update.audit_status],missing_reason:result.quarterly_update_error || null});
  }
  return values;
}

export function toHkSharedObserveResult(result: HkObserveResult): ObserveResult & { market: "hk"; security_kind: "listed_equity" | "ordinary_share" } {
  const stock = stockFromHk(result);
  const financial = result.financial;
  const quarter = result.quarterly_update?.code === result.security.code ? result.quarterly_update : undefined;
  const latestQuarter = quarter && (!financial?.report_date || quarter.period_end > financial.report_date) ? quarter : undefined;
  return {
    market: "hk",
    security_kind: result.security.classification_status === "exchange_listed_equity" ? "listed_equity" : "ordinary_share",
    source: "hk-market",
    stock,
    financial_indicators: financial || latestQuarter ? {
      title: "港股财务快照",
      period: financial?.normalized_period,
      latest_disclosure_period: latestQuarter?.period,
      latest_disclosure_date: latestQuarter?.disclosed_at,
      report_date: financial?.report_date,
      disclosed_at: financial?.disclosed_at,
      updated_at: result.candidate_updated_at || financial?.candidate_fetched_at,
      supplemental_source: financial?.supplemental_source,
      supplemental_updated_at: financial?.supplemental_fetched_at,
      source: financial?.candidate_source,
      verification_status: financial?.verification_status,
      fx_reference: result.fx_rate ? { rate: result.fx_rate.rate, source: result.fx_rate.source, observed_at: result.fx_rate.observed_at } : null,
      items: financialItems(result),
      notes: result.financial_notes || [],
    } : null,
    trend: result.quote || result.history.length ? hkTrend(result, stock) : undefined,
    notes: [
      ...result.notes,
      ...(result.quote_capability.status === "unavailable" && result.quote_capability.reason ? [`行情能力不可用：${result.quote_capability.reason}`] : []),
      ...(result.quote_error ? [`报价：${result.quote_error}`] : []),
      ...(result.history_error ? [`日 K：${result.history_error}`] : []),
      ...(result.refresh_error ? [`刷新：${result.refresh_error}`] : []),
      ...(result.financial_notes || []),
      ...Object.entries(result.financial_field_errors || result.financial?.field_errors || {}).map(([field, reason]) => `${additionalMetricLabels[field] || field}：${reason}`),
      ...financialItems(result).flatMap(item => item.reason && item.raw_value == null ? [`${item.label}：${item.reason}`] : []),
      result.financial ? "港股财务候选值尚未按官方财报逐项核验。" : "",
      ...(result.financial?.verification_conflicts || []).map((field) => `候选值与官方披露存在冲突：${additionalMetricLabels[field] || field}。`),
      result.fx_rate ? `汇率参考：1 HKD = ${result.fx_rate.rate} CNY（${result.fx_rate.source}，${result.fx_rate.observed_at}）。金额按来源原币展示。` : "缺少 HKD/CNY 汇率缓存；金额按来源原币展示。",
      result.official_disclosure ? `HKEXnews 披露检索：${result.official_disclosure.status}；该状态不代表财务数值已经核验。` : "尚无 HKEXnews 披露检索记录。",
    ].filter(Boolean),
    data_metadata: { data_policy: "cache_only", source: [result.quote?.source, result.history_source, financial?.candidate_source].filter(Boolean).join(" / ") || "港股缓存", quote_as_of: result.quote?.quote_time || null, history_as_of: result.history.at(-1)?.date || null },
    observation_metadata: provenance(result),
  };
}

const additionalMetricLabels: Record<string, string> = {
  revenue_yoy: "营业收入同比",
  net_profit_yoy: "归母净利润同比",
  gross_margin: "毛利率",
  net_margin: "净利率",
  asset_liability_ratio: "资产负债率",
  bps: "每股净资产",
  operating_cash_flow: "经营现金流",
  operating_cash_flow_per_share: "每股经营现金流",
  current_ratio: "流动比率",
  diluted_eps: "摊薄每股收益",
};
