import type { ObserveResult, StockItem, TrendIndicatorPoint, TrendIndicatorSignal } from "../types";
import { computeKdj, type KlineBar } from "./kline";

export function hydrateObserveTrendFromHistory(
  result: ObserveResult,
  code: string,
  historyRows: Record<string, unknown>[] | null | undefined,
): ObserveResult {
  const series = historyRowsToTrendSeries(historyRows || []);
  if (series.length < 2) return result;
  if ((result.trend?.series || []).length >= 2) return result;

  const latest = series[series.length - 1];
  const currentLatest = latestResultTrendPoint(result);
  const stock: StockItem = {
    ...(result.stock || { code, name: code }),
    code: result.stock?.code || code,
    name: result.stock?.name || code,
    price: currentLatest?.close ?? latest.close,
  };
  const currentSignal = result.trend?.signal;
  const shouldRefreshSignal = !currentLatest?.date || currentLatest.date < latest.date;
  const signal: TrendIndicatorSignal = shouldRefreshSignal
    ? {
        ...(currentSignal || {}),
        code: stock.code,
        date: latest.date,
        close: latest.close,
        k: latest.k ?? null,
        d: latest.d ?? null,
        j: latest.j ?? null,
        status: currentSignal?.status || "neutral",
      }
    : {
        ...(currentSignal || {}),
        code: currentSignal?.code || stock.code,
        date: currentSignal?.date || latest.date,
        close: currentSignal?.close ?? latest.close,
        k: currentSignal?.k ?? latest.k ?? null,
        d: currentSignal?.d ?? latest.d ?? null,
        j: currentSignal?.j ?? latest.j ?? null,
        status: currentSignal?.status || "neutral",
      };

  return {
    ...result,
    stock,
    trend: {
      ...(result.trend || {}),
      stock,
      signal,
      series,
    },
  };
}

function latestResultTrendPoint(result: ObserveResult): { date?: string; close?: number | null } | null {
  const series = result.trend?.series || [];
  if (series.length) return series[series.length - 1];
  return result.trend?.signal || null;
}
export function historyRowsToTrendSeries(rows: Record<string, unknown>[]): TrendIndicatorPoint[] {
  const bars = rows
    .map((row) => {
      const close = finiteNumber(row.close);
      if (!row.date || close == null) return null;
      const open = finiteNumber(row.open) ?? close;
      const high = finiteNumber(row.high) ?? Math.max(open, close);
      const low = finiteNumber(row.low) ?? Math.min(open, close);
      return {
        date: String(row.date),
        open,
        close,
        high,
        low,
        volume: finiteNumber(row.volume) ?? 0,
      } satisfies KlineBar;
    })
    .filter(Boolean) as KlineBar[];

  bars.sort((left, right) => left.date.localeCompare(right.date));
  const kdjByDate = new Map(computeKdj(bars).map((point) => [point.date, point]));
  return bars.map((bar) => {
    const kdj = kdjByDate.get(bar.date);
    return {
      ...bar,
      k: kdj?.k ?? null,
      d: kdj?.d ?? null,
      j: kdj?.j ?? null,
    };
  });
}

export function finiteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
}
