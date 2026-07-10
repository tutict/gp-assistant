import { describe, expect, it } from "vitest";
import type { StockRowView } from "../types";
import { sortStocksByDisplayScore } from "./StockList";

describe("sortStocksByDisplayScore", () => {
  it("sorts by balanced score before legacy score without mutating input", () => {
    const items: StockRowView[] = [
      { code: "000001.SZ", name: "平安银行", balancedScore: 15, score: 99 },
      { code: "600000.SH", name: "浦发银行", balancedScore: 18, score: 10 },
      { code: "000002.SZ", name: "万科A", score: 16 },
    ];

    const sorted = sortStocksByDisplayScore(items);

    expect(sorted.map((item) => item.code)).toEqual([
      "600000.SH",
      "000002.SZ",
      "000001.SZ",
    ]);
    expect(items.map((item) => item.code)).toEqual([
      "000001.SZ",
      "600000.SH",
      "000002.SZ",
    ]);
  });

  it("places missing scores at the end", () => {
    const items: StockRowView[] = [
      { code: "000001.SZ", name: "平安银行" },
      { code: "600000.SH", name: "浦发银行", score: 12 },
    ];

    expect(sortStocksByDisplayScore(items).map((item) => item.code)).toEqual([
      "600000.SH",
      "000001.SZ",
    ]);
  });
});