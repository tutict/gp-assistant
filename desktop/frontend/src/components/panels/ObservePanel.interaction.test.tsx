import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { getJsonMock } = vi.hoisted(() => ({ getJsonMock: vi.fn() }));

vi.mock("../../lib/tauri", () => ({ getJson: getJsonMock }));

import { ObservePanel } from "./ObservePanel";
import { currentSystemDateInputValue } from "../../lib/format";

function textContent(renderer: ReactTestRenderer): string {
  return renderer.root.findAll((node) => typeof node.children[0] === "string")
    .flatMap((node) => node.children)
    .filter((child): child is string => typeof child === "string")
    .join(" ");
}

function observeCalls() {
  return getJsonMock.mock.calls.filter((call) => String(call[0]).includes("/api/observe/"));
}

describe("ObservePanel requests", () => {
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("window", {
      location: { href: "http://localhost/" },
      setTimeout: () => 1,
      clearTimeout: () => undefined,
    });
    vi.stubGlobal("document", {
      activeElement: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    getJsonMock.mockImplementation((url: string) => {
      const target = String(url);
      const publicData = {
        capital_evidence: {
          stock_code: "600519.SH",
          items: [{
            category: "fund_flow",
            source: "东方财富个股资金流",
            metrics: {
              主力净流入额: "1200 万",
              主力净占比: "4.6%",
              超大单净流入额: "800 万",
              大单净流入额: "400 万",
              中小单净流入额: "-1200 万",
            },
          }],
        },
        market_active_brokerages: {
          status: "available" as const,
          source: "东方财富每日活跃营业部",
          trade_date: currentSystemDateInputValue(),
          checked_on: currentSystemDateInputValue(),
          total_departments: 0,
          items: [],
        },
        institutional_holdings: {
          status: "no_institutional_holders" as const,
          source: "东方财富前十大流通股东",
          checked_on: currentSystemDateInputValue(),
          report_period: "2026中报",
          report_date: "2026-06-30",
          disclosed_at: "2026-08-29",
          items: [],
        },
      };
      if (target.includes("/api/observe/600519.SH")) {
        return Promise.resolve({
          stock: { code: "600519.SH", name: "贵州茅台", industry: "白酒" },
          ...publicData,
          data_metadata: {
            data_policy: "cache_only",
            quote_as_of: "20261012",
            history_as_of: "20261012",
            coverage_count: 1,
            coverage: [{ code: "600519.SH", available_bars: 750, required_bars: 750 }],
          },
          notes: [],
        });
      }
      if (target.includes("/api/observe/000001.SZ")) {
        return Promise.resolve({ stock: { code: "000001.SZ", name: "平安银行" }, ...publicData, notes: [] });
      }
      return Promise.resolve({ stock: { code: "000000.SZ", name: "未知" }, notes: [] });
    });
  });

  afterEach(() => {
    getJsonMock.mockReset();
    vi.unstubAllGlobals();
  });

  it("runs once for an incoming stock and does not rerun when the code is edited", async () => {
    const onOpenNews = vi.fn();
    const onAskAgent = vi.fn();
    let renderer!: ReactTestRenderer;
    const props = {
      watchlist: [],
      onWatchlistChange: () => undefined,
      initialCode: "600519.SH",
      initialCodeRequestId: 1,
      onOpenNews,
      onAskAgent,
    };
    await act(async () => {
      renderer = create(<ObservePanel {...props} />);
    });
    await act(async () => { await Promise.resolve(); });

    expect(observeCalls()).toHaveLength(1);
    expect(String(observeCalls()[0][0])).toContain("data_policy=cache_only");
    expect(String(observeCalls()[0][0])).toContain("/api/observe/600519.SH");
    expect(textContent(renderer)).toContain("贵州茅台");
    expect(textContent(renderer)).not.toContain("仅供研究，不构成投资建议。");
    expect(textContent(renderer)).toContain("仅供选股研究，不构成投资建议；请结合公告、财报和自身风险承受能力独立判断。");
    expect(renderer.root.findAllByProps({ className: "observe-overview-data-summary" })).toHaveLength(1);
    const details = renderer.root.findByProps({ className: "observe-detail-disclosure" });
    expect(details.props.open).toBe(true);
    await act(async () => { details.props.onToggle({ currentTarget: { open: false } }); });
    expect(renderer.root.findByProps({ className: "observe-detail-disclosure" }).props.open).toBe(false);

    await act(async () => {
      renderer.update(<ObservePanel {...props} />);
    });
    expect(observeCalls()).toHaveLength(1);

    const input = renderer.root.find((node) => node.type === "input" && node.props.id === "observeCode");
    await act(async () => {
      input.props.onChange({ target: { value: "000001.SZ" } });
    });
    expect(input.props.value).toBe("000001.SZ");
    expect(observeCalls()).toHaveLength(1);

    const run = renderer.root.find((node) => node.type === "button" && node.children.includes("开始观察"));
    await act(async () => { await run.props.onClick(); });
    expect(observeCalls()).toHaveLength(2);
    expect(String(observeCalls()[1][0])).toContain("/api/observe/000001.SZ");

    await act(async () => {
      renderer.update(<ObservePanel {...props} initialCodeRequestId={2} />);
    });
    await act(async () => { await Promise.resolve(); });
    expect(observeCalls()).toHaveLength(3);
    expect(textContent(renderer)).toContain("贵州茅台");

    const news = renderer.root.find((node) => node.type === "button" && node.children.includes("消息"));
    const handoff = renderer.root.find((node) => node.type === "button" && node.children.includes("交给 Agent"));
    await act(async () => { news.props.onClick(); });
    await act(async () => { handoff.props.onClick(); });
    expect(onOpenNews).toHaveBeenCalledWith("600519.SH");
    expect(onAskAgent).toHaveBeenCalledTimes(1);
    expect(onAskAgent.mock.calls[0][0]).toContain("600519.SH");
    expect(onAskAgent.mock.calls[0][0]).toContain("贵州茅台");
    await act(async () => renderer.unmount());
  });


  it("drops an in-flight observation when the code is edited before it returns", async () => {
    let resolveRequest: (value: unknown) => void = () => undefined;
    getJsonMock.mockImplementation((url: string) => {
      if (String(url).includes("/api/observe/600519.SH")) {
        return new Promise((resolve) => { resolveRequest = resolve; });
      }
      return Promise.resolve({ stock: { code: "000001.SZ", name: "平安银行" }, notes: [] });
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
    });
    const input = renderer.root.find((node) => node.type === "input" && node.props.id === "observeCode");
    await act(async () => {
      input.props.onChange({ target: { value: "000001.SZ" } });
    });
    await act(async () => {
      resolveRequest({ stock: { code: "600519.SH", name: "贵州茅台" }, notes: [] });
      await Promise.resolve();
    });
    expect(textContent(renderer)).not.toContain("贵州茅台");
    expect(renderer.root.findByProps({ id: "observeCode" }).props.value).toBe("000001.SZ");
    await act(async () => renderer.unmount());
  });

  it("reports an invalid incoming code without requesting observation", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="not-a-code"
          initialCodeRequestId={1}
        />,
      );
    });
    expect(textContent(renderer)).toContain("请输入有效股票代码。");
    expect(observeCalls()).toHaveLength(0);
    await act(async () => renderer.unmount());
  });
  it("keeps observation local when data is unavailable so refresh stays in the toolbar", async () => {
    let renderer!: ReactTestRenderer;
    const props = { watchlist: [], onWatchlistChange: () => undefined, initialCode: "600519.SH", initialCodeRequestId: 1 };
    await act(async () => { renderer = create(<ObservePanel {...props} />); });
    expect(String(observeCalls()[0][0])).toContain("data_policy=cache_only");
    const button = renderer.root.find(node => node.type === "button" && String(node.props.className).includes("observe-run-btn"));
    expect(button.children).toContain("开始观察");
    expect(renderer.root.findAll(node => node.type === "select" && node.props["aria-label"] === "观察数据模式")).toHaveLength(0);
    await act(async () => { button.props.onClick(); await Promise.resolve(); });
    expect(String(observeCalls()[1][0])).toContain("data_policy=cache_only");
    await act(async () => renderer.unmount());
  });

  it("retries with an online history fill when local history coverage is insufficient", async () => {
    const missing = JSON.stringify({
      code: "LOCAL_DATA_MISSING",
      missing_count: 1,
      missing: [{ kind: "history", code: "600519.SH", available_bars: 87, required_bars: 750 }],
    });
    getJsonMock
      .mockRejectedValueOnce(new Error(missing))
      .mockResolvedValueOnce({ stock: { code: "600519.SH", name: "贵州茅台" }, notes: [] });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(observeCalls()).toHaveLength(2);
    expect(String(observeCalls()[0][0])).toContain("data_policy=cache_only");
    expect(String(observeCalls()[1][0])).toContain("data_policy=refresh");
    expect(textContent(renderer)).toContain("贵州茅台");
    await act(async () => renderer.unmount());
  });

  it("does not refresh or show hidden individual LHB seat details", async () => {
    const cached = {
      stock: { code: "600519.SH", name: "贵州茅台" },
      capital_evidence: {
        stock_code: "600519.SH",
        items: [
          {
            category: "fund_flow",
            source: "东方财富个股资金流",
            metrics: {
              主力净流入额: "1200 万",
              主力净占比: "4.6%",
              超大单净流入额: "800 万",
              大单净流入额: "400 万",
              中小单净流入额: "-1200 万",
            },
          },
          { category: "institution_lhb", title: "东方财富龙虎榜机构席位", seats: [], seat_detail_status: "partial" },
        ],
      },
      market_active_brokerages: {
        status: "available",
        source: "东方财富每日活跃营业部",
        checked_on: currentSystemDateInputValue(),
        trade_date: currentSystemDateInputValue(),
        total_departments: 0,
        items: [],
      },
      institutional_holdings: {
        status: "no_institutional_holders",
        source: "东方财富前十大流通股东",
        checked_on: currentSystemDateInputValue(),
        items: [],
      },
      notes: [],
    };
    getJsonMock.mockResolvedValueOnce(cached);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(observeCalls()).toHaveLength(1);
    expect(String(observeCalls()[0][0])).toContain("data_policy=cache_only");
    expect(textContent(renderer)).not.toContain("个股最近一次公开龙虎榜");
    expect(textContent(renderer)).not.toContain("机构专用");
    await act(async () => renderer.unmount());
  });

  it("refreshes a stale Q1 financial snapshot after the half-year report cutoff", async () => {
    const cached = {
      stock: { code: "600519.SH", name: "贵州茅台" },
      financial_indicators: { period: "2026Q1", items: [{ metric_key: "latest_eps", label: "每股收益", value: "1元" }] },
      notes: [],
    };
    const refreshed = {
      ...cached,
      financial_indicators: { period: "2026中报", items: [{ metric_key: "latest_eps", label: "每股收益", value: "2元" }] },
    };
    getJsonMock.mockResolvedValueOnce(cached).mockResolvedValueOnce(refreshed);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(observeCalls()).toHaveLength(2);
    expect(String(observeCalls()[0][0])).toContain("data_policy=cache_only");
    expect(String(observeCalls()[1][0])).toContain("data_policy=refresh");
    expect(textContent(renderer)).toContain("2026中报");
    await act(async () => renderer.unmount());
  });

  it("refreshes missing daily market intelligence and institutional holdings", async () => {
    const cached = { stock: { code: "688372.SH", name: "伟测科技" }, notes: [] };
    const refreshed = {
      ...cached,
      market_active_brokerages: {
        status: "available",
        source: "东方财富每日活跃营业部",
        trade_date: "2026-10-12",
        checked_on: currentSystemDateInputValue(),
        total_departments: 1,
        items: [{ rank: 1, name: "活跃营业部甲", buy_appearances: 5, sell_appearances: 2, total_appearances: 7, buy_amount: 20, sell_amount: 10, net_amount: 10, stock_count: 1 }],
      },
      institutional_holdings: {
        status: "available",
        source: "东方财富前十大流通股东",
        checked_on: currentSystemDateInputValue(),
        report_period: "2026中报",
        report_date: "2026-06-30",
        disclosed_at: "2026-08-29",
        items: [{ rank: 3, name: "某基金", type: "证券投资基金", shares: 100, ratio: 2, change: "加仓" }],
      },
    };
    getJsonMock.mockResolvedValueOnce(cached).mockResolvedValueOnce(refreshed);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<ObservePanel watchlist={[]} onWatchlistChange={() => undefined} initialCode="688372.SH" initialCodeRequestId={1} />);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(observeCalls()).toHaveLength(2);
    expect(String(observeCalls()[0][0])).toContain("data_policy=cache_only");
    expect(String(observeCalls()[1][0])).toContain("data_policy=refresh");
    expect(textContent(renderer)).toContain("活跃营业部甲");
    expect(textContent(renderer)).toContain("某基金");
    await act(async () => renderer.unmount());
  });

  it("renders market active brokerages and disclosed institutional holdings", async () => {
    getJsonMock.mockResolvedValueOnce({
      stock: { code: "688372.SH", name: "伟测科技" },
      capital_evidence: {
        stock_code: "688372.SH",
        items: [
          {
            category: "fund_flow",
            source: "东方财富个股资金流",
            metrics: {
              主力净流入额: "1200 万",
              主力净占比: "4.6%",
              超大单净流入额: "800 万",
              大单净流入额: "400 万",
              中小单净流入额: "-1200 万",
            },
          },
          {
            category: "institution_lhb",
            date: "2026-10-09",
            metrics: { "机构买入额": "300万元", "机构卖出额": "200万元", "机构净买额": "100万元" },
            seats: [{ name: "机构专用", direction: "buy", buy_amount: 3_000_000, sell_amount: 0, net_amount: 3_000_000 }],
            seat_detail_status: "complete",
          },
        ],
      },
      market_active_brokerages: {
        status: "available",
        source: "东方财富每日活跃营业部",
        trade_date: "2026-10-09",
        checked_on: currentSystemDateInputValue(),
        total_departments: 1,
        items: [{ rank: 1, code: "101", name: "活跃营业部甲", buy_appearances: 5, sell_appearances: 3, total_appearances: 8, buy_amount: 50_000_000, sell_amount: 20_000_000, net_amount: 30_000_000, stock_count: 4 }],
      },
      institutional_holdings: {
        status: "available",
        source: "东方财富前十大流通股东",
        checked_on: currentSystemDateInputValue(),
        report_period: "2026中报",
        report_date: "2026-06-30",
        disclosed_at: "2026-08-29",
        items: [{ rank: 2, name: "某公募基金", type: "证券投资基金", shares: 4_000_000, ratio: 2.4, change: "加仓", change_ratio: 74.5 }],
      },
      notes: [],
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="688372.SH"
          initialCodeRequestId={1}
        />,
      );
      await Promise.resolve();
    });

    const rendered = textContent(renderer);
    expect(observeCalls()).toHaveLength(1);
    expect(rendered).not.toContain("个股最近一次公开龙虎榜");
    expect(rendered).toContain("全市场每日活跃营业部");
    expect(rendered).toContain("活跃营业部甲");
    expect(rendered).toContain("机构持仓");
    expect(rendered).toContain("2026-06-30");
    expect(rendered).toContain("2026-08-29");
    expect(rendered).toContain("某公募基金");
    await act(async () => renderer.unmount());
  });

  it("does not retry ordinary observation errors", async () => {
    getJsonMock.mockRejectedValueOnce(new Error("网络请求失败"));
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
      await Promise.resolve();
    });
    expect(observeCalls()).toHaveLength(1);
    expect(textContent(renderer)).toContain("网络请求失败");
    await act(async () => renderer.unmount());
  });

  it("keeps professional details collapsed on Android", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
          mobileRuntime
        />,
      );
      await Promise.resolve();
    });
    expect(renderer.root.findByProps({ className: "observe-detail-disclosure" }).props.open).toBe(false);
    await act(async () => renderer.unmount());
  });

  it("reports an online fill failure separately from the initial local miss", async () => {
    const missing = JSON.stringify({
      code: "LOCAL_DATA_MISSING",
      missing_count: 1,
      missing: [{ kind: "history", code: "600519.SH", available_bars: 87, required_bars: 750 }],
    });
    getJsonMock.mockRejectedValueOnce(new Error(missing)).mockRejectedValueOnce(new Error(missing));
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(observeCalls()).toHaveLength(2);
    expect(textContent(renderer)).toContain("自动补齐失败，请稍后重试");
    expect(textContent(renderer)).not.toContain("请先点击顶部「刷新」补齐行情");
    await act(async () => renderer.unmount());
  });

});
