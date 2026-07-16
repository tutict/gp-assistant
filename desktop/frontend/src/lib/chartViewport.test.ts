import { describe, expect, it } from "vitest";
import { chartVisibleBounds, clampChartVisibleCount, zoomChartVisibleCount } from "./chartViewport";

describe("chart viewport helpers", () => {
  it("uses the available history when it is shorter than the normal minimum", () => {
    expect(chartVisibleBounds(12)).toEqual({ minimum: 12, maximum: 12 });
    expect(clampChartVisibleCount(72, 12)).toBe(12);
  });

  it("clamps requests to the supported chart window", () => {
    expect(clampChartVisibleCount(10, 400)).toBe(30);
    expect(clampChartVisibleCount(300, 400)).toBe(180);
  });

  it("zooms in and out while preserving the bounds", () => {
    expect(zoomChartVisibleCount(100, "in", 400)).toBe(80);
    expect(zoomChartVisibleCount(100, "out", 400)).toBe(125);
    expect(zoomChartVisibleCount(30, "in", 400)).toBe(30);
    expect(zoomChartVisibleCount(180, "out", 400)).toBe(180);
  });
});
