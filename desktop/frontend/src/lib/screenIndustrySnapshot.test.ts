// @ts-expect-error Vitest runs this contract test in Node; the browser build omits global Node types.
import { readFileSync } from "node:fs";
// @ts-expect-error Vitest runs this contract test in Node; the browser build omits global Node types.
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { INDUSTRY_OPTIONS } from "./screenIndustryOptions";

describe("bundled screen industry snapshot", () => {
  it("covers the stock universe with real industry values", () => {
    const path = fileURLToPath(new URL("../../public/mobile-industry-snapshot.json", import.meta.url));
    const snapshot = JSON.parse(readFileSync(path, "utf8")) as {
      stock_count: number;
      source_stock_count: number;
      options: string[];
      industries: Record<string, string>;
    };

    expect(snapshot.source_stock_count).toBeGreaterThanOrEqual(5800);
    expect(snapshot.stock_count).toBeGreaterThanOrEqual(5500);
    expect(Object.keys(snapshot.industries).length).toBe(snapshot.stock_count);
    expect(snapshot.options).toContain("影视院线");
    expect(snapshot.options).toContain("半导体");
    expect(snapshot.options).not.toContain("-");
    expect(snapshot.industries["000001.SZ"]).toBe("银行Ⅱ");
    expect(INDUSTRY_OPTIONS.slice(1)).toEqual(snapshot.options);
  });
});
