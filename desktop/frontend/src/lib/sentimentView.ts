import type { SentimentEvidence, SentimentSnapshot } from "../types/sentiment";

const DEFAULT_METRIC_REFERENCES = {
  M1: ["positive_count", "negative_count", "sentiment_balance", "discussion_count", "sentiment_change_7d", "heat_percentile"],
  M2: ["price_return_30d_pct", "price_return_7d_pct", "volume_change_7d_pct"],
  M3: ["industry_return_30d_pct", "industry_return_7d_pct", "industry_breadth_pct", "industry_coverage_pct"],
} as const;

export type MetricReferenceId = keyof typeof DEFAULT_METRIC_REFERENCES;

export function sentimentCivilDate(value: unknown): string | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    return formatShanghai(value < 1e12 ? value * 1000 : value);
  }
  if (typeof value !== "string") return null;
  const text = value.trim();
  const compact = text.match(/^(\d{4})(\d{2})(\d{2})$/);
  if (compact) return `${compact[1]}-${compact[2]}-${compact[3]}`;
  const parsed = Date.parse(text);
  if (!Number.isNaN(parsed)) return formatShanghai(parsed);
  const prefix = text.slice(0, 10);
  return /^\d{4}-\d{2}-\d{2}$/.test(prefix) ? prefix : null;
}

function formatShanghai(ms: number): string | null {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(new Date(ms));
  const year = parts.find((part) => part.type === "year")?.value;
  const month = parts.find((part) => part.type === "month")?.value;
  const day = parts.find((part) => part.type === "day")?.value;
  return year && month && day ? `${year}-${month}-${day}` : null;
}

export function metricIncludesZero(metric: string): boolean {
  return metric === "sentiment_balance" || metric === "industry_return";
}

export function chartBounds(values: number[], includeZero: boolean): { min: number; max: number } {
  if (!values.length) return { min: 0, max: 1 };
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (includeZero) {
    min = Math.min(min, 0);
    max = Math.max(max, 0);
  }
  if (min === max) {
    const pad = Math.abs(min) > 0 ? Math.abs(min) * 0.05 : 1;
    min -= pad;
    max += pad;
  }
  return { min, max };
}

export function chartMarkBottom(value: number, bounds: { min: number; max: number }): number {
  const range = bounds.max - bounds.min || 1;
  return 8 + ((value - bounds.min) / range) * 80;
}

export function metricEntries(snapshot: SentimentSnapshot, id: MetricReferenceId): Array<[string, number | string | null]> {
  const referenced = snapshot.metric_references?.[id];
  const keys = referenced && referenced.length ? referenced : [...DEFAULT_METRIC_REFERENCES[id]];
  return keys.map((key) => [key, snapshot.metrics?.[key] ?? null]);
}

export function formatMetric(key: string, value: number | string | null | undefined): string {
  if (value == null || value === "") return "缺失";
  if (typeof value === "string") return value;
  if (!Number.isFinite(value)) return "缺失";
  const digits = value.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
  if (key.endsWith("_pct") || key === "heat_percentile") return `${digits}%`;
  if (key.endsWith("_ratio") && Math.abs(value) <= 1) {
    return `${(value * 100).toLocaleString("zh-CN", { maximumFractionDigits: 1 })}%`;
  }
  return digits;
}

export function sourceHostLabel(verified: boolean): string {
  return verified ? "来源域名已识别" : "来源域名未识别";
}

export function evidenceNotes(evidence: SentimentEvidence): string[] {
  const notes: string[] = [];
  if (evidence.provenance?.retracted) notes.push("已撤回");
  if (evidence.provenance?.correction_of) notes.push("更正稿");
  if (evidence.duplicate_count > 0) notes.push(`另有 ${evidence.duplicate_count} 条同源`);
  return notes;
}

export function safeHttpUrl(value: string | null | undefined): string | null {
  return value && /^https?:\/\//i.test(value) ? value : null;
}
