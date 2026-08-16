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
}));

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
    expect(componentMocks.rawJson).toHaveBeenCalledWith({ result });
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
    expect(componentMocks.rawJson).toHaveBeenCalledWith({ result });
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
