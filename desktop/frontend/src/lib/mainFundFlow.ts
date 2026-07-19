import type { CapitalEvidenceItem, CapitalEvidenceResult } from "../types";

export type MainFundFlowTone = "positive" | "negative" | "neutral";

export interface MainFundFlowView {
  available: boolean;
  tradeDate: string;
  source: string;
  netAmount: string;
  netRatio: string;
  involvement: string;
  conclusion: string;
  tone: MainFundFlowTone;
  status: string;
}

const AMOUNT_KEYS = ["主力净流入额", "主力净流入", "main_net_inflow", "main_inflow"];
const RAW_AMOUNT_KEYS = ["主力净流入额原值", ...AMOUNT_KEYS];
const RATIO_KEYS = ["主力净占比", "主力净流入占比", "main_net_ratio", "main_inflow_ratio"];
const INVOLVEMENT_KEYS = ["主力介入度", "main_involvement", "main_participation"];

export function buildMainFundFlowView(
  capital?: CapitalEvidenceResult | null,
): MainFundFlowView {
  const items = capital?.items || [];
  const item = items.find((candidate) => (
    candidate.category === "fund_flow"
    && !isLocalFundFlowProxy(candidate)
    && hasAnyMetric(candidate, [...AMOUNT_KEYS, ...RATIO_KEYS])
  ));
  const statusItem = items.find((candidate) => candidate.category === "fund_flow_status");

  if (!item) {
    return {
      available: false,
      tradeDate: statusItem?.date || capital?.as_of_trade_date || "最新交易日",
      source: statusItem?.source || "东方财富个股资金流",
      netAmount: "暂缺",
      netRatio: "暂缺",
      involvement: "暂缺",
      conclusion: "当前没有拿到真实的当天主力资金数据，暂时不能判断主力是在买还是在卖。下方量价代理只能反映成交行为，不能替代主力净流入。",
      tone: "neutral",
      status: metricText(statusItem, ["状态"]) || "等待真实资金流数据",
    };
  }

  const amountMetric = metricEntry(item, AMOUNT_KEYS);
  const rawAmountMetric = metricEntry(item, RAW_AMOUNT_KEYS);
  const ratioMetric = metricEntry(item, RATIO_KEYS);
  const amountValue = parseMetricNumber(rawAmountMetric?.value);
  const ratioValue = parseMetricNumber(ratioMetric?.value);
  const amountDirection = numericDirection(amountValue);
  const ratioDirection = numericDirection(ratioValue);
  const directionsConflict = amountDirection !== 0
    && ratioDirection !== 0
    && amountDirection !== ratioDirection;
  const tone: MainFundFlowTone = directionsConflict
    ? "neutral"
    : (ratioDirection || amountDirection) > 0
      ? "positive"
      : (ratioDirection || amountDirection) < 0
        ? "negative"
        : "neutral";
  const providedConclusion = metricText(item, ["通俗结论", "plain_conclusion"]);

  return {
    available: true,
    tradeDate: item.date || capital?.as_of_trade_date || "最新交易日",
    source: item.source || "外部个股资金流",
    netAmount: formatAmountMetric(amountMetric?.value),
    netRatio: formatRatioMetric(ratioMetric?.value),
    involvement: metricText(item, INVOLVEMENT_KEYS) || involvementLabel(ratioValue),
    conclusion: directionsConflict
      ? "净流入额和净占比方向不一致，数据可能仍在刷新，先不要据此判断主力方向。"
      : buildPlainConclusion(ratioValue, amountValue, providedConclusion),
    tone,
    status: item.confidence ? item.confidence + "置信" : "已取得真实资金流",
  };
}

export function isLocalFundFlowProxy(item: CapitalEvidenceItem): boolean {
  return Boolean(
    item.title?.includes("量价资金代理")
    || item.source?.includes("Tauri/Rust")
    || metricText(item, ["证据类型"]) === "本地日线量价代理",
  );
}

function buildPlainConclusion(
  ratioValue: number | null,
  amountValue: number | null,
  provided: string,
): string {
  if (ratioValue != null) {
    const magnitude = Math.abs(ratioValue);
    if (magnitude <= 0.05) {
      return "主力净流入接近零，当天买卖力量大致相抵，没有明确的资金方向。";
    }
    const direction = ratioValue > 0 ? "净买入" : "净卖出";
    const attitude = ratioValue > 0 ? "短线资金态度偏积极" : "短线资金态度偏谨慎";
    const strength = magnitude >= 8 ? "影响较大" : magnitude >= 3 ? "影响中等" : "影响较小";
    return "每 100 元成交中，约有 " + formatDecimal(magnitude) + " 元形成主力" + direction
      + "，主力交易对当天价格的" + strength + "，" + attitude + "；但单日资金不能单独当作买卖依据。";
  }
  if (provided) {
    return provided + " 这是单日信号，不能单独当作买卖依据。";
  }
  if (amountValue != null && Math.abs(amountValue) > 0) {
    return amountValue > 0
      ? "主力资金当天为净流入，但缺少成交占比，暂时看不出这笔流入对股价的影响有多大。"
      : "主力资金当天为净流出，但缺少成交占比，暂时看不出这笔流出对股价的影响有多大。";
  }
  return "真实资金流已返回，但关键数值不完整，暂时不能判断主力方向。";
}

function involvementLabel(ratio: number | null): string {
  if (ratio == null) return "暂缺";
  const magnitude = Math.abs(ratio);
  const level = magnitude >= 8 ? "高" : magnitude >= 3 ? "中" : "低";
  return level + "（" + formatDecimal(magnitude) + "%）";
}

function metricEntry(
  item: CapitalEvidenceItem | null | undefined,
  keys: string[],
): { key: string; value: unknown } | null {
  for (const key of keys) {
    const value = item?.metrics?.[key];
    if (!isMissing(value)) return { key, value };
  }
  return null;
}

function metricText(
  item: CapitalEvidenceItem | null | undefined,
  keys: string[],
): string {
  const entry = metricEntry(item, keys);
  return entry ? String(entry.value).trim() : "";
}

function hasAnyMetric(item: CapitalEvidenceItem, keys: string[]): boolean {
  return keys.some((key) => !isMissing(item.metrics?.[key]));
}

function formatAmountMetric(value: unknown): string {
  if (isMissing(value)) return "暂缺";
  if (typeof value === "number") return formatCnyAmount(value);
  const text = String(value).trim();
  if (/[万亿]/.test(text)) return text;
  const parsed = parseMetricNumber(text);
  return parsed == null ? text : formatCnyAmount(parsed);
}

function formatRatioMetric(value: unknown): string {
  if (isMissing(value)) return "暂缺";
  const text = String(value).trim();
  const parsed = parseMetricNumber(value);
  if (parsed == null || text.includes("%")) return text;
  return formatDecimal(parsed) + "%";
}

function formatCnyAmount(value: number): string {
  if (Math.abs(value) >= 100_000_000) return formatDecimal(value / 100_000_000) + " 亿";
  if (Math.abs(value) >= 10_000) return formatDecimal(value / 10_000) + " 万";
  return formatDecimal(value);
}

function parseMetricNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value !== "string") return null;
  const text = value.trim().replaceAll(",", "");
  const match = text.match(/[+-]?\d+(?:\.\d+)?/);
  if (!match) return null;
  const parsed = Number(match[0]);
  if (!Number.isFinite(parsed)) return null;
  if (text.includes("亿")) return parsed * 100_000_000;
  if (text.includes("万")) return parsed * 10_000;
  return parsed;
}

function numericDirection(value: number | null): -1 | 0 | 1 {
  if (value == null || Math.abs(value) <= Number.EPSILON) return 0;
  return value > 0 ? 1 : -1;
}

function formatDecimal(value: number): string {
  return value.toFixed(2).replace(/\.?0+$/, "");
}

function isMissing(value: unknown): boolean {
  if (value == null) return true;
  const text = String(value).trim();
  return !text || ["-", "--", "—", "暂无"].includes(text);
}
