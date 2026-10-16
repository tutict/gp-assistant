import { describe, expect, it } from "vitest";
import { buildMainFundFlowView, hasMainFundFlowBreakdown } from "./mainFundFlow";

describe("buildMainFundFlowView", () => {
  it("detects cached real-flow records that still lack the new breakdown", () => {
    const base = {
      category: "fund_flow" as const,
      source: "东方财富个股资金流",
      metrics: {
        主力净流入额: "1200 万",
        主力净占比: "4.6%",
      },
    };
    expect(hasMainFundFlowBreakdown({ stock_code: "000001.SZ", items: [base] })).toBe(false);
    expect(hasMainFundFlowBreakdown({ stock_code: "000001.SZ", items: [{
      ...base,
      metrics: {
        ...base.metrics,
        超大单净流入额: "800 万",
        大单净流入额: "400 万",
        中小单净流入额: "-1200 万",
      },
    }] })).toBe(true);
  });

  it("turns real outflow metrics into a plain-language conclusion", () => {
    const view = buildMainFundFlowView({
      stock_code: "600519.SH",
      as_of_trade_date: "2026-07-17",
      items: [{
        category: "fund_flow",
        source: "东方财富个股资金流",
        title: "当日主力资金流",
        date: "2026-07-17",
        metrics: {
          主力净流入额: "-8.54 亿",
          主力净流入额原值: "-854126672.00",
          主力净占比: "-11.66%",
          主力介入度: "高（11.66%）",
          超大单净流入额: "-1.61 亿",
          大单净流入额: "8192.89 万",
          中小单净流入额: "7927.31 万",
        },
        confidence: "中",
      }],
    });

    expect(view.available).toBe(true);
    expect(view.netAmount).toBe("-8.54 亿");
    expect(view.netRatio).toBe("-11.66%");
    expect(view.involvement).toBe("高（11.66%）");
    expect(view.superLargeAmount).toBe("-1.61 亿");
    expect(view.largeAmount).toBe("8192.89 万");
    expect(view.smallMediumAmount).toBe("7927.31 万");
    expect(view.tone).toBe("negative");
    expect(view.conclusion).toContain("每 100 元成交中");
    expect(view.conclusion).toContain("主力净卖出");
    expect(view.conclusion).toContain("单日资金不能单独当作买卖依据");
  });

  it("never treats the local price-volume proxy as real main-fund flow", () => {
    const view = buildMainFundFlowView({
      stock_code: "000100.SZ",
      items: [{
        category: "fund_flow",
        source: "Tauri/Rust 日线量价",
        title: "本地量价资金代理",
        date: "2026-07-17",
        metrics: {
          隐性资金代理分: "72",
          证据类型: "本地日线量价代理",
        },
      }],
    });

    expect(view.available).toBe(false);
    expect(view.netAmount).toBe("暂缺");
    expect(view.conclusion).toContain("不能替代主力净流入");
  });

  it("derives involvement from the absolute net ratio when the source omits it", () => {
    const view = buildMainFundFlowView({
      stock_code: "000001.SZ",
      items: [{
        category: "fund_flow",
        source: "外部资金流",
        metrics: {
          main_net_inflow: 32_000_000,
          main_net_ratio: 4.6,
        },
      }],
    });

    expect(view.netAmount).toBe("3200 万");
    expect(view.netRatio).toBe("4.6%");
    expect(view.involvement).toBe("中（4.6%）");
    expect(view.tone).toBe("positive");
  });

  it("warns instead of concluding when amount and ratio directions conflict", () => {
    const view = buildMainFundFlowView({
      stock_code: "000001.SZ",
      items: [{
        category: "fund_flow",
        source: "外部资金流",
        metrics: {
          主力净流入额: "1200 万",
          主力净占比: "-3.2%",
        },
      }],
    });

    expect(view.tone).toBe("neutral");
    expect(view.conclusion).toContain("方向不一致");
  });
});
