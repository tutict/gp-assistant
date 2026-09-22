import { expect, it } from "vitest";
import { chartBounds, chartMarkBottom, formatMetric, metricEntries, sentimentCivilDate } from "./sentimentView";
import type { SentimentSnapshot } from "../types/sentiment";

it("maps timestamps onto the Shanghai civil date used by the timeline", () => {
  expect(sentimentCivilDate("2026-09-14T20:00:00Z")).toBe("2026-09-15");
  expect(sentimentCivilDate("2026-09-14T10:00:00+08:00")).toBe("2026-09-14");
  expect(sentimentCivilDate("20260914")).toBe("2026-09-14");
  expect(sentimentCivilDate(Date.parse("2026-09-14T20:00:00Z"))).toBe("2026-09-15");
});

it("does not pin a positive price series to zero", () => {
  const bounds = chartBounds([10, 11], false);
  expect(chartMarkBottom(10, bounds)).toBeCloseTo(8);
  expect(chartMarkBottom(11, bounds)).toBeCloseTo(88);
  const crossed = chartBounds([-0.2, 0.4], true);
  expect(crossed.min).toBeLessThan(0);
  expect(crossed.max).toBeGreaterThan(0);
});

it("reads only the snapshot metric references and keeps percent units", () => {
  const snapshot = {
    metrics: { sentiment_balance: 0.4, price_return_7d_pct: 2, unofficial: 9 },
    metric_references: { M2: ["price_return_7d_pct"] },
  } as unknown as SentimentSnapshot;
  expect(metricEntries(snapshot, "M2")).toEqual([["price_return_7d_pct", 2]]);
  expect(formatMetric("price_return_7d_pct", 2)).toBe("2%");
  expect(formatMetric("heat_percentile", null)).toBe("缺失");
});

