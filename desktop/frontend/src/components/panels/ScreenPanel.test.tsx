import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

let ScreenResultView: typeof import("./ScreenPanel").ScreenResultView;

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ ScreenResultView } = await import("./ScreenPanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

describe("ScreenResultView", () => {
  it("shows hydrated EPS values from HK financial caches in the stock list", () => {
    const markup = renderToStaticMarkup(<ScreenResultView result={{ total: 1, returned: 1,
      items: [{ stock: { code: "01522.HK", name: "京投交通科技", price: 0.19, latest_eps: 0.23 }, score: 15.31, reasons: [] }],
    }} grouped={false} watchlist={[]} onToggleWatchlist={() => undefined} />);

    expect(markup.match(/<td class="stock-eps-column">([^<]+)/)?.[1]).toBe("0.23");
  });

  it("shows Chinese HK names supplied by the screening result", () => {
    const markup = renderToStaticMarkup(<ScreenResultView result={{ total: 1, returned: 1,
      items: [{ stock: { code: "01186.HK", name: "中国中铁", price: 4.11 }, score: 16.96, reasons: [] }],
    }} grouped={false} watchlist={[]} onToggleWatchlist={() => undefined} />);

    expect(markup).toContain("中国中铁");
    expect(markup).not.toContain("CHINA RAIL CONS");
  });

  it("keeps history coverage warnings in expandable notes while retaining other screening notes", () => {
    const markup = renderToStaticMarkup(<ScreenResultView result={{ total: 1, returned: 1,
      items: [{ stock: { code: "688279.SH", name: "峰岹科技", price: 173.73 }, score: 10.46, reasons: [] }],
      notes: [
        "14 只候选的日线落后行情，未使用旧指标参与当前趋势排名。",
        "605358.SH：日线截至 2026-09-04，落后于 2026-10-30 的行情，需补齐日线后重新计算依据。",
        "Skipped 2 stocks without usable OHLC history.",
        "Trend screen OHLCV prefetch: candidates 50, missing 14, fetched 12, failed 2.",
        "保留的选股说明。",
      ],
    }} grouped={false} watchlist={[]} onToggleWatchlist={() => undefined} />);
    const visibleMarkup = markup.split('<details class="raw-json">')[0];
    expect(visibleMarkup).toContain("数据覆盖提示");
    expect(visibleMarkup).toContain('class="notes-block"');
    expect(visibleMarkup).toContain("605358.SH");
    expect(visibleMarkup).toContain("14 只候选");
    expect(visibleMarkup).toContain("Skipped 2");
    expect(visibleMarkup).toContain("OHLCV prefetch");
    expect(visibleMarkup).toContain("保留的选股说明");
  });
  it("shows the calibrated parameter identity alongside its actual industry sample count", () => {
    const markup = renderToStaticMarkup(<ScreenResultView
      result={{ total: 1, returned: 1, algorithm_version: "adaptive_swing_v2", scoring_revision: "unit-rank-fix/v1", parameter_set_id: "parameter-a", metric_contract_version: 2,
        items: [{ stock: { code: "000001.SZ", name: "样本", price: 10 }, score: 12, reasons: [], industry_sample_count: 3 }] }}
      grouped={false} watchlist={[]} onToggleWatchlist={() => undefined} onObserveStock={() => undefined}
    />);
    expect(markup).toContain("参数版本");
    expect(markup).toContain("parameter-a");
    expect(markup).toContain("行业样本");
  });
  it("renders regime evidence, override, coverage, and both adaptive lists without rollout status", () => {
    const stock = (code: string, name: string) => ({
      stock: { code, name, industry: "测试行业", price: 10 },
      score: 15,
      reasons: ["测试理由"],
    });
    const markup = renderToStaticMarkup(
      <ScreenResultView
        result={{
          algorithm_version: "adaptive_swing_v1",
          total: 20,
          returned: 10,
          items: [stock("600000.SH", "主榜样本")],
          groups: [
            { key: "primary", title: "主榜", items: [stock("600000.SH", "主榜样本")] },
            { key: "exploration", title: "探索榜", items: [stock("000001.SZ", "探索样本")] },
          ],
          market_regime: {
            detected: "range",
            effective: "trend",
            confidence: 0.78,
            overridden: true,
            as_of_date: "20260729",
            evidence: [{ key: "breadth", label: "上涨家数比例", value: 0.52, summary: "市场宽度" }],
            coverage: {
              candidate_requested: 80,
              candidate_usable: 64,
              candidate_ratio: 0.8,
              benchmark_requested: 3,
              benchmark_usable: 3,
              breadth_usable: true,
              breadth_requested: 5_200,
              breadth_observed: 4_680,
              breadth_coverage_ratio: 0.9,
            },
          },
          rollout: {
            adaptive_available: true,
            adaptive_default_enabled: false,
            reason: "等待发布门槛",
          },
        }}
        grouped={false}
        watchlist={[]}
        onToggleWatchlist={() => undefined}
      />,
    );

    expect(markup).not.toContain("新版待发布门槛验证");
    expect(markup).not.toContain("aria-label=\"算法发布状态\"");
    expect(markup).toContain("系统识别");
    expect(markup).toContain("震荡");
    expect(markup).toContain("人工覆盖为 趋势");
    expect(markup).toContain("候选覆盖 80%");
    expect(markup).toContain("市场宽度 有效");
    expect(markup).toContain("4680/5200");
    expect(markup).toContain("90%");
    expect(markup).toContain("主榜");
    expect(markup).toContain("探索榜");
    expect(markup).toContain("按综合评分选出");
    expect(markup).toContain("不是买卖建议");
    expect(markup).toContain("入选：测试理由");
    expect(markup).toContain("当前识别为趋势");
  });
});
