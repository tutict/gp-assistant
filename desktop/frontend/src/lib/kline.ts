// Multi-timeframe K-line helpers for the observe charts.
// The backend returns a daily trend series; weekly/monthly candles are
// aggregated here, and MA/KDJ are recomputed per timeframe so each period
// renders a complete K-line view without extra backend calls.

import type { TrendIndicatorPoint } from "../types";

export type KlinePeriod = "daily" | "weekly" | "monthly";

export const KLINE_PERIODS: { key: KlinePeriod; label: string }[] = [
  { key: "daily", label: "日K" },
  { key: "weekly", label: "周K" },
  { key: "monthly", label: "月K" },
];

export interface KlineBar {
  date: string;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

export type MarketDirection = "up" | "down" | "flat";

export function marketDirection(value: number, reference: number | null | undefined): MarketDirection {
  if (!Number.isFinite(value) || reference == null || !Number.isFinite(reference)) return "flat";
  const delta = value - reference;
  const tolerance = Number.EPSILON * Math.max(1, Math.abs(value), Math.abs(reference)) * 8;
  if (Math.abs(delta) <= tolerance) return "flat";
  return delta > 0 ? "up" : "down";
}

const toNum = (value: unknown): number => {
  const n = typeof value === "number" ? value : Number(value);
  return Number.isFinite(n) ? n : NaN;
};

// Daily points only carry OHLC when the backend cache has them; drop partial
// bars so candles and aggregation never see incomplete data.
export function toDailyBars(series: TrendIndicatorPoint[]): KlineBar[] {
  return series
    .map((point) => ({
      date: String(point.date || ""),
      open: toNum(point.open),
      high: toNum(point.high),
      low: toNum(point.low),
      close: toNum(point.close),
      volume: toNum(point.volume),
    }))
    .filter(
      (bar) =>
        bar.date !== "" &&
        Number.isFinite(bar.open) &&
        Number.isFinite(bar.high) &&
        Number.isFinite(bar.low) &&
        Number.isFinite(bar.close),
    )
    .sort((left, right) => left.date.replace(/\D/g, "").localeCompare(right.date.replace(/\D/g, "")));
}

function ymd(date: string): { y: number; m: number; d: number } | null {
  const digits = String(date).replace(/\D/g, "");
  if (digits.length < 8) return null;
  return { y: Number(digits.slice(0, 4)), m: Number(digits.slice(4, 6)), d: Number(digits.slice(6, 8)) };
}

// ISO week bucket so Mon–Fri of one trading week collapse into a single candle.
function weekKey(date: string): string {
  const parts = ymd(date);
  if (!parts) return date;
  const dt = new Date(Date.UTC(parts.y, parts.m - 1, parts.d));
  const day = dt.getUTCDay() || 7;
  dt.setUTCDate(dt.getUTCDate() + 4 - day);
  const yearStart = Date.UTC(dt.getUTCFullYear(), 0, 1);
  const week = Math.ceil(((dt.getTime() - yearStart) / 86_400_000 + 1) / 7);
  return `${dt.getUTCFullYear()}-W${String(week).padStart(2, "0")}`;
}

function monthKey(date: string): string {
  const parts = ymd(date);
  if (!parts) return date;
  return `${parts.y}-${String(parts.m).padStart(2, "0")}`;
}

// Aggregate ascending daily bars into the requested timeframe (open = first,
// close = last, high/low = extremes, volume = sum). Chronological order is kept.
export function aggregateBars(daily: KlineBar[], period: KlinePeriod): KlineBar[] {
  if (period === "daily" || daily.length === 0) return daily;
  const keyOf = period === "weekly" ? weekKey : monthKey;
  const buckets = new Map<string, KlineBar>();
  const order: string[] = [];
  for (const bar of daily) {
    const key = keyOf(bar.date);
    const bucket = buckets.get(key);
    if (!bucket) {
      buckets.set(key, { ...bar, volume: Number.isFinite(bar.volume) ? bar.volume : 0 });
      order.push(key);
    } else {
      bucket.high = Math.max(bucket.high, bar.high);
      bucket.low = Math.min(bucket.low, bar.low);
      bucket.close = bar.close;
      bucket.date = bar.date;
      if (Number.isFinite(bar.volume)) bucket.volume += bar.volume;
    }
  }
  return order.map((key) => buckets.get(key) as KlineBar);
}

// Simple moving average aligned to the input; entries before `window` are null.
export function movingAverage(values: number[], window: number): (number | null)[] {
  const out: (number | null)[] = [];
  let sum = 0;
  for (let i = 0; i < values.length; i += 1) {
    sum += values[i];
    if (i >= window) sum -= values[i - window];
    out.push(i >= window - 1 ? sum / window : null);
  }
  return out;
}

export type KdjPoint = {
  date: string;
  k: number;
  d: number;
  j: number;
};

export type MacdPoint = {
  date: string;
  dif: number;
  dea: number;
  macd: number;
};

export type MacdScale = {
  zeroY: number;
  y: (value: number) => number;
};

export function buildMacdScale(series: MacdPoint[], top: number, height: number): MacdScale {
  const values = series
    .flatMap((point) => [point.dif, point.dea, point.macd])
    .filter(Number.isFinite);
  const maxAbs = Math.max(...values.map((value) => Math.abs(value)), 1e-6) * 1.08;
  const zeroY = top + height / 2;
  const y = (value: number) => zeroY - (value / maxAbs) * (height * 0.44);
  return {
    zeroY,
    y,
  };
}

// TDX-style smoothing: Y = (weight*X + (window-weight)*prevY) / window.
function tdxSma(values: number[], window: number, weight: number): number[] {
  const out: number[] = [];
  let prev: number | null = null;
  for (const raw of values) {
    const value = Number.isFinite(raw) ? raw : 50;
    const current: number = prev === null ? value : (weight * value + (window - weight) * prev) / window;
    prev = current;
    out.push(current);
  }
  return out;
}

// KDJ(9,3,3) matching the backend daily computation, usable on any timeframe.
export function computeKdj(bars: KlineBar[]): KdjPoint[] {
  const rsv = bars.map((bar, index) => {
    const start = Math.max(0, index - 8);
    let high = -Infinity;
    let low = Infinity;
    for (let i = start; i <= index; i += 1) {
      high = Math.max(high, bars[i].high);
      low = Math.min(low, bars[i].low);
    }
    const spread = high - low;
    if (spread <= 0) return NaN;
    return Math.min(100, Math.max(0, ((bar.close - low) / spread) * 100));
  });
  const k = tdxSma(rsv, 3, 1);
  const d = tdxSma(k, 3, 1);
  return bars.map((bar, index) => ({ date: bar.date, k: k[index], d: d[index], j: 3 * k[index] - 2 * d[index] }));
}

function exponentialMovingAverage(values: number[], period: number): number[] {
  const out: number[] = [];
  const alpha = 2 / (period + 1);
  let prev: number | null = null;
  for (const raw of values) {
    const value = Number.isFinite(raw) ? raw : (prev ?? 0);
    const current: number = prev === null ? value : alpha * value + (1 - alpha) * prev;
    prev = current;
    out.push(current);
  }
  return out;
}

// MACD(12,26,9): DIF = EMA12 - EMA26, DEA = EMA(DIF,9), histogram = 2*(DIF-DEA).
export function computeMacd(bars: KlineBar[], shortPeriod = 12, longPeriod = 26, signalPeriod = 9): MacdPoint[] {
  const closes = bars.map((bar) => bar.close);
  const emaShort = exponentialMovingAverage(closes, shortPeriod);
  const emaLong = exponentialMovingAverage(closes, longPeriod);
  const dif = closes.map((_, index) => emaShort[index] - emaLong[index]);
  const dea = exponentialMovingAverage(dif, signalPeriod);
  return bars.map((bar, index) => ({
    date: bar.date,
    dif: dif[index],
    dea: dea[index],
    macd: 2 * (dif[index] - dea[index]),
  }));
}
