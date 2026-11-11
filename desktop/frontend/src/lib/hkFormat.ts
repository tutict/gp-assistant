import type { HkFinancialMetric } from "../types";

const numberFormat = new Intl.NumberFormat("en-US", { useGrouping: false, maximumFractionDigits: 6 });
const priceFormat = new Intl.NumberFormat("en-US", { useGrouping: false, minimumFractionDigits: 2, maximumFractionDigits: 3 });
const amountFormat = new Intl.NumberFormat("en-US", { useGrouping: false, maximumFractionDigits: 2 });

export function finiteHkNumber(value: number | null | undefined): number | undefined {
  return value != null && Number.isFinite(value) ? value : undefined;
}

export function hkDataTimestamp(value: string | null | undefined): number | undefined {
  if (!value) return undefined;
  const timestamp = /^\d{13}$/.test(value) ? Number(value) : Date.parse(value);
  return Number.isFinite(timestamp) ? timestamp : undefined;
}

export function formatHkDataTime(value: string | null | undefined): string {
  const timestamp = hkDataTimestamp(value);
  if (timestamp == null) return value || "时间未知";
  return new Intl.DateTimeFormat("zh-CN", {timeZone:"Asia/Hong_Kong",year:"numeric",month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit",hourCycle:"h23"}).format(new Date(timestamp));
}

export function formatHkNumber(value: number | null | undefined): string {
  return finiteHkNumber(value) == null ? "暂无" : numberFormat.format(value!);
}

export function formatHkPrice(value: number | null | undefined): string {
  return finiteHkNumber(value) == null ? "暂无" : priceFormat.format(value!);
}

export function formatHkSignedPercent(value: number | null | undefined): string {
  const ratio = finiteHkNumber(value);
  if (ratio == null) return "暂无";
  return `${ratio > 0 ? "+" : ""}${(ratio * 100).toFixed(2)}%`;
}

export function formatHkAmount(value: number | null | undefined, currency: string): string {
  if (finiteHkNumber(value) == null) return "暂无";
  const amount = value!;
  const scale = Math.abs(amount) >= 100_000_000 ? 100_000_000 : Math.abs(amount) >= 10_000 ? 10_000 : 1;
  const unit = scale === 100_000_000 ? "亿" : scale === 10_000 ? "万" : "";
  return `${currency.trim() || "币种未知"} ${amountFormat.format(amount / scale)}${unit}`;
}

export function formatHkShares(value: number | null | undefined): string {
  if (finiteHkNumber(value) == null || value! < 0) return "暂无";
  const shares = value!;
  const scale = shares >= 100_000_000 ? 100_000_000 : shares >= 10_000 ? 10_000 : 1;
  const unit = scale === 100_000_000 ? "亿" : scale === 10_000 ? "万" : "";
  return `${amountFormat.format(shares / scale)}${unit}股`;
}


export function formatHkFinancialMetric(metric?: HkFinancialMetric | null): string {
  if (!metric || finiteHkNumber(metric.value) == null) return "暂无";
  const currency = typeof metric.currency === "string" ? metric.currency : "";
  if (metric.unit === "percent") return `${formatHkNumber(metric.value)}%`;
  if (metric.unit === "shares") return formatHkShares(metric.value);
  if (metric.unit === "currency/share") return `${currency || "币种未知"} ${formatHkNumber(metric.value)}/股`;
  if (metric.unit === "currency_total" || metric.unit === "yuan") return formatHkAmount(metric.value, currency);
  if (metric.unit && metric.unit !== "ratio") return `${currency ? `${currency} ` : ""}${formatHkNumber(metric.value)} ${metric.unit}`;
  return formatHkNumber(metric.value);
}
