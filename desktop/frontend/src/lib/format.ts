// Format utilities ported from app.js

export function formatNumber(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  if (Math.abs(n) >= 1e8) return (n / 1e8).toFixed(2) + "亿";
  if (Math.abs(n) >= 1e4) return (n / 1e4).toFixed(2) + "万";
  return n.toFixed(2);
}

export function formatPrice(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  return n.toFixed(2);
}

export function formatMoney(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  if (Math.abs(n) >= 1e8) return (n / 1e8).toFixed(2) + "亿";
  if (Math.abs(n) >= 1e4) return (n / 1e4).toFixed(2) + "万";
  return n.toFixed(0);
}

export function formatPercent(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  return n.toFixed(1) + "%";
}

export function formatSignedPercent(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  const sign = n > 0 ? "+" : "";
  return sign + n.toFixed(2) + "%";
}

export function formatSignedPercentPrecise(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  const sign = n > 0 ? "+" : "";
  return sign + n.toFixed(2) + "%";
}

export function formatBytes(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  if (n >= 1e9) return (n / 1e9).toFixed(1) + " GB";
  if (n >= 1e6) return (n / 1e6).toFixed(1) + " MB";
  if (n >= 1e3) return (n / 1e3).toFixed(0) + " KB";
  return n.toFixed(0) + " B";
}

export function formatDateTime(value: unknown): string {
  if (!value) return "—";
  const date = new Date(String(value));
  if (isNaN(date.getTime())) return String(value);
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatMarketCapYi(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  return n.toFixed(0) + "亿";
}

export function normalizePercent(value: unknown): number | null {
  const n = parseLooseNumber(value);
  if (n === null) return null;
  return n / 100;
}

export function parseLooseNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  if (typeof value === "number") return isFinite(value) ? value : null;
  const s = String(value).trim().replace(/[,%\s]/g, "");
  if (!s) return null;
  const n = parseFloat(s);
  return isFinite(n) ? n : null;
}

export function firstFiniteNumber(...values: unknown[]): number | null {
  for (const v of values) {
    const n = parseLooseNumber(v);
    if (n !== null) return n;
  }
  return null;
}

export function clampInt(raw: unknown, min: number, max: number, fallback: number): number {
  const n = parseLooseNumber(raw);
  if (n === null) return fallback;
  const i = Math.round(n);
  return Math.max(min, Math.min(max, i));
}

export function clampFloat(raw: unknown, min: number, max: number, fallback: number): number {
  const n = parseLooseNumber(raw);
  if (n === null) return fallback;
  return Math.max(min, Math.min(max, n));
}

export function escapeHtml(value: unknown): string {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// --- Stock code utilities ---

export function normalizeStockCode(value: unknown, market?: string): string {
  const raw = sanitizeStockCodeInput(value);
  if (!raw) return "";

  const suffixed = raw.match(/^(\d{6})\.(SH|SZ|BJ)$/);
  if (suffixed) return `${suffixed[1]}.${suffixed[2]}`;

  const prefixed = raw.match(/^(SH|SZ|BJ)(\d{6})$/);
  if (prefixed) return `${prefixed[2]}.${prefixed[1]}`;

  const digits = stockCodeDigits(raw);
  if (digits.length !== 6) return "";
  return `${digits}.${market || inferMarketFromDigits(digits)}`;
}

export function sanitizeStockCodeInput(value: unknown): string {
  return String(value ?? "")
    .trim()
    .toUpperCase()
    .replace(/[^\dA-Z.]/g, "");
}

export function sanitizeStockLookupInput(value: unknown): string {
  return String(value ?? "")
    .replace(/[\r\n\t]+/g, " ")
    .replace(/\s{2,}/g, " ")
    .trim();
}

export function stockCodeDigits(value: unknown): string {
  const raw = sanitizeStockCodeInput(value);
  const prefixed = raw.match(/^(SH|SZ|BJ)(\d{6})$/);
  if (prefixed) return prefixed[2];
  const match = raw.match(/\d{6}/);
  return match ? match[0] : "";
}

export function hasMarketSuffix(value: unknown): boolean {
  const raw = sanitizeStockCodeInput(value);
  return /^(\d{6})\.(SH|SZ|BJ)$/.test(raw) || /^(SH|SZ|BJ)(\d{6})$/.test(raw);
}

export function inferMarketFromDigits(digits: string): string {
  if (/^[569]/.test(digits)) return "SH";
  if (/^[48]/.test(digits)) return "BJ";
  return "SZ";
}

// --- Date utilities ---

export function currentSystemDateInputValue(): string {
  const now = new Date();
  return [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
  ].join("-");
}

export function currentSystemDateCompact(): string {
  return currentSystemDateInputValue().replace(/-/g, "");
}

export function currentLocalDateCompact(): string {
  return currentSystemDateCompact();
}

export function normalizeDateParam(value: unknown, fallback = ""): string {
  const s = String(value ?? "").trim();
  if (!s) return fallback;
  const digits = s.replace(/\D/g, "");
  if (digits.length === 8) return digits;
  return s;
}

export function parseDateInputValue(value: unknown): Date | null {
  const s = String(value ?? "").trim();
  if (!s) return null;
  const date = new Date(s);
  return isNaN(date.getTime()) ? null : date;
}

export function formatDateInputValue(date: Date): string {
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join("-");
}

export function toDateInputValue(value: unknown): string {
  if (!value) return "";
  const digits = String(value).replace(/\D/g, "");
  if (digits.length === 8) {
    return `${digits.slice(0, 4)}-${digits.slice(4, 6)}-${digits.slice(6, 8)}`;
  }
  return String(value);
}

export function isWeekday(date: Date): boolean {
  const day = date.getDay();
  return day !== 0 && day !== 6;
}

export function tradingWindowStartDateInputValue(referenceValue: string, tradingDays: number): string {
  const ref = parseDateInputValue(referenceValue) || new Date();
  const date = new Date(ref);
  let counted = 0;
  while (counted < tradingDays) {
    date.setDate(date.getDate() - 1);
    if (isWeekday(date)) counted++;
  }
  return formatDateInputValue(date);
}

export function defaultObserveStartDateInputValue(referenceValue = ""): string {
  return tradingWindowStartDateInputValue(referenceValue || currentSystemDateInputValue(), 10);
}

export function defaultObserveStartCompact(referenceValue = ""): string {
  return defaultObserveStartDateInputValue(referenceValue).replace(/-/g, "");
}

export function localTradeDateCompact(value: unknown): string {
  const date = parseDateInputValue(value);
  if (!date) return currentSystemDateCompact();
  return formatDateInputValue(date).replace(/-/g, "");
}

export function expectedMarketQuoteDateCompact(now = new Date()): string {
  const date = new Date(now);
  const hour = date.getHours();
  const day = date.getDay();
  if (hour < 15) {
    date.setDate(date.getDate() - (day === 0 ? 2 : day === 1 ? 3 : 1));
  }
  return formatDateInputValue(date).replace(/-/g, "");
}

export function parseDateLike(value: unknown): Date | null {
  return parseDateInputValue(value);
}

export function formatTradeDate(value: unknown): string {
  const date = parseDateLike(value);
  if (!date) return "—";
  return formatDateInputValue(date);
}

// --- Misc ---

export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function rebalanceLabel(value: string): string {
  const labels: Record<string, string> = {
    none: "买入持有",
    monthly: "月度再平衡",
    quarterly: "季度再平衡",
  };
  return labels[value] || "月度再平衡";
}

export function benchmarkLabel(value: string): string {
  const labels: Record<string, string> = {
    candidate_equal_weight: "候选池等权基准",
    none: "无基准",
  };
  return labels[value] || value || "候选池等权基准";
}

export function shortRebalanceLabel(value: string): string {
  const labels: Record<string, string> = {
    none: "持有",
    monthly: "月度",
    quarterly: "季度",
  };
  return labels[value] || rebalanceLabel(value);
}

export function shortBenchmarkLabel(value: string): string {
  const labels: Record<string, string> = {
    candidate_equal_weight: "候选池",
    none: "无",
  };
  return labels[value] || benchmarkLabel(value);
}

export function actionLabel(action: unknown): string {
  const labels: Record<string, string> = {
    buy: "买入",
    sell: "卖出",
    hold: "持有",
    reduce: "减仓",
    add: "加仓",
    init: "建仓",
  };
  return labels[String(action || "")] || String(action || "—");
}

export function sourceLabel(source: unknown): string {
  const labels: Record<string, string> = {
    tdx: "通达信",
    tencent: "腾讯",
    sina: "新浪",
    ths: "同花顺",
    eastmoney: "东方财富",
  };
  return labels[String(source || "")] || String(source || "—");
}

export function statusLabel(status: unknown): string {
  const labels: Record<string, string> = {
    ok: "正常",
    stale: "过期",
    error: "错误",
    refreshing: "刷新中",
    empty: "空",
  };
  return labels[String(status || "")] || String(status || "—");
}

export function reasonLabel(reason: unknown): string {
  const labels: Record<string, string> = {
    market_closed: "休市",
    no_data: "无数据",
    rate_limited: "请求频繁",
    timeout: "超时",
    manual: "手动",
  };
  return labels[String(reason || "")] || String(reason || "");
}

export function relationTypeLabel(type: unknown): string {
  const labels: Record<string, string> = {
    supplier: "供应商",
    customer: "客户",
    competitor: "竞争对手",
    peer: "同业",
    parent: "母公司",
    subsidiary: "子公司",
    partner: "合作伙伴",
  };
  return labels[String(type || "")] || String(type || "—");
}

export function relationStatusLabel(status: unknown): string {
  const labels: Record<string, string> = {
    active: "活跃",
    historical: "历史",
    rumored: "传闻",
  };
  return labels[String(status || "")] || String(status || "—");
}

export function impactClass(direction: unknown): string {
  if (direction === "positive" || direction === "bullish") return "positive";
  if (direction === "negative" || direction === "bearish") return "negative";
  return "neutral";
}

export function newsSentimentLabel(sentiment: unknown): string {
  const labels: Record<string, string> = {
    positive: "利好",
    negative: "利空",
    neutral: "中性",
  };
  return labels[String(sentiment || "")] || "中性";
}

export function sentimentClass(sentiment: unknown): string {
  if (sentiment === "positive" || sentiment === "bullish") return "positive";
  if (sentiment === "negative" || sentiment === "bearish") return "negative";
  return "neutral";
}

export function metricTone(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "neutral";
  if (n > 0) return "positive";
  if (n < 0) return "negative";
  return "neutral";
}

export function sourceTierLabel(tier: unknown): string {
  const labels: Record<string, string> = {
    s: "核心",
    a: "重要",
    b: "一般",
    c: "参考",
  };
  return labels[String(tier || "").toLowerCase()] || "一般";
}

export function sourceTierClass(tier: unknown): string {
  return String(tier || "").toLowerCase();
}

export function trendScreenStyleLabel(value: unknown): string {
  const labels: Record<string, string> = {
    breakthrough: "突破",
    pullback: "回踩",
    reversal: "反转",
    continuation: "延续",
  };
  return labels[String(value || "")] || String(value || "—");
}

export function newsAnalysisModeLabel(mode: unknown): string {
  const labels: Record<string, string> = {
    rag: "RAG 检索",
    direct: "直连分析",
    cache: "缓存分析",
  };
  return labels[String(mode || "")] || String(mode || "—");
}

export function heatStatus(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "neutral";
  if (n >= 80) return "hot";
  if (n >= 50) return "warm";
  return "cool";
}

export function formatHeatValue(value: unknown): string {
  const n = parseLooseNumber(value);
  if (n === null) return "—";
  return Math.round(n).toString();
}

export function kdjStateLabel(signal: Record<string, unknown> = {}, latest: Record<string, unknown> = {}): string {
  const k = parseLooseNumber(latest.k);
  const d = parseLooseNumber(latest.d);
  const j = parseLooseNumber(latest.j);
  if (k === null || d === null) return "—";
  if (j !== null && j > k && k > d) return "金叉向上";
  if (j !== null && j < k && k < d) return "死叉向下";
  if (k < 20) return "超卖区";
  if (k > 80) return "超买区";
  return "震荡";
}

export function macdStatus(latest: Record<string, unknown>, previous: Record<string, unknown>): string {
  const hist = parseLooseNumber(latest?.hist);
  const prevHist = parseLooseNumber(previous?.hist);
  if (hist === null) return "—";
  if (hist > 0 && prevHist !== null && prevHist <= 0) return "金叉";
  if (hist < 0 && prevHist !== null && prevHist >= 0) return "死叉";
  if (hist > 0) return "多头";
  if (hist < 0) return "空头";
  return "震荡";
}

export function capitalMetricStatus(tone: string, value: number | null): string {
  if (value === null) return "unknown";
  if (tone === "positive" && value > 0) return "bullish";
  if (tone === "negative" && value < 0) return "bearish";
  return "neutral";
}

export function capitalSectionStatus(section: Record<string, unknown> = {}): string {
  const tone = String(section.tone || section.sentiment || "neutral");
  return capitalMetricStatus(tone, parseLooseNumber(section.value));
}

export function mobileRefreshNotes(status: unknown, fallback = ""): string {
  const s = String(status || "");
  if (s === "ok") return "刷新成功";
  if (s === "skipped") return "跳过（非交易时段）";
  if (s === "partial") return "部分成功";
  if (s === "failed") return "刷新失败";
  return fallback || s;
}

export function priceMoveFromSignal(signal: Record<string, unknown> = {}): { price: number | null; move: number | null } {
  const price = parseLooseNumber(signal.price ?? signal.close);
  const move = parseLooseNumber(signal.change ?? signal.pct);
  return { price, move };
}

export function buildPriceMove(close: number | null, previousClose: number | null, fallback: { price?: number | null; move?: number | null } = {}): { price: number | null; move: number | null } {
  const price = close ?? fallback.price ?? null;
  if (price !== null && previousClose !== null && previousClose !== 0) {
    return { price, move: ((price - previousClose) / previousClose) * 100 };
  }
  return { price, move: fallback.move ?? null };
}

export function uniqueNotes(notes: string[] = []): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const note of notes) {
    const key = String(note || "").trim();
    if (key && !seen.has(key)) {
      seen.add(key);
      result.push(key);
    }
  }
  return result;
}

export function uniqueCompactStrings(values: string[] = []): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const v of values) {
    const s = String(v || "").trim();
    if (s && !seen.has(s)) {
      seen.add(s);
      result.push(s);
    }
  }
  return result;
}

export function parseCodes(raw: string): string[] {
  return raw
    .split(/[,，;；\s]+/)
    .map((item) => sanitizeStockLookupInput(item))
    .filter(Boolean);
}
