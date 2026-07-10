import { describe, expect, it } from "vitest";
import { metricOrMissing } from "../../lib/format";

describe("metricOrMissing", () => {
  it("keeps available indicator values", () => {
    expect(metricOrMissing("20.72")).toBe("20.72");
    expect(metricOrMissing(" 1.67 ")).toBe("1.67");
  });

  it("uses 暂无 for missing indicators", () => {
    expect(metricOrMissing("")).toBe("暂无");
    expect(metricOrMissing("   ")).toBe("暂无");
    expect(metricOrMissing("--")).toBe("暂无");
  });
});