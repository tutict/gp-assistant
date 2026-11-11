import { describe, expect, it } from "vitest";
import { toHkSharedObserveResult } from "./hkObserveAdapter";
import type { HkObserveResult } from "../../types";
import { buildFundamentalSnapshotData } from "../../lib/fundamentalSnapshot";

function observation(overrides: Partial<HkObserveResult> = {}): HkObserveResult {
  return {
    market: "hk",
    security: { code: "00700", name: "腾讯控股", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "exchange_listed_equity" },
    quote: { code: "00700", name: "腾讯控股", price: 401.2, currency: "HKD", source: "Sina Finance", quote_time: "2026-11-04 15:05:46" },
    quote_capability: { market: "hk", id: "observe", status: "available", reason: null, coverage: null },
    history: [
      { date: "2026-10-16", open: 415, high: 420, low: 410, close: 417, volume_shares: 1000 },
      { date: "2026-10-19", open: 417, high: 425, low: 416, close: 419, volume_shares: 1200 },
    ],
    notes: [], stale: false,
    ...overrides,
  };
}

describe("toHkSharedObserveResult", () => {
  it("identifies Q3 as the latest disclosure while keeping H1 statement metrics in H1", () => {
    const source = observation({ financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026H1", normalized_period: "2026H1", report_date: "2026-06-30",
      candidate_source: "Eastmoney", candidate_fetched_at: "2026-11-07", verification_status: "unverified",
      eps: {value:0.0939,currency:"CNY",unit:"currency/share",source:"Eastmoney"},
    }, quarterly_update: {
      code:"00700",period:"2026Q3",period_end:"2026-09-30",disclosed_at:"2026-10-12",title:"Financial Update",
      source_name:"HKEXnews",source_url:"https://www1.hkexnews.hk/update.pdf",currency:"CNY",unit:"million",audit_status:"unaudited",fetched_at:"2026-11-07",
      quarterly_revenue:3946.2,nine_month_revenue:11155.1,quarterly_net_profit:365.1,nine_month_net_profit:937,
    } });
    const adapted=toHkSharedObserveResult(source);
    expect(adapted.financial_indicators?.latest_disclosure_period).toBe("2026Q3");
    expect(adapted.financial_indicators?.period).toBe("2026H1");
    expect(adapted.financial_indicators?.items?.find(item=>item.metric_key==="latest_eps")?.period).toBe("2026H1");
    expect(adapted.observation_metadata).toContainEqual(expect.objectContaining({report_period:"2026Q3",disclosed_at:"2026-10-12"}));
  });
  it("keeps the current quote separate from older daily closes", () => {
    const source = observation({ quote: {
      code: "00700", name: "腾讯控股", price: 401.2, previous_close: 397.2, change: 4,
      change_pct: 0.01007, currency: "HKD", source: "Sina Finance", quote_time: "2026-11-04 15:05:46",
    } });

    const adapted = toHkSharedObserveResult(source);

    expect(adapted.trend?.signal.close).toBe(401.2);
    expect(adapted.trend?.signal.previous_close).toBe(397.2);
    expect(adapted.trend?.signal.close_change).toBe(4);
    expect(adapted.trend?.signal.close_change_pct).toBe(0.01007);
    expect(adapted.trend?.signal.date).toBe("2026-11-04 15:05:46");
    expect(adapted.trend?.series.at(-1)?.close).toBe(419);
  });

  it("calculates a legacy quote's change using its previous price rather than its current price", () => {
    const source = observation();
    source.quote!.change_pct = 0.01;

    const adapted = toHkSharedObserveResult(source);

    expect(adapted.trend?.signal.previous_close).toBeCloseTo(397.227722772277);
    expect(adapted.trend?.signal.close_change).toBeCloseTo(3.972277227723);
    expect(adapted.trend?.signal.close_change_pct).toBe(0.01);
  });

  it("does not invent quote changes from an older daily-history pair", () => {
    const adapted = toHkSharedObserveResult(observation());

    expect(adapted.trend?.signal.close).toBe(401.2);
    expect(adapted.trend?.signal.previous_close).toBeUndefined();
    expect(adapted.trend?.signal.close_change).toBeUndefined();
    expect(adapted.trend?.signal.close_change_pct).toBeUndefined();
  });

  it("uses daily history explicitly when no quote is available", () => {
    const adapted = toHkSharedObserveResult(observation({ quote: null }));

    expect(adapted.trend?.signal.close).toBe(419);
    expect(adapted.trend?.signal.previous_close).toBe(417);
    expect(adapted.trend?.signal.close_change).toBe(2);
    expect(adapted.trend?.signal.date).toBe("2026-10-19");
  });

  it("formats large financial amounts in their original currency and exposes HK cash-flow metrics", () => {
    const source = observation({ financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年中报", normalized_period: "2026H1",
      candidate_source: "Eastmoney", candidate_fetched_at: "2026-11-07", verification_status: "unverified",
      revenue: { value: 401_243_000_000, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
      additional_metrics: {
        operating_cash_flow: { value: 107_263_000_000, currency: "CNY", unit: "currency_total", source: "Sina Finance" },
        current_ratio: { value: 1.76, currency: "", unit: "ratio", source: "Sina Finance" },
        diluted_eps: { value: 12.422, currency: "CNY", unit: "currency/share", source: "Sina Finance" },
      },
    } });

    const adapted = toHkSharedObserveResult(source);
    const snapshot = buildFundamentalSnapshotData(adapted.stock, adapted.financial_indicators);

    expect(snapshot.details).toEqual(expect.arrayContaining([
      expect.objectContaining({ label: "营业总收入", value: "CNY 4012.43亿" }),
      expect.objectContaining({ label: "经营现金流", value: "CNY 1072.63亿" }),
      expect.objectContaining({ label: "流动比率", value: "1.76" }),
      expect.objectContaining({ label: "摊薄每股收益", value: "CNY 12.422/股" }),
    ]));
    expect(snapshot.details.some(item => item.label === "质押总股本比")).toBe(false);
  });

  it("keeps non-finite values missing and does not label a CNY market cap as HKD", () => {
    const source = observation({ financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026H1", normalized_period: "2026H1",
      candidate_source: "Eastmoney", candidate_fetched_at: "2026-11-07", verification_status: "unverified",
      eps: { value: Number.NaN, currency: "CNY", unit: "currency/share", source: "Eastmoney", reason: "来源未返回有效每股收益" },
      market_cap: { value: 100_000_000, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
    } });

    const adapted = toHkSharedObserveResult(source);

    expect(adapted.stock.latest_eps).toBeUndefined();
    expect(adapted.stock.market_cap).toBeUndefined();
    expect(adapted.financial_indicators?.items).toContainEqual(expect.objectContaining({ metric_key: "latest_eps", value: "暂无", raw_value: null }));
    expect(adapted.notes).toContain("每股收益：来源未返回有效每股收益");
  });

  it.each([null, 0.5])("uses the quote dividend yield when the report value is %s", (reportYield) => {
    const source = observation({ financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026H1", normalized_period: "2026H1",
      candidate_source: "Eastmoney", candidate_fetched_at: "2026-11-07", verification_status: "unverified",
      dividend_yield: { value: reportYield, currency: "", unit: "percent", source: "Eastmoney" },
    } });
    source.quote!.dividend_yield_percent = 1.27;

    const adapted = toHkSharedObserveResult(source);
    const snapshot = buildFundamentalSnapshotData(adapted.stock, adapted.financial_indicators);

    expect(snapshot.details).toContainEqual(expect.objectContaining({ label: "股息率", value: "1.27%" }));
  });

  it("adapts HK quote, report metrics, dates and OHLC to the shared A/HK observation model", () => {
    const source: HkObserveResult = {
      market: "hk",
      security: {
        code: "00700",
        name: "TENCENT",
        name_zh: "騰訊控股",
        currency: "HKD",
        board_lot: 100,
        security_type: "Equity Securities",
        classification_status: "exchange_listed_equity",
      },
      quote: {
        code: "00700",
        name: "Tencent",
        name_zh: "騰訊控股",
        price: 419,
        change_pct: 0.012,
        currency: "HKD",
        source: "Tencent Finance",
        quote_time: "2026-11-04 16:00:00",
        fetched_at: "fetch-at",
        pe: 15.2,
        pb: 3.1,
        dividend_yield_percent: 5.58,
        total_shares: 9_000_000_000,
        circulating_shares: 8_900_000_000,
      },
      quote_capability: {
        market: "hk", id: "observe", status: "available", reason: null, coverage: null,
      },
      history: [
        { date: "2026-11-03", open: 415, high: 420, low: 410, close: 417, volume_shares: 1000 },
        { date: "2026-11-04", open: 417, high: 425, low: 416, close: 419, volume_shares: 1200 },
      ],
      financial: ({
        code: "00700",
        currency: "CNY",
        raw_report_period: "2026年中报",
        normalized_period: "2026H1",
        report_date: "2026-06-30",
        disclosed_at: "2026-08-25",
        candidate_source: "Eastmoney + Sina Finance HK statements",
        candidate_fetched_at: "financial-fetch-at",
        verification_status: "metadata_only",
        revenue: { value: 401_243_000_000, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
        net_profit: { value: 117_369_000_000, currency: "HKD", unit: "currency_total", source: "Sina Finance" },
        eps: { value: 12.639, currency: "CNY", unit: "currency/share", source: "Eastmoney" },
        roe: { value: 9.966, currency: "", unit: "percent", source: "Eastmoney" },
        additional_metrics: {
          revenue_yoy: { value: 10.07, currency: "", unit: "percent", source: "Eastmoney" },
          bps: { value: 124.925, currency: "HKD", unit: "currency/share", source: "Sina Finance" },
        },
      }) as unknown as HkObserveResult["financial"],
      candidate_updated_at: "financial-fetch-at",
      history_source: "Tencent Finance",
      history_updated_at: "history-fetch-at",
      notes: [],
      stale: false,
    };

    const adapted = toHkSharedObserveResult(source);

    expect(adapted.market).toBe("hk");
    expect(adapted.security_kind).toBe("listed_equity");
    expect(adapted.stock.code).toBe("00700.HK");
    expect(adapted.stock.name).toBe("騰訊控股");
    expect(adapted.stock.price).toBe(419);
    expect(adapted.stock.roe).toBeCloseTo(0.09966);
    expect(adapted.stock.dividend_yield).toBeCloseTo(0.0558);
    expect(adapted.trend?.series).toEqual([
      { date: "2026-11-03", open: 415, high: 420, low: 410, close: 417, volume: 1000 },
      { date: "2026-11-04", open: 417, high: 425, low: 416, close: 419, volume: 1200 },
    ]);
    expect(adapted.financial_indicators?.period).toBe("2026H1");
    expect(adapted.financial_indicators?.items).toEqual(expect.arrayContaining([
      expect.objectContaining({ metric_key: "latest_eps", value: "CNY 12.639/股" }),
      expect.objectContaining({ metric_key: "revenue_yoy", value: "10.07%" }),
      expect.objectContaining({ metric_key: "bps", value: "HKD 124.925/股" }),
    ]));
    expect(adapted.observation_metadata?.map((item) => item.source_id)).toEqual(expect.arrayContaining([
      "Tencent Finance", "Eastmoney + Sina Finance HK statements",
    ]));
  });

  it("keeps unknown financial units and missing values explicit", () => {
    const source = {
      market: "hk",
      security: { code: "00005", name: "HSBC", currency: "HKD", security_type: "Equity", classification_status: "exchange_listed_equity" },
      quote_capability: { market: "hk", id: "observe", status: "available", reason: null, coverage: null },
      history: [], notes: [], stale: false,
      financial: {
        code: "00005", currency: "USD", raw_report_period: "2026FY", normalized_period: "2026FY",
        candidate_source: "Sina Finance HK statements", candidate_fetched_at: "now", verification_status: "unverified",
        revenue: { value: null, currency: "USD", unit: "currency_total", source: "Sina Finance", reason: "field absent" },
      },
    } as unknown as HkObserveResult;

    const adapted = toHkSharedObserveResult(source);

    expect(adapted.financial_indicators?.items).toContainEqual(expect.objectContaining({
      metric_key: "operating_revenue", value: "暂无", period: "2026FY",
    }));
    expect(adapted.notes).toContain("港股财务候选值尚未按官方财报逐项核验。");
  });
});
