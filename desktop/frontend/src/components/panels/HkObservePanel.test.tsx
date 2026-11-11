import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HkObservePanel } from "./HkObservePanel";
import { getJson, postJson } from "../../lib/tauri";
import type { HkFinancialResult, HkObserveResult } from "../../types";

vi.mock("../../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));
vi.mock("../observe/ObserveCharts", () => ({
  TrendCharts: ({ series }: { series: unknown[] }) => <div className="trend-charts-test-double">{series.length} bars</div>,
}));
const getJsonMock = vi.mocked(getJson);
const postJsonMock = vi.mocked(postJson);

function textContent(value: unknown): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map(textContent).join("");
  if (typeof value === "object" && value !== null && "children" in value) return textContent(value.children);
  return "";
}

const staleHistory = Array.from({ length: 25 }, (_, index) => {
  const date = new Date(Date.UTC(2026, 8, 24 + index)).toISOString().slice(0, 10);
  return {
    date,
    open: 394 + index,
    high: 396 + index,
    low: 393 + index,
    close: 395 + index,
    volume_shares: 1000 + index,
  };
});

const staleResult = {
  market: "hk",
  security: { code: "00700", name: "Tencent", name_zh: "腾讯控股", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "verified_ordinary_share" },
  quote: { code: "00700", name: "Tencent", name_zh: "腾讯控股", price: 419, currency: "HKD", source: "Tencent Finance", fetched_at: "2026-10-19T12:00:00Z" },
  history: staleHistory,
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
  beforeEach(() => {
    getJsonMock.mockReset();
    postJsonMock.mockReset().mockRejectedValue(new Error("financial fixture unavailable"));
  });
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

  it("shows current trading facts with HKD and share units despite older history", async () => {
    const current: HkObserveResult = {
      ...staleResult,
      market: "hk",
      security: { ...staleResult.security, classification_status: "verified_ordinary_share" },
      quote_capability: { ...staleResult.quote_capability, market: "hk", id: "observe", status: "available" },
      quote: { ...staleResult.quote, price: 401.2, previous_close: 397.2, open: 397.8, high: 402.2, low: 391.6, change: 4, change_pct: 0.01007, volume_shares: 13_339_378, turnover_hkd: 5_301_982_786.6, quote_time: "2026-11-04 15:05:46" },
      quote_error: null, notes: [], stale: false,
    };
    getJsonMock.mockResolvedValue(current);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const keyMetrics = textContent(renderer.root.findByProps({ "aria-label": "关键数值" }));
    expect(keyMetrics).toContain("401.20");
    expect(keyMetrics).toContain("+1.01%");
    const trading = textContent(renderer.root.findByProps({ "aria-label": "港股交易信息" }));
    expect(trading).toContain("昨收397.20");
    expect(trading).toContain("今开397.80");
    expect(trading).toContain("最高402.20");
    expect(trading).toContain("最低391.60");
    expect(trading).toContain("成交量1333.94万股");
    expect(trading).toContain("成交额HKD 53.02亿");
    expect(trading).toContain("每手股数100股");
    expect(trading).toContain("一手金额（参考）HKD 4.01万");
    await act(async () => renderer.unmount());
  });

  it("preserves three-decimal HK prices in the observation summary", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, quote: { ...staleResult.quote, price: 0.005, change_pct: 0 }, notes: [] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    expect(textContent(renderer.root.findByProps({ "aria-label": "关键数值" }))).toContain("0.005");
    await act(async () => renderer.unmount());
  });

  it("keeps HK percentage units correct for gains above 100 percent", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, quote: { ...staleResult.quote, price: 900, previous_close: 400, change: 500, change_pct: 1.25 }, notes: [] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    expect(textContent(renderer.root.findByProps({ "aria-label": "关键数值" }))).toContain("+125.00%");
    expect(textContent(renderer.root.findByProps({ "aria-label": "核心判断" }))).toContain("近端涨跌幅为 +125.00%");
    await act(async () => renderer.unmount());
  });

  it("does not borrow the daily-history date for a quote with an unknown timestamp", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const valuation = renderer.root.findAllByProps({ className: "observe-text-metric-section" })
      .find(section => textContent(section.findByType("h4")) === "行情与估值")!;
    expect(textContent(valuation)).not.toContain("2026-10-18");
    expect(textContent(renderer.root.findByProps({ "aria-label": "港股交易信息" }))).toContain("日 K 截至 2026-10-18");
    await act(async () => renderer.unmount());
  });

  it("refreshes a missing quote even when cached Chinese identity and history are complete", async () => {
    getJsonMock.mockResolvedValueOnce({ ...staleResult, quote: null, quote_error: "旧新浪报价缓存需要重新获取最新价" })
      .mockResolvedValueOnce({ ...staleResult, quote: { ...staleResult.quote, price: 401.2 }, quote_error: null });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    expect(textContent(renderer.root.findByProps({ "aria-label": "关键数值" }))).toContain("401.20");
    expect(getJsonMock.mock.calls.some(([url]) => String(url).includes("data_policy=refresh"))).toBe(true);
    await act(async () => renderer.unmount());
  });

  it("uses HK net-profit growth in the financial overview", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026H1", normalized_period: "2026H1",
      candidate_source: "Eastmoney", candidate_fetched_at: "2026-11-07", verification_status: "unverified",
      eps: { value: 12.639, currency: "CNY", unit: "currency/share", source: "Eastmoney" },
      additional_metrics: { net_profit_yoy: { value: 12.68, currency: "", unit: "percent", source: "Eastmoney" } },
    } });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const overview = textContent(renderer.root.findByProps({ "aria-label": "核心判断" }));
    expect(overview).toContain("归母净利同比 12.68%");
    expect(overview).not.toContain("扣非增长");
    await act(async () => renderer.unmount());
  });

  it("retries a failed quarter check even when H1 financials and a check timestamp are cached", async () => {
    const snapshot={code:"00700",currency:"CNY",raw_report_period:"2026H1",normalized_period:"2026H1",report_date:"2026-06-30",candidate_source:"Eastmoney",candidate_fetched_at:"2026-11-07",verification_status:"unverified"};
    getJsonMock.mockResolvedValue({...staleResult,financial:snapshot,quarterly_update_checked_at:String(Date.now()),quarterly_update_error:"HKEX update does not contain a recognized quarter-end date"});
    postJsonMock.mockResolvedValue({market:"hk",code:"00700",snapshot,quarterly_update:{code:"00700",period:"2026Q3",period_end:"2026-09-30",disclosed_at:"2026-10-12",title:"Financial Update",source_name:"HKEXnews",source_url:"https://www1.hkexnews.hk/update.pdf",currency:"CNY",unit:"million",audit_status:"unaudited",fetched_at:String(Date.now()),quarterly_revenue:3946.2,nine_month_revenue:11155.1},quarterly_update_checked_at:String(Date.now()),quarterly_update_error:null,notes:[],field_errors:{},stale:false});
    let renderer!:ReactTestRenderer;
    await act(async()=>{renderer=create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()}/>);});
    expect(textContent(renderer.toJSON())).toContain("最新披露2026Q3");
    await act(async()=>renderer.unmount());
  });

  it("checks again when the cached quarter lookup is older than a day", async () => {
    const snapshot={code:"00700",currency:"CNY",raw_report_period:"2026H1",normalized_period:"2026H1",report_date:"2026-06-30",candidate_source:"Eastmoney",candidate_fetched_at:"2026-11-07",verification_status:"unverified"};
    getJsonMock.mockResolvedValue({...staleResult,financial:snapshot,quarterly_update_checked_at:String(Date.now()-2*24*60*60*1000),quarterly_update_error:null});
    postJsonMock.mockResolvedValue({market:"hk",code:"00700",snapshot,quarterly_update:{code:"00700",period:"2026Q3",period_end:"2026-09-30",disclosed_at:"2026-10-12",title:"Financial Update",source_name:"HKEXnews",source_url:"https://www1.hkexnews.hk/update.pdf",currency:"CNY",unit:"million",audit_status:"unaudited",fetched_at:String(Date.now()),quarterly_revenue:3946.2,nine_month_revenue:11155.1},quarterly_update_checked_at:String(Date.now()),notes:[],field_errors:{},stale:false});
    let renderer!:ReactTestRenderer;
    await act(async()=>{renderer=create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()}/>);});
    expect(textContent(renderer.toJSON())).toContain("最新披露2026Q3");
    await act(async()=>renderer.unmount());
  });

  it("reuses the observation search frame and finds HK stocks by Chinese name", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", { addEventListener: vi.fn(), removeEventListener: vi.fn() });
    vi.stubGlobal("window", {
      setTimeout: globalThis.setTimeout.bind(globalThis),
      clearTimeout: globalThis.clearTimeout.bind(globalThis),
    });
    getJsonMock
      .mockResolvedValueOnce({ market: "hk", items: [{ code: "00700", name: "Tencent", name_zh: "腾讯控股" }] })
      .mockResolvedValueOnce({
        ...staleResult,
        security: { ...staleResult.security, name_zh: "腾讯控股" },
        quote: { ...staleResult.quote, name_zh: "腾讯控股" },
      });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HkObservePanel code="" watchlist={[]} onWatchlistChange={vi.fn()} />);
    });

    const input = renderer.root.findAllByType("input").find((node) => node.props.id === "hkObserveCode")!;
    await act(async () => input.props.onChange({ target: { value: "腾讯" } }));
    await act(async () => { await vi.advanceTimersByTimeAsync(200); });

    expect(getJsonMock).toHaveBeenCalledWith("/api/hk/stock-search?q=%E8%85%BE%E8%AE%AF&limit=6");
    expect(JSON.stringify(renderer.toJSON())).toContain("腾讯控股");
    const suggestion = renderer.root.findAllByType("button").find((button) => button.props.role === "option");
    expect(suggestion).toBeTruthy();
    await act(async () => {
      suggestion!.props.onClick();
      await Promise.resolve();
    });

    expect(getJsonMock).toHaveBeenLastCalledWith("/api/hk/observe/00700.HK?data_policy=cache_only");
    expect(textContent(renderer.root.findByType("h2"))).toContain("腾讯控股");
  });

  it("renders the shared A-share observation layout and keeps an explicit financial refresh action", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    expect(renderer.root.findByProps({ className: "observe-fundamental-snapshot" })).toBeTruthy();
    expect(renderer.root.findAllByProps({ className: "observe-chart-card" })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ className: "hk-quote-grid" })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ className: "hk-daily-table" })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ children: "刷新财务" })).toHaveLength(1);
  });

  it("shows the source failure and retry guidance when HK daily history is unavailable", async () => {
    getJsonMock.mockResolvedValue({
      ...staleResult,
      history: [],
      history_error: "Tencent history blocked",
      notes: [],
    });
    let renderer!: ReactTestRenderer;
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    await act(async () => {
      renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    const status = renderer.root.findAllByType("p").find((node) => String(node.props.className || "").includes("hk-history-status"));
    expect(status).toBeTruthy();
    expect(textContent(status)).toContain("0 条");
    expect(textContent(status)).toContain("Tencent history blocked");
    expect(textContent(status)).toContain("刷新行情");
    expect(textContent(status)).toContain("腾讯日 K");
    await act(async () => renderer.unmount());
  });

  it("marks unsupported HK capital and brokerage data without claiming A-share sources", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const text = textContent(renderer.toJSON());
    expect(text).toContain("港股资金流数据源暂未接入");
    expect(text).toContain("港股营业部与龙虎榜数据源暂未接入");
    expect(text).toContain("港股机构持仓数据源暂未接入");
    expect(text).not.toContain("东方财富个股资金流");
    expect(text).not.toContain("东方财富公开龙虎榜统计");
    await act(async () => renderer.unmount());
  });

  it("still identifies a nonempty malformed route as an invalid HK code", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HkObservePanel code="600000.SH" watchlist={[]} onWatchlistChange={vi.fn()} />);
    });
    expect(textContent(renderer.toJSON())).toContain("请输入有效港股代码");
    expect(getJsonMock).not.toHaveBeenCalled();
  });

  it("shows independent update times, field errors and an explicit refresh action", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("observe-fundamental-snapshot");
    expect(text).toContain("observe-chart-card");
    expect(text).not.toContain("hk-quote-grid");
    expect(text).not.toContain("hk-daily-table");
    expect(text).toContain("Tencent Finance");
    expect(text).toContain("2026-10-19T12:00:00Z");
    expect(text).toContain("报价服务暂时不可用");
    expect(renderer.root.findByProps({ children: "刷新行情" })).toBeTruthy();
  });

  it("uses the cached Chinese security name in the observation heading", async () => {
    getJsonMock.mockResolvedValue({
      ...staleResult,
      security: { ...staleResult.security, name: "CHINA RAIL CONS", name_zh: "中国中铁" },
      quote: { ...staleResult.quote, name_zh: undefined },
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="01186.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    expect(textContent(renderer.root.findByType("h2"))).toContain("中国中铁");
    expect(textContent(renderer.root.findByType("h2"))).not.toContain("CHINA RAIL CONS");
  });

  it("observes an HKEX-listed equity while showing that the source does not distinguish share class", async () => {
    getJsonMock.mockResolvedValue({
      ...staleResult,
      security: { ...staleResult.security, classification_status: "exchange_listed_equity" },
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const text = textContent(renderer.toJSON());
    expect(text).toContain("HKEX 已将证券列入 Equity Securities");
    expect(text).toContain("普通股或优先股细分");
    expect(renderer.root.findByType("h2").children.join("")).toContain("腾讯控股");
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

    const quoteCapabilityText = textContent(renderer.toJSON());
    expect(quoteCapabilityText).toContain("Tencent HK quote adapter does not support CNY");
    expect(quoteCapabilityText).toContain("报价币种 CNY");
    expect(quoteCapabilityText).toContain("暂无");
    expect(quoteCapabilityText).not.toContain("hk-quote-grid");
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

  it("refreshes an incomplete daily-history cache when opening an HK observation", async () => {
    const incomplete = { ...staleResult, history: [] };
    const complete = {
      ...staleResult,
      history: Array.from({ length: 25 }, (_, index) => ({
        date: `2026-10-${String(index + 1).padStart(2, "0")}`,
        open: 400 + index,
        high: 402 + index,
        low: 399 + index,
        close: 401 + index,
        volume_shares: 1000 + index,
      })),
      history_error: null,
    };
    getJsonMock.mockResolvedValueOnce(incomplete).mockResolvedValueOnce(complete);
    let renderer!: ReactTestRenderer;
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    await act(async () => {
      renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(getJsonMock).toHaveBeenCalledTimes(2);
    expect(getJsonMock.mock.calls[0][0]).toBe("/api/hk/observe/00700.HK?data_policy=cache_only");
    expect(getJsonMock.mock.calls[1][0]).toMatch(/^\/api\/hk\/observe\/00700\.HK\?data_policy=refresh&request_id=hk-observe-/);
    await act(async () => renderer.unmount());
  });

  it("refreshes an English-only cached name so the HK observation can show Chinese", async () => {
    const englishCache = {
      ...staleResult,
      security: { ...staleResult.security, code: "09677", name: "WEIHAI BANK", name_zh: undefined },
      quote: { ...staleResult.quote, code: "09677", name: "WEIHAI BANK", name_zh: undefined },
      history: staleHistory,
    };
    const enriched = {
      ...englishCache,
      security: { ...englishCache.security, name_zh: "威海银行" },
      quote: { ...englishCache.quote, name_zh: "威海银行" },
    };
    getJsonMock.mockResolvedValueOnce(englishCache).mockResolvedValueOnce(enriched);
    let renderer!: ReactTestRenderer;
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    await act(async () => {
      renderer = create(<HkObservePanel code="09677.HK" watchlist={[]} onWatchlistChange={vi.fn()} />);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(getJsonMock).toHaveBeenCalledTimes(2);
    expect(textContent(renderer.root.findByType("h2"))).toContain("威海银行");
    expect(textContent(renderer.root.findByType("h2"))).not.toContain("WEIHAI BANK");
    await act(async () => renderer.unmount());
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
    expect(text).toContain("最新价");
    expect(text).toContain("419");
    expect(text).toContain("2026-10-18");
    expect(text).toContain("刷新：transport unavailable");
    expect(text).not.toContain("报价：transport unavailable");
    expect(text).not.toContain("日 K：transport unavailable");
    expect(textContent(renderer.root.findByProps({ "aria-label": "港股交易信息" }))).toContain("transport unavailable");
  });

  it("loads cached H1 financials and checks for a newer HKEX quarter update", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, stale: false, notes: [], financial: {
      code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", report_date: "2026-06-30", disclosed_at: "2026-08-25", candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "2026-10-19T12:00:00Z", verification_status: "metadata_only", revenue: { value: 401243000000, currency: "CNY", unit: "yuan", source: "fixture" }, eps: { value: 12.639, currency: "CNY", unit: "currency/share", source: "fixture" },
    }, fx_rate: { pair: "HKD/CNY", rate: 0.85, source: "Frankfurter", fetched_at: "2026-10-19T12:00:00Z", observed_at: "2026-10-19" }, official_disclosure: { source_url: "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=EN", source_name: "HKEXnews", checked_at: "2026-10-19T12:00:00Z", status: "metadata_only" } });
    postJsonMock.mockResolvedValue({ market: "hk", code: "00700", snapshot: { code: "00700", currency: "CNY", raw_report_period: "2026年 中报", normalized_period: "2026H1", candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "now", verification_status: "metadata_only", revenue: { value: 401243000000, currency: "CNY", unit: "yuan", source: "fixture" } }, quarterly_update_checked_at: "2026-11-07T10:00:00Z", fx_rate: null, official_disclosure: null, notes: [], stale: false, field_errors: {} });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(postJsonMock).toHaveBeenCalledTimes(1);
    const initialText = textContent(renderer.toJSON());
    expect(initialText).toContain("基本面快照");
    expect(initialText).toContain("Eastmoney HK financial indicators");
    expect(initialText).toContain("401243000000");
    await act(async () => { renderer.root.findByProps({ children: "刷新财务" }).props.onClick(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledTimes(2);
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/financial-refresh", { code: "00700.HK" });
  });

  it("automatically fetches the HK financial snapshot when the selected stock has no cached snapshot", async () => {
    getJsonMock.mockResolvedValue({ ...staleResult, financial: null, financial_notes: [] });
    postJsonMock.mockResolvedValue({
      market: "hk",
      code: "00700",
      snapshot: {
        code: "00700",
        currency: "HKD",
        raw_report_period: "2026年三季报",
        normalized_period: "2026Q3",
        report_date: "2026-09-30",
        disclosed_at: "2026-10-28",
        candidate_source: "Eastmoney HK financial indicators",
        candidate_fetched_at: "2026-11-07T10:00:00Z",
        verification_status: "unverified",
        eps: { value: 0.62, currency: "HKD", unit: "currency/share", source: "Eastmoney" },
      },
      notes: [],
      stale: false,
      field_errors: {},
    } satisfies HkFinancialResult);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(postJsonMock).toHaveBeenCalledTimes(1);
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/financial-refresh", { code: "00700.HK" });
    expect(textContent(renderer.toJSON())).toContain("2026Q3");
    expect(textContent(renderer.toJSON())).toContain("HKD 0.62/股");
    await act(async () => renderer.unmount());
  });

  it("shows automatic HK financial refresh failures in the fundamental snapshot", async () => {
    getJsonMock.mockResolvedValue({
      ...staleResult,
      security: { ...staleResult.security, code: "06969", name_zh: "思摩尔国际" },
      quote: { ...staleResult.quote, code: "06969", name_zh: "思摩尔国际" },
      financial: null,
      financial_notes: [],
    });
    postJsonMock.mockRejectedValue(new Error("Eastmoney timed out; Sina returned 403"));
    let renderer!: ReactTestRenderer;
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    await act(async () => {
      renderer = create(<HkObservePanel code="06969.HK" watchlist={[]} onWatchlistChange={vi.fn()} />);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(postJsonMock).toHaveBeenCalledTimes(1);
    const snapshot = renderer.root.findByProps({ className: "observe-fundamental-snapshot" });
    const message = snapshot.findAllByType("p").map(textContent).join(" ");
    expect(message).toContain("Eastmoney timed out; Sina returned 403");
    expect(textContent(snapshot)).toContain("刷新财务");
    await act(async () => renderer.unmount());
  });

  it("renders report dates, source currency, shared metric units and missing-field reasons", async () => {
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
      field_errors: { net_profit: "公开接口未返回净利润", sina_supplement: "新浪返回与主报告期不一致" },
    }, candidate_updated_at: "2026-10-19T12:00:00Z", fx_updated_at: "2026-10-19T12:00:00Z", official_checked_at: "2026-10-19T12:00:00Z", financial_field_errors: { net_profit: "公开接口未返回净利润", sina_supplement: "新浪返回与主报告期不一致" }, fx_rate: { pair: "HKD/CNY", rate: 0.85, source: "Frankfurter", fetched_at: "2026-10-19T12:00:00Z", observed_at: "2026-10-19" }, official_disclosure: { source_url: "https://www1.hkexnews.hk/search/titlesearch.xhtml?lang=EN", source_name: "HKEXnews", checked_at: "2026-10-19T12:00:00Z", status: "metadata_only" } });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("报告 2026-06-30");
    expect(text).toContain("披露 2026-08-25");
    expect(text).toContain("CNY 100");
    expect(text).toContain("CNY 2/股");
    expect(text).toContain("9.9%");
    expect(text).toContain("1.27%");
    expect(text).toContain("总股本");
    expect(text).toContain("公开接口未返回净利润");
    expect(text).toContain("新浪补充未完成：新浪返回与主报告期不一致");
    expect(text).toContain("HKEXnews");
    expect(text).toContain("金额按来源原币展示");
  });

  it("shows an unaudited HKEX Q3 update alongside the separate H1 snapshot", async () => {
    getJsonMock.mockResolvedValue({
      ...staleResult,
      security: { ...staleResult.security, code: "06969" },
      quote: { ...staleResult.quote, code: "06969" },
      financial: {
        code: "06969", currency: "CNY", raw_report_period: "2026 interim", normalized_period: "2026H1",
        report_date: "2026-06-30",
        candidate_source: "Eastmoney", candidate_fetched_at: "2026-08-25", verification_status: "unverified",
      },
      quarterly_update: {
        code: "06969", period: "2026Q3", period_end: "2026-09-30", disclosed_at: "2026-10-12",
        title: "Financial Update for the Period Ended 30 September 2026", source_name: "HKEXnews",
        source_url: "https://www1.hkexnews.hk/update.pdf", currency: "CNY", unit: "million",
        audit_status: "unaudited", fetched_at: "2026-11-07T10:00:00Z",
        quarterly_revenue: 3946.2, quarterly_revenue_yoy_percent: -6, quarterly_revenue_qoq_percent: 17.7,
        nine_month_revenue: 11155.1, nine_month_revenue_yoy_percent: 9.3,
        quarterly_net_profit: 365.1, quarterly_net_profit_yoy_percent: 15.2, quarterly_net_profit_qoq_percent: 18,
        nine_month_net_profit: 937, nine_month_net_profit_yoy_percent: 15.8,
        quarterly_total_comprehensive_income: 294.3, quarterly_total_comprehensive_income_yoy_percent: 3.6,
        quarterly_total_comprehensive_income_qoq_percent: 107.1,
        nine_month_total_comprehensive_income: 564.3, nine_month_total_comprehensive_income_yoy_percent: -28.2,
      },
      quarterly_update_checked_at: "2026-11-07T10:00:00Z",
      quarterly_update_error: "HKEX request timed out",
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="06969.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });

    const text = textContent(renderer.toJSON());
    expect(text).toContain("2026H1");
    expect(text).toContain("2026Q3 经营更新 · 未经审计");
    expect(text).toContain("2026Q3");
    expect(text).toContain("CNY 3,946.2 million");
    expect(text).toContain("同比 -6.0%");
    expect(text).toContain("环比 +17.7%");
    expect(text).toContain("CNY 11,155.1 million");
    expect(text).toContain("同比 +15.2% · 环比 +18.0%");
    expect(text).toContain("CNY 294.3 million");
    expect(text).toContain("同比 +3.6% · 环比 +107.1%");
    expect(text).toContain("CNY 564.3 million");
    expect(text).toContain("HKEXnews");
    expect(textContent(renderer.root.findByProps({ "aria-label": "港股季度经营更新" }))).toContain("HKEX request timed out");
    const fundamental=renderer.root.findByProps({"aria-label":"最新基本面"});
    expect(fundamental.findAllByProps({"aria-label":"港股季度经营更新"})).toHaveLength(1);
    expect(textContent(fundamental)).toContain("最新披露2026Q3");
    expect(textContent(fundamental)).toContain("2026H1 报表指标");
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
    expect(text).toContain("CNY 100");
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
    expect(financialText).toContain("金额按来源原币展示");
    expect(financialText).not.toContain("HKD 117.65");
    expect(financialText).toContain("HKEXnews");
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
    expect(text).toContain("候选值与官方披露存在冲突：revenue");
    expect(text).toContain("CNY 100");
  });
});
