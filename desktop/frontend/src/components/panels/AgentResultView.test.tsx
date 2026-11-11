import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentResult, StockRowView, WatchlistItem } from "../../types";

const componentMocks = vi.hoisted(() => ({
  backtest: vi.fn(),
  news: vi.fn(),
  observe: vi.fn(),
  rawJson: vi.fn(),
  stockList: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({ openExternalUrl: componentMocks.openExternalUrl }));

vi.mock("../StockList", () => ({
  StockList: (props: Record<string, unknown>) => {
    componentMocks.stockList(props);
    return <div data-view="stock-list" />;
  },
}));

vi.mock("../RawJson", () => ({
  RawJson: (props: Record<string, unknown>) => {
    componentMocks.rawJson(props);
    return <div data-view="raw-json" />;
  },
}));

vi.mock("./BacktestPanel", () => ({
  BacktestResultView: (props: Record<string, unknown>) => {
    componentMocks.backtest(props);
    return <div data-view="backtest" />;
  },
}));

vi.mock("./NewsRagPanel", () => ({
  NewsRagView: (props: Record<string, unknown>) => {
    componentMocks.news(props);
    return <div data-view="news" />;
  },
}));

vi.mock("./ObservePanel", () => ({
  ObserveResultView: (props: Record<string, unknown>) => {
    componentMocks.observe(props);
    return <div data-view="observe" />;
  },
}));

let AgentResultView: typeof import("./AgentResultView").AgentResultView;

beforeAll(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ AgentResultView } = await import("./AgentResultView"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

beforeEach(() => {
  vi.resetAllMocks();
});

const watchlist: WatchlistItem[] = [{ code: "000001", name: "Ping An Bank" }];

function renderResult(
  result: AgentResult,
  onToggleWatchlist: (item: StockRowView) => void = vi.fn(),
) {
  const html = renderToStaticMarkup(
    <AgentResultView
      result={result}
      watchlist={watchlist}
      onToggleWatchlist={onToggleWatchlist}
    />,
  );
  return { html, onToggleWatchlist };
}

describe("AgentResultView", () => {
  it.each([["pointer", 1], ["keyboard", 0]])("keeps the assistant route and focuses the exact evidence card on %s activation", async (_activation, detail) => {
    const result = { reply: "查看原文 [E2]，未知 [E9]", evidence_summary: [{ id: "E1", title: "第一条" }, { id: "E2", title: "第二条" }] };
    let renderer: ReactTestRenderer | undefined;
    try {
      await act(async () => {
        renderer = create(<><AgentResultView result={result} watchlist={[]} onToggleWatchlist={vi.fn()} /><AgentResultView result={result} watchlist={[]} onToggleWatchlist={vi.fn()} /></>);
      });
      const links = renderer!.root.findAllByType("a");
      expect(links).toHaveLength(2);
      const link = links[1];
      const linkedCardId = String(link.props.href).slice(1);
      const articles = renderer!.root.findAllByType("article").filter(node => node.props.id);
      expect(articles.find(node => node.props.id === linkedCardId)?.props.tabIndex).toBe(-1);
      let focusedCardId = "";
      let scrolledCardId = "";
      let focusPreventedScroll = false;
      const cards = new Map(articles.map(node => [node.props.id, {
        id: node.props.id,
        scrollIntoView: () => { scrolledCardId = node.props.id; },
        focus: (options?: FocusOptions) => { focusedCardId = node.props.id; focusPreventedScroll = options?.preventScroll === true; },
      }]));
      const ownerDocument = { getElementById: (id: string) => cards.get(id) || null };
      const event = { detail, defaultPrevented: false, currentTarget: { ownerDocument }, preventDefault() { this.defaultPrevented = true; } };
      let route = "#agent";
      await act(async () => link.props.onClick?.(event));
      // Model the browser's default same-page navigation after the real click handler returns.
      if (!event.defaultPrevented) route = link.props.href;
      expect(route).toBe("#agent");
      expect(scrolledCardId).toBe(linkedCardId);
      expect(focusedCardId).toBe(linkedCardId);
      expect(focusPreventedScroll).toBe(true);
      expect(link.props["aria-label"]).toBe("查看证据 E2");
    } finally {
      if (renderer) await act(async () => renderer!.unmount());
    }
  });
  it("links reply prose to the same evidence cards without duplicating the reply", () => {
    const { html } = renderResult({ reply: "主回答 [E1] 与未知 [E9]", evidence_summary: [{ id: "E1", title: "原文" }] });
    const target = html.match(/href="#([^"]+-E1)"/)?.[1];
    expect(target).toBeTruthy();
    expect(html).toContain(`id="${target}"`);
    expect(html.match(/主回答/g)).toHaveLength(1);
    expect(html).not.toMatch(/href="#[^"]+-E9"/);
  });
  it("labels HK source tiers and disclosure status without implying an audit", () => {
    const { html } = renderResult({ evidence_summary: [
      { id: "E1", source_tier: "filing", title: "季度公告", verification_status: "official_disclosure_unaudited", audit_status: "unaudited" },
      { id: "E2", source_tier: "financial_snapshot", title: "财务数据", verification_status: "metadata_only", currency: "CNY", unit: "million" },
    ] });
    expect(html).toContain("官方公告");
    expect(html).toContain("官方披露，未经审计");
    expect(html).toContain("财务快照");
    expect(html).toContain("仅有披露线索，尚未核验");
    expect(html).toContain("百万元");
  });
  it("connects answer citations to dated source cards and preserves financial units", () => {
    const result = {
      action: "stock_research",
      answer_sections: [{ title: "盈利质量", bullets: ["收入 CNY 192869 百万元，未经审计。[E1]", "未知引用 [E9] 保留文字。"] }],
      evidence_summary: [{ id: "E1", title: "腾讯季度业绩公告", source: "HKEXnews", summary: "单季收入 CNY 192869 百万元。",
        published_at: "2026-11-09", report_period: "2026Q3", currency: "CNY", unit: "million", audit_status: "unaudited",
        url: "https://www.hkexnews.hk/listedco/listconews/sehk/2026/1109/tencent.pdf" }],
    };
    const { html } = renderResult(result);
    const target = html.match(/href="#([^"]+-E1)"/)?.[1];
    expect(target).toBeTruthy();
    expect(html).toContain(`id="${target}"`);
    expect(html).toContain("2026-11-09");
    expect(html).toContain("2026Q3");
    expect(html).toContain("CNY");
    expect(html).toContain("查看原文");
    expect(html).not.toMatch(/href="#[^"]+-E9"/);
  });

  it("opens the actual evidence source through the existing external URL action", async () => {
    const url = "https://www.hkexnews.hk/listedco/listconews/sehk/2026/1109/tencent.pdf";
    const result = { evidence_summary: [{ title: "腾讯公告", source: "HKEXnews", summary: "公告摘要", url }] };
    let renderer: ReactTestRenderer | undefined;
    try {
      await act(async () => { renderer = create(<AgentResultView result={result} watchlist={[]} onToggleWatchlist={vi.fn()} />); });
      const button = renderer!.root.findAllByType("button").find((node) => node.children.join("").includes("查看原文"));
      expect(button).toBeDefined();
      await act(async () => { await button!.props.onClick(); });
      expect(componentMocks.openExternalUrl).toHaveBeenCalledWith(url);
    } finally {
      if (renderer) await act(async () => renderer!.unmount());
    }
  });
  it("renders structured agent details with the legacy result fallback", () => {
    const result: AgentResult = {
      action: "data_status",
      tool_calls: [{
        id: "status-check",
        label: "Check local market data",
        status: "ok",
        output_summary: "Three datasets are current",
      }],
      evidence_summary: [{
        level: "primary",
        title: "Daily bars are current",
        summary: "Latest trade date is available",
        source: "Local data status",
      }],
      answer_sections: [{
        title: "Data readiness",
        bullets: ["All required datasets are ready."],
      }],
      warnings: ["Intraday quotes are delayed."],
      data: { status: "ready" },
    };

    const { html } = renderResult(result);

    expect(html).toContain("Check local market data");
    expect(html).toContain("Three datasets are current");
    expect(html).toContain("Daily bars are current");
    expect(html).toContain("Latest trade date is available");
    expect(html).toContain("Data readiness");
    expect(html).toContain("All required datasets are ready.");
    expect(html).toContain("Intraday quotes are delayed.");
    expect(html).toContain('data-view="raw-json"');
    expect(componentMocks.rawJson).toHaveBeenCalledWith({ result, enabled: true });
  });

  it("dispatches a complete nested backtest payload to BacktestResultView", () => {
    const backtest = {
      metrics: { total_return: 0.18, num_stocks: 1 },
      equity_curve: [{ date: "2026-08-08", equity: 1.18 }],
      symbols: ["000001.SZ"],
    };
    const result: AgentResult = { action: "backtest", backtest };

    const { html } = renderResult(result);

    expect(html).toContain('data-view="backtest"');
    expect(componentMocks.backtest).toHaveBeenCalledWith({ result: backtest });
    expect(componentMocks.rawJson).not.toHaveBeenCalled();
  });

  it("contains a specialized renderer failure to the result region", async () => {
    componentMocks.news.mockImplementation(() => {
      throw new Error("malformed news payload");
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const result: AgentResult = {
      action: "news_rag",
      warnings: ["Structured warning remains visible"],
      news_rag: { answer: "unsafe specialized payload" },
    };
    let renderer: ReactTestRenderer | undefined;

    try {
      await act(async () => {
        renderer = create(
          <AgentResultView result={result} watchlist={watchlist} onToggleWatchlist={vi.fn()} />,
        );
      });

      const unavailable = renderer!.root.findByProps({ role: "status" });
      expect(unavailable.children.join("")).toBe("结果不可用");
      expect(JSON.stringify(renderer!.toJSON())).toContain("Structured warning remains visible");
      expect(componentMocks.rawJson).not.toHaveBeenCalled();
    } finally {
      if (renderer) {
        await act(async () => renderer!.unmount());
      }
      consoleError.mockRestore();
    }
  });

  it.each([
    ["screen", "data", "000001"],
    ["sector_screen", "sector_screen", "000002"],
    ["graph_screen", "graph_screen", "000003"],
    ["trend_screen", "trend_screen", "000004"],
  ] as const)("dispatches %s rows and watchlist controls to StockList", (action, nestedKey, code) => {
    const nested = { items: [{ code, name: `${action} candidate`, price: 12.5 }] };
    const result = { action, [nestedKey]: nested } as AgentResult;
    const onToggleWatchlist = vi.fn<(item: StockRowView) => void>();

    const { html } = renderResult(result, onToggleWatchlist);

    expect(html).toContain('data-view="stock-list"');
    expect(componentMocks.stockList).toHaveBeenCalledWith({
      items: [expect.objectContaining({ code, name: `${action} candidate`, price: 12.5 })],
      watchlist,
      onToggleWatchlist,
    });
    expect(componentMocks.rawJson).not.toHaveBeenCalled();
  });

  it("dispatches the nested news payload to NewsRagView", () => {
    const news = { answer: "Policy support continues", citations: [{ title: "Source A" }] };
    const result: AgentResult = { action: "news_rag", news_rag: news };

    const { html } = renderResult(result);

    expect(html).toContain('data-view="news"');
    expect(componentMocks.news).toHaveBeenCalledWith({ result: news });
    expect(componentMocks.rawJson).not.toHaveBeenCalled();
  });

  it("dispatches the nested observation payload to ObserveResultView", () => {
    const observe = { code: "600519", signal: "hold" };
    const result: AgentResult = { action: "observe_stock", observe };

    const { html } = renderResult(result);

    expect(html).toContain('data-view="observe"');
    expect(componentMocks.observe).toHaveBeenCalledWith({ result: observe });
    expect(componentMocks.rawJson).not.toHaveBeenCalled();
  });

  it.each([
    ["unknown", "future_action"],
    ["data", "data_status"],
  ])("dispatches %s results to RawJson without discarding nested data", (_kind, action) => {
    const result: AgentResult = { action, data: { marker: `${action}-payload` } };

    const { html } = renderResult(result);

    expect(html).toContain('data-view="raw-json"');
    expect(componentMocks.rawJson).toHaveBeenCalledWith({ result, enabled: true });
    expect(componentMocks.backtest).not.toHaveBeenCalled();
    expect(componentMocks.news).not.toHaveBeenCalled();
    expect(componentMocks.observe).not.toHaveBeenCalled();
    expect(componentMocks.stockList).not.toHaveBeenCalled();
  });

  it("contains malformed structured items inside the local result boundary", async () => {
    const result = {
      action: "screen",
      tool_calls: [null],
      answer_sections: [null],
      data: { rows: [] },
    } as unknown as AgentResult;
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    let renderer: ReactTestRenderer | undefined;

    try {
      await act(async () => {
        renderer = create(
          <AgentResultView result={result} watchlist={watchlist} onToggleWatchlist={vi.fn()} />,
        );
      });

      const unavailable = renderer!.root.findAllByProps({ role: "status" });
      expect(unavailable.map((node) => node.children.join(""))).toContain("结果不可用");
    } finally {
      if (renderer) {
        await act(async () => renderer!.unmount());
      }
      consoleError.mockRestore();
    }
  });
});
