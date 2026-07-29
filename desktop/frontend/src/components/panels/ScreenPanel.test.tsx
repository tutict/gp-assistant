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
  it("renders rollout status, regime evidence, override, coverage, and both adaptive lists", () => {
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

    expect(markup).toContain("新版待发布门槛验证");
    expect(markup).toContain("系统识别");
    expect(markup).toContain("震荡");
    expect(markup).toContain("人工覆盖为 趋势");
    expect(markup).toContain("候选覆盖 80%");
    expect(markup).toContain("市场宽度 有效");
    expect(markup).toContain("主榜");
    expect(markup).toContain("探索榜");
  });
});
