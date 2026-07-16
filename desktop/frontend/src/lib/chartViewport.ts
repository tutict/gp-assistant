export type ChartZoomDirection = "in" | "out";

export const MIN_CHART_VISIBLE_BARS = 30;
export const MAX_CHART_VISIBLE_BARS = 180;

export function chartVisibleBounds(
  totalBars: number,
  minBars = MIN_CHART_VISIBLE_BARS,
  maxBars = MAX_CHART_VISIBLE_BARS,
) {
  const safeTotal = Math.max(0, Math.floor(totalBars));
  const maximum = Math.max(1, Math.min(maxBars, safeTotal || maxBars));
  const minimum = Math.min(Math.max(1, minBars), maximum);
  return { minimum, maximum };
}

export function clampChartVisibleCount(
  requested: number,
  totalBars: number,
  minBars = MIN_CHART_VISIBLE_BARS,
  maxBars = MAX_CHART_VISIBLE_BARS,
) {
  const { minimum, maximum } = chartVisibleBounds(totalBars, minBars, maxBars);
  const safeRequested = Number.isFinite(requested) ? Math.round(requested) : minimum;
  return Math.max(minimum, Math.min(maximum, safeRequested));
}

export function zoomChartVisibleCount(
  current: number,
  direction: ChartZoomDirection,
  totalBars: number,
) {
  const factor = direction === "in" ? 0.8 : 1.25;
  const candidate = direction === "in"
    ? Math.min(current - 1, Math.round(current * factor))
    : Math.max(current + 1, Math.round(current * factor));
  return clampChartVisibleCount(candidate, totalBars);
}
