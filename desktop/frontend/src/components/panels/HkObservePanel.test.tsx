import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HkObservePanel } from "./HkObservePanel";
import { getJson, postJson } from "../../lib/tauri";

vi.mock("../../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));
const getJsonMock = vi.mocked(getJson);
const postJsonMock = vi.mocked(postJson);

function textContent(value: unknown): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map(textContent).join("");
  if (typeof value === "object" && value !== null && "children" in value) return textContent(value.children);
  return "";
}

const staleResult = {
  market: "hk",
  security: { code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "verified_ordinary_share" },
  quote: { code: "00700", name: "Tencent", price: 419, currency: "HKD", source: "Tencent Finance", fetched_at: "2026-10-19T12:00:00Z" },
  history: [{ date: "2026-10-18", open: 418, high: 420, low: 416, close: 419, volume_shares: 100 }],
  quote_updated_at: "2026-10-19T12:00:00Z",
  history_updated_at: "2026-10-18T16:00:00Z",
  quote_capability: {
    market: "hk",
    id: "observe",
    status: "available",
    reason: null,
    coverage: null,
  },
  quote_error: "报价服务暂时不可用",
  history_error: null,
  notes: ["报价：报价服务暂时不可用"],
  stale: true,
};

describe("HkObservePanel stability", () => {
  beforeEach(() => { getJsonMock.mockReset(); postJsonMock.mockReset(); });

  it("shows independent update times, field errors and an explicit refresh action", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("Tencent Finance");
    expect(text).toContain("2026-10-19T12:00:00Z");
    const quoteCapability = renderer.root.findByProps({ "aria-label": "观察能力状态" });
    expect(quoteCapability.findByType("strong").children.join("")).toBe("观察能力：可用");
    expect(text).toContain("报价服务暂时不可用");
    expect(renderer.root.findByProps({ children: "刷新行情" })).toBeTruthy();
  });

  it("shows an unavailable quote capability for an unsupported security currency", async () => {
    getJsonMock.mockResolvedValue({
      ...staleResult,
      security: { ...staleResult.security, currency: "CNY" },
      quote: null,
      quote_capability: {
        market: "hk",
        id: "observe",
        status: "unavailable",
        reason: "Tencent HK quote adapter does not support CNY",
        coverage: {
          requested_count: 1,
          available_count: 0,
          missing_count: 1,
          start_date: null,
          end_date: null,
          missing_fields: ["quote"],
        },
      },
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00012.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const quoteCapability = renderer.root.findByProps({ "aria-label": "观察能力状态" });
    const quoteCapabilityText = textContent(renderer.toJSON());
    expect(quoteCapability.findByType("strong").children.join("")).toBe("观察能力：不可用");
    expect(quoteCapabilityText).toContain("Tencent HK quote adapter does not support CNY");
    const quoteGrid = renderer.root.findByProps({ className: "hk-quote-grid" });
    expect(quoteGrid.findAllByType("strong")[0].children.join("")).toBe("CNY --");
    expect(quoteGrid.findAllByType("strong")[3].children.join("")).toBe("-- CNY");
  });

  it("refreshes only after the user presses the refresh button", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    expect(getJsonMock).toHaveBeenCalledTimes(1);
    expect(getJsonMock).toHaveBeenCalledWith("/api/hk/observe/00700.HK?data_policy=cache_only");
    await act(async () => { renderer.root.findByProps({ children: "刷新行情" }).props.onClick(); await Promise.resolve(); });
    const refreshUrl = getJsonMock.mock.calls.at(-1)?.[0] || "";
    expect(refreshUrl).toMatch(/^\/api\/hk\/observe\/00700\.HK\?data_policy=refresh&request_id=hk-observe-/);
  });

  it("keeps the last successful observation visible when refresh fails", async () => {
    getJsonMock.mockResolvedValueOnce({ ...staleResult, stale: false, notes: [] }).mockRejectedValueOnce(new Error("Tencent unavailable"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    await act(async () => { renderer.root.findByProps({ children: "刷新行情" }).props.onClick(); await Promise.resolve(); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("Tencent");
    expect(text).toContain("Tencent unavailable");
  });

  it("keeps a generic refresh failure separate from quote and history failures", async () => {
    getJsonMock.mockResolvedValueOnce({ ...staleResult, stale: false, notes: [] })
      .mockRejectedValueOnce(new Error("transport unavailable"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    await act(async () => { renderer.root.findByProps({ children: "刷新行情" }).props.onClick(); await Promise.resolve(); });

    const text = textContent(renderer.toJSON());
    expect(text).toContain("现价");
    expect(text).toContain("419");
    expect(text).toContain("2026-10-18");
    expect(text).toContain("刷新：transport unavailable");
    expect(text).not.toContain("报价：transport unavailable");
    expect(text).not.toContain("日 K：transport unavailable");
  });

  it("loads a cached financial snapshot without refreshing and refreshes only on explicit click", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, stale: false, notes: [], financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", report_date: "2026-06-30", disclosed_at: "2026-08-25", candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "2026-10-19T12:00:00Z", verification_status: "metadata_only", revenue: { value: 401243000000, currency: "CNY", unit: "yuan", source: "fixture" }, eps: { value: 12.639, currency: "CNY", unit: "currency/share", source: "fixture" },
    }, fx_rate: { pair: "HKD/CNY", rate: 0.85, source: "Frankfurter", fetched_at: "2026-10-19T12:00:00Z", observed_at: "2026-10-19" }, official_disclosure: { source_url: "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=EN", source_name: "HKEXnews", checked_at: "2026-10-19T12:00:00Z", status: "metadata_only" } });
    postJsonMock.mockResolvedValue({ ...staleResult, financial: { code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", candidate_source: "Eastmoney", candidate_fetched_at: "now", verification_status: "metadata_only" }, fx_rate: null, official_disclosure: null, notes: [], stale: false });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    expect(postJsonMock).not.toHaveBeenCalled();
    expect(JSON.stringify(renderer.toJSON())).toContain("财务快照");
    expect(JSON.stringify(renderer.toJSON())).toContain("1 HKD = 0.85 CNY");
    await act(async () => { renderer.root.findByProps({ children: "刷新财务" }).props.onClick(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/financial-refresh", { code: "00700.HK" });
  });

  it("renders report dates, currency conversions, metric units, source status and missing-field reasons", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, stale: false, notes: [], financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", report_date: "2026-06-30", disclosed_at: "2026-08-25", candidate_source: "Eastmoney", candidate_fetched_at: "2026-10-19T12:00:00Z", verification_status: "metadata_only",
      revenue: { value: 100, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
      net_profit: { value: null, currency: "CNY", unit: "currency_total", source: "Eastmoney", reason: "公开接口未返回净利润" },
      eps: { value: 2, currency: "CNY", unit: "currency/share", source: "Eastmoney" },
      roe: { value: 9.9, currency: "", unit: "percent", source: "Eastmoney" },
      pe: { value: 15.26, currency: "", unit: "ratio", source: "Tencent Finance" },
      pb: { value: 3.11, currency: "", unit: "ratio", source: "Tencent Finance" },
      dividend_yield: { value: 1.27, currency: "", unit: "percent", source: "Tencent Finance" },
      market_cap: { value: 200, currency: "HKD", unit: "currency_total", source: "Tencent Finance" },
      total_shares: { value: 1000, currency: "", unit: "shares", source: "Tencent Finance" },
      circulating_shares: { value: 900, currency: "", unit: "shares", source: "Tencent Finance" },
      field_errors: { net_profit: "公开接口未返回净利润" },
    }, candidate_updated_at: "2026-10-19T12:00:00Z", fx_updated_at: "2026-10-19T12:00:00Z", official_checked_at: "2026-10-19T12:00:00Z", financial_field_errors: { net_profit: "公开接口未返回净利润" }, fx_rate: { pair: "HKD/CNY", rate: 0.85, source: "Frankfurter", fetched_at: "2026-10-19T12:00:00Z", observed_at: "2026-10-19" }, official_disclosure: { source_url: "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=EN", source_name: "HKEXnews", checked_at: "2026-10-19T12:00:00Z", status: "metadata_only" } });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("报告日期");
    expect(text).toContain("HKD 117.65");
    expect(text).toContain("CNY 100");
    expect(text).toContain("HKD 2.35/股");
    expect(text).toContain("9.9%");
    expect(text).toContain("1.27%");
    expect(text).toContain("1,000 股");
    expect(text).toContain("公开接口未返回净利润");
    expect(text).toContain("HKEXnews");
  });

  it("keeps cached financial values and exposes a retry after financial refresh failure", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, stale: false, notes: [], financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", report_date: "2026-06-30", disclosed_at: "2026-08-25", candidate_source: "Eastmoney", candidate_fetched_at: "old-candidate", verification_status: "metadata_only", revenue: { value: 100, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
    }, fx_rate: { pair: "HKD/CNY", rate: 0.85, source: "Frankfurter", fetched_at: "old-fx", observed_at: "2026-10-18" }, official_disclosure: { source_url: "https://example.test/hkex", source_name: "HKEXnews", checked_at: "old-official", status: "metadata_only" } });
    postJsonMock.mockRejectedValue(new Error("财务源暂不可用"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    await act(async () => { renderer.root.findByProps({ children: "刷新财务" }).props.onClick(); await Promise.resolve(); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("财务源暂不可用");
    expect(text).toContain("old-candidate");
    expect(text).toContain("HKD 117.65");
  });

  it("shows only the source currency when no exchange rate is cached", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", candidate_source: "Eastmoney", candidate_fetched_at: "old", verification_status: "unverified",
      revenue: { value: 100, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
    }, fx_rate: null, official_disclosure: null });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const financialText = JSON.stringify(renderer.toJSON());
    expect(financialText).toContain("CNY 100");
    expect(financialText).toContain("不可用，仅显示原币");
    expect(financialText).not.toContain("HKD 117.65");
    expect(financialText).toContain("官方披露元数据不可用");
  });

  it("shows numeric verification conflicts even when official metadata is available", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", candidate_source: "Eastmoney", candidate_fetched_at: "old", verification_status: "conflict", verification_conflicts: ["revenue"],
      revenue: { value: 100, currency: "CNY", unit: "currency_total", source: "Eastmoney" },
      official_values: { revenue: { value: 120, currency: "CNY", unit: "currency_total", source: "Issuer annual report" } },
    }, fx_rate: { pair: "HKD/CNY", rate: 0.85, source: "Frankfurter", fetched_at: "fx", observed_at: "2026-10-19" }, official_disclosure: { source_url: "https://example.test/hkex", source_name: "HKEXnews", checked_at: "official", status: "metadata_only" } });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("候选值与官方值冲突");
    expect(text).toContain("HKD 141.18 · CNY 120");
    expect(text).toContain("候选值与官方记录存在差异");
  });
});
