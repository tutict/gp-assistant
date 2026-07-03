import { currentSystemDateCompact, normalizeDateParam, normalizeStockCode, parseLooseNumber } from "./format";

const OBSERVE_HISTORY_LIMIT = 10000;
const EASTMONEY_KLINE_ENDPOINT = "https://push2his.eastmoney.com/api/qt/stock/kline/get";
const TENCENT_DAILY_KLINE_ENDPOINT = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";

export type TimedFetch = (
  input: RequestInfo | URL,
  init: RequestInit,
  timeoutMs: number,
  message: string,
) => Promise<Response>;

export async function fetchObserveDailyHistoryRows(
  payload: Record<string, unknown>,
  timeoutMs: number,
  timedFetch: TimedFetch,
): Promise<Record<string, unknown>[] | null> {
  const eastmoneyRows = await fetchObserveEastmoneyDailyHistory(payload, timeoutMs, timedFetch).catch(() => null);
  if (eastmoneyRows?.length) return eastmoneyRows;

  const tencentRows = await fetchObserveTencentDailyHistory(payload, timeoutMs, timedFetch).catch(() => null);
  return tencentRows?.length ? tencentRows : null;
}

async function fetchObserveEastmoneyDailyHistory(
  payload: Record<string, unknown>,
  timeoutMs: number,
  timedFetch: TimedFetch,
): Promise<Record<string, unknown>[]> {
  const secid = observeEastmoneySecid(String(payload.code || ""));
  if (!secid) return [];

  const beg = normalizeDateParam(payload.start_date, "20200101");
  const end = normalizeDateParam(payload.end_date, currentSystemDateCompact());
  const url =
    `${EASTMONEY_KLINE_ENDPOINT}?fields1=f1,f2,f3,f4,f5,f6`
    + `&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61`
    + `&ut=7eea3edcaed734bea9cbfc24409ed989&klt=101&fqt=0`
    + `&secid=${encodeURIComponent(secid)}&beg=${encodeURIComponent(beg)}&end=${encodeURIComponent(end)}`;

  const response = await timedFetch(
    url,
    { cache: "no-store" },
    timeoutMs,
    `Eastmoney daily history timed out after ${Math.round(timeoutMs / 1000)}s.`,
  );
  if (!response.ok) throw new Error(`Eastmoney daily history HTTP ${response.status}`);

  const data = asRecord(await response.json());
  const klines = asRecord(data.data).klines;
  const rows: unknown[] = Array.isArray(klines) ? klines : [];
  return rows.map(parseEastmoneyKlineRow).filter(Boolean) as Record<string, unknown>[];
}

async function fetchObserveTencentDailyHistory(
  payload: Record<string, unknown>,
  timeoutMs: number,
  timedFetch: TimedFetch,
): Promise<Record<string, unknown>[]> {
  const symbol = observeTencentDailySymbol(String(payload.code || ""));
  if (!symbol) return [];

  const start = hyphenDateParam(payload.start_date, "2020-01-01");
  const end = hyphenDateParam(payload.end_date, hyphenDateParam(currentSystemDateCompact(), "2050-12-31"));
  const param = `${symbol},day,${start},${end},${OBSERVE_HISTORY_LIMIT},`;
  const url = `${TENCENT_DAILY_KLINE_ENDPOINT}?param=${encodeURIComponent(param)}`;

  const response = await timedFetch(
    url,
    { cache: "no-store" },
    timeoutMs,
    `Tencent daily history timed out after ${Math.round(timeoutMs / 1000)}s.`,
  );
  if (!response.ok) throw new Error(`Tencent daily history HTTP ${response.status}`);

  const data = asRecord(await response.json());
  const quote = asRecord(asRecord(data.data)[symbol]);
  const rows = Array.isArray(quote.day) ? quote.day : Array.isArray(quote.qfqday) ? quote.qfqday : [];
  return rows.map(observeTencentDailyRowToHistory).filter(Boolean) as Record<string, unknown>[];
}

function observeTencentDailySymbol(rawCode: string): string {
  const code = normalizeStockCode(rawCode);
  if (!code) return "";
  const [digits, market] = code.split(".");
  return `${String(market || "").toLowerCase()}${digits}`;
}

function observeEastmoneySecid(rawCode: string): string {
  const normalized = normalizeStockCode(rawCode);
  if (!normalized) return "";
  const [digits, market] = normalized.split(".");
  if (market === "SH") return `1.${digits}`;
  if (market === "SZ" || market === "BJ") return `0.${digits}`;
  return "";
}

function observeTencentDailyRowToHistory(row: unknown): Record<string, unknown> | null {
  if (!Array.isArray(row) || row.length < 6) return null;
  const close = parseLooseNumber(row[2]);
  if (close === null) return null;
  return {
    date: normalizeHistoryDate(row[0]),
    open: parseLooseNumber(row[1]) ?? close,
    close,
    high: parseLooseNumber(row[3]) ?? close,
    low: parseLooseNumber(row[4]) ?? close,
    volume: parseLooseNumber(row[5]),
  };
}

function parseEastmoneyKlineRow(row: unknown): Record<string, unknown> | null {
  if (typeof row !== "string") return null;
  const parts = row.split(",");
  if (parts.length < 6) return null;
  const close = parseLooseNumber(parts[2]);
  if (close === null) return null;
  return {
    date: normalizeHistoryDate(parts[0]),
    open: parseLooseNumber(parts[1]) ?? close,
    close,
    high: parseLooseNumber(parts[3]) ?? close,
    low: parseLooseNumber(parts[4]) ?? close,
    volume: parseLooseNumber(parts[5]),
  };
}

function normalizeHistoryDate(value: unknown): string {
  const digits = String(value || "").replace(/\D/g, "");
  if (digits.length === 8) {
    return `${digits.slice(0, 4)}-${digits.slice(4, 6)}-${digits.slice(6, 8)}`;
  }
  return String(value || "");
}

function hyphenDateParam(value: unknown, fallback: string): string {
  const digits = normalizeDateParam(value, fallback).replace(/\D/g, "");
  if (digits.length !== 8) return fallback;
  return `${digits.slice(0, 4)}-${digits.slice(4, 6)}-${digits.slice(6, 8)}`;
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}
