import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import type { TrendIndicatorPoint } from "../types";

const SERIES_LENGTH = 128;
const VISIBLE_BARS = 96;
const DESKTOP_VIEWBOX_WIDTH = 1180;

function buildSeries(): TrendIndicatorPoint[] {
  const start = Date.UTC(2026, 0, 1);
  return Array.from({ length: SERIES_LENGTH }, (_, index) => {
    const close = 10 + Math.sin(index / 3) * 0.8 + Math.sin(index / 11) * 0.35 + index * 0.005;
    const open = close + Math.sin(index * 1.7) * 0.12;
    return {
      date: new Date(start + index * 86_400_000).toISOString().slice(0, 10),
      open,
      high: Math.max(open, close) + 0.16,
      low: Math.min(open, close) - 0.16,
      close,
      volume: 1_000_000 + index * 10_000,
    };
  });
}

function numberAttribute(tag: string, attribute: string): number {
  const value = tag.match(new RegExp(`${attribute}="([^"]+)"`))?.[1];
  return Number(value);
}

function polylinePoints(markup: string, className: string): string[] {
  const tag = markup.match(new RegExp(`<polyline[^>]*class="${className}"[^>]*>`))?.[0] || "";
  const points = tag.match(/points="([^"]+)"/)?.[1] || "";
  return points.trim().split(/\s+/).filter(Boolean);
}

describe("K-line rendering regressions", () => {
  let markup = "";

  beforeAll(async () => {
    vi.stubGlobal("window", { location: { href: "http://localhost/" } });
    const { TrendCharts } = await import("../components/observe/ObserveCharts");
    markup = renderToStaticMarkup(createElement(TrendCharts, { series: buildSeries() }));
  });

  afterAll(() => {
    vi.unstubAllGlobals();
  });

  it("uses nearly all horizontal plot space while preserving the current-price label", () => {
    const candleBodies = Array.from(markup.matchAll(/<rect class="kline-body"[^>]*>/g), ([tag]) => tag);
    const priceLabel = markup.match(/<g class="kline-last-price[^"]*"[^>]*><rect[^>]*>/)?.[0] || "";
    const firstX = numberAttribute(candleBodies[0], "x");
    const lastX = numberAttribute(candleBodies.at(-1) || "", "x");
    const lastWidth = numberAttribute(candleBodies.at(-1) || "", "width");
    const labelX = numberAttribute(priceLabel, "x");
    const labelWidth = numberAttribute(priceLabel, "width");

    expect(candleBodies).toHaveLength(VISIBLE_BARS);
    expect(markup).not.toContain("kline-axis-label left");
    expect(markup).toContain("kline-axis-label right");
    expect(firstX).toBeLessThanOrEqual(12);
    expect(labelX - (lastX + lastWidth)).toBeLessThanOrEqual(4);
    expect(DESKTOP_VIEWBOX_WIDTH - (labelX + labelWidth)).toBe(8);
  });

  it("renders finite, continuous DIF and DEA lines above non-masking cross markers", () => {
    const macdStart = markup.indexOf('<g class="kline-macd-layer">');
    const macdEnd = markup.indexOf('<text class="kline-subpanel-title"', macdStart);
    const macdMarkup = markup.slice(macdStart, macdEnd);
    const markerIndex = macdMarkup.indexOf('<g class="indicator-cross-layer"');
    const difIndex = macdMarkup.indexOf('class="trend-line line-dif"');
    const deaIndex = macdMarkup.indexOf('class="trend-line line-dea"');
    const difPoints = polylinePoints(macdMarkup, "trend-line line-dif");
    const deaPoints = polylinePoints(macdMarkup, "trend-line line-dea");
    const allCoordinatesAreFinite = [...difPoints, ...deaPoints]
      .every((point) => point.split(",").every((value) => Number.isFinite(Number(value))));

    expect(markerIndex).toBeGreaterThan(-1);
    expect(markerIndex).toBeLessThan(difIndex);
    expect(markerIndex).toBeLessThan(deaIndex);
    expect(macdMarkup).toContain('<circle r="5.8" fill="none"></circle>');
    expect(macdMarkup).toContain('fill="none"></path>');
    expect(difPoints).toHaveLength(VISIBLE_BARS);
    expect(deaPoints).toHaveLength(VISIBLE_BARS);
    expect(allCoordinatesAreFinite).toBe(true);
  });
});
