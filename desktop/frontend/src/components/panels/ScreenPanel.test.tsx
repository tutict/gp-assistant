import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { FilterCriteria } from "../FilterBar";
import { CriteriaFields } from "../CriteriaFields";

const { postJsonMock } = vi.hoisted(() => ({ postJsonMock: vi.fn() }));
vi.mock("../../lib/tauri", () => ({ postJson: postJsonMock, getTauriListen: () => undefined }));

let ScreenResultView: typeof import("./ScreenPanel").ScreenResultView;
let ScreenPanel: typeof import("./ScreenPanel").ScreenPanel;

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ ScreenResultView, ScreenPanel } = await import("./ScreenPanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

describe("ScreenResultView", () => {
  it("summarizes returned count, coverage total and highest score in one accessible row", () => {
    const markup = renderToStaticMarkup(<ScreenResultView result={{ total: 120, returned: 2,
      items: [
        { stock: { code: "600000.SH", name: "样本甲", price: 10 }, score: 15.31, reasons: [] },
        { stock: { code: "600001.SH", name: "样本乙", price: 11 }, score: 18.2, reasons: [] },
      ],
    }} grouped={false} watchlist={[]} onToggleWatchlist={() => undefined} />);

    expect(markup).toContain('<dl class="screen-result-summary" aria-label="筛选结果统计">');
    expect(markup).toContain("<dt>返回数</dt><dd>2</dd>");
    expect(markup).toContain("<dt>总数</dt><dd>120</dd>");
    expect(markup).toContain("<dt>最高分</dt><dd>18.20</dd>");
    expect(markup).not.toContain("screen-result-metric-strip");
    expect(markup).not.toContain("按综合评分选出");
    expect(markup.indexOf('class="workspace-boundary screen-result-boundary"')).toBeLessThan(markup.indexOf('class="quote-table"'));
  });
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
    expect(markup).toContain("综合评分反映条件匹配程度；入选原因与资料来源可在个股依据中查看。");
    expect(markup).toContain('<p class="workspace-boundary screen-result-boundary">仅供研究，不构成投资建议。</p>');
    expect(markup).toContain("入选：测试理由");
    expect(markup).toContain("当前识别为趋势");
  });
});

const panelCriteria: FilterCriteria = {
  includeSt: false, requireInstitutionBuyRatio: false,
  minRoe: "", maxPe: "25", maxPb: "", minMcap: "", industry: "", marketScope: "",
  resultLimit: 10, sortBy: "score", sortDir: "desc", scoreProfile: "balanced",
};
const cachedResult = {
  total: 100, returned: 1,
  items: [{ stock: { code: "600000.SH", name: "保留的候选", price: 10 }, score: 15, reasons: ["可核查依据"] }],
};

describe("ScreenPanel result workflow", () => {
  let renderer: ReactTestRenderer | undefined;

  beforeEach(() => {
    postJsonMock.mockReset();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  });
  afterEach(async () => {
    if (renderer) await act(async () => renderer!.unmount());
    renderer = undefined;
    vi.unstubAllGlobals();
  });

  const button = (label: string) => renderer!.root.find(node => node.type === "button" && node.children.includes(label));
  const panel = (criteria = panelCriteria, change = vi.fn()) => <ScreenPanel criteria={criteria} onCriteriaChange={change} watchlist={[]} onWatchlistChange={() => undefined} onRunBacktest={backtest} />;
  const backtest = vi.fn();
  const startPanel = async () => {
    await act(async () => { renderer = create(panel()); });
  };
  const runPanel = async () => {
    await act(async () => { button("运行筛选").props.onClick(); await Promise.resolve(); });
  };

  it("keeps cached results and their criteria snapshot during a refresh and retry", async () => {
    backtest.mockReset();
    postJsonMock.mockResolvedValueOnce(cachedResult);
    await startPanel();
    await act(async () => button("自定义选股").props.onClick());
    await runPanel();

    const changed = { ...panelCriteria, maxPe: "35", sortBy: "roe", sortDir: "asc", scoreProfile: "quality" };
    await act(async () => renderer!.update(panel(changed)));
    let rejectRefresh!: (reason: Error) => void;
    postJsonMock.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectRefresh = reject; }));
    await runPanel();

    expect(renderer!.root.findAll(node => node.type === "strong" && node.children.includes("保留的候选"))).toHaveLength(1);
    expect(renderer!.root.findAllByProps({ className: "panel-feedback panel-feedback-loading panel-feedback-compact" })).toHaveLength(1);
    expect(renderer!.root.findAllByProps({ className: "skeleton-layout" })).toHaveLength(0);
    expect(renderer!.root.findByProps({ className: "screen-result-conditions" }).children.join(" ")).toContain("PE≤25");
    expect(renderer!.root.findByProps({ className: "screen-result-conditions" }).children.join(" ")).toContain("条件已修改");
    await act(async () => button("回测").props.onClick());
    expect(backtest).toHaveBeenLastCalledWith(undefined, panelCriteria);

    await act(async () => { rejectRefresh(new Error("测试查询失败")); await Promise.resolve(); });
    expect(renderer!.root.findAll(node => node.type === "strong" && node.children.includes("保留的候选"))).toHaveLength(1);
    expect(renderer!.root.findAllByProps({ className: "panel-feedback panel-feedback-error panel-feedback-compact" })).toHaveLength(1);
    expect(button("重试")).toBeDefined();
    postJsonMock.mockResolvedValueOnce(cachedResult);
    await act(async () => { button("重试").props.onClick(); await Promise.resolve(); });
    await act(async () => button("回测").props.onClick());
    expect(backtest).toHaveBeenLastCalledWith(undefined, changed);
    expect(renderer!.root.findAllByProps({ className: "screen-result-conditions" })).toHaveLength(0);
  });

  it("offers mobile adjustment after success with collapsed conditions, and applies only confirmed drafts", async () => {
    Object.assign(window, { matchMedia: vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })) });
    vi.stubGlobal("document", { activeElement: null, addEventListener: vi.fn(), removeEventListener: vi.fn() });
    const change = vi.fn();
    postJsonMock.mockResolvedValue(cachedResult);
    await act(async () => { renderer = create(panel(panelCriteria, change)); });
    await act(async () => button("自定义选股").props.onClick());
    await runPanel();

    const conditions = renderer!.root.findByProps({ className: "screen-current-conditions" });
    expect(conditions.type).toBe("details");
    expect(conditions.props.open).not.toBe(true);
    expect(renderer!.root.findAllByType(CriteriaFields)).toHaveLength(0);
    expect(button("运行筛选")).toBeDefined();
    const container = renderer!.root.findByProps({ className: "panel-container screen-panel-container has-result" });
    const children = container.children.filter(child => typeof child !== "string");
    expect(children.findIndex(child => child.props.className === "panel-result screen-panel-result"))
      .toBeLessThan(children.findIndex(child => child.props.className === "screen-mode-note"));
    await act(async () => button("调整条件").props.onClick());
    await act(async () => renderer!.root.findByType(CriteriaFields).props.onChange({ ...panelCriteria, maxPe: "45" }));
    expect(change).not.toHaveBeenCalled();
    await act(async () => button("取消").props.onClick());
    await act(async () => button("调整条件").props.onClick());
    expect(renderer!.root.findByType(CriteriaFields).props.criteria.maxPe).toBe("25");
    await act(async () => renderer!.root.findByType(CriteriaFields).props.onChange({ ...panelCriteria, maxPe: "45" }));
    await act(async () => { button("应用并筛选").props.onClick(); await Promise.resolve(); });
    expect(change).toHaveBeenCalledWith({ ...panelCriteria, maxPe: "45" });
    expect(postJsonMock).toHaveBeenLastCalledWith("/api/custom-screen", expect.objectContaining({ criteria: expect.objectContaining({ max_pe: 45 }) }));
    expect(renderer!.root.findAllByProps({ role: "dialog" })).toHaveLength(0);
  });

  it("scrolls the selected mobile mode into view on initial render and on selection", async () => {
    Object.assign(window, { matchMedia: vi.fn(() => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() })) });
    const scrolls = new Map<string, ReturnType<typeof vi.fn>>();
    await act(async () => {
      renderer = create(panel(), { createNodeMock: element => {
        const props = element.props as { role?: string; children?: string };
        if (props.role !== "tab") return null;
        const scrollIntoView = vi.fn();
        scrolls.set(String(props.children), scrollIntoView);
        return { scrollIntoView };
      } });
    });
    expect(scrolls.get("智能选股")).toHaveBeenCalledWith({ block: "nearest", inline: "nearest" });
    await act(async () => button("趋势选股").props.onClick());
    expect(scrolls.get("趋势选股")).toHaveBeenCalledWith({ block: "nearest", inline: "nearest" });
    expect(button("趋势选股").props["aria-selected"]).toBe(true);
  });

  it("retains the successful trend interval while new dates are being screened", async () => {
    postJsonMock.mockResolvedValueOnce(cachedResult);
    await startPanel();
    await act(async () => button("趋势选股").props.onClick());
    await act(async () => {
      renderer!.root.findByProps({ id: "trendStart" }).props.onChange({ target: { value: "2026-09-01" } });
      renderer!.root.findByProps({ id: "trendEnd" }).props.onChange({ target: { value: "2026-10-01" } });
    });
    await runPanel();
    await act(async () => renderer!.root.findByProps({ id: "trendEnd" }).props.onChange({ target: { value: "2026-11-01" } }));
    let resolveRefresh!: (value: unknown) => void;
    postJsonMock.mockReturnValueOnce(new Promise(resolve => { resolveRefresh = resolve; }));
    await runPanel();
    expect(renderer!.root.findAll(node => node.type === "strong" && node.children.includes("保留的候选"))).toHaveLength(1);
    expect(renderer!.root.findByProps({ className: "screen-result-conditions" }).children.join(" ")).toContain("2026-09-01 至 2026-10-01");
    await act(async () => { resolveRefresh(cachedResult); await Promise.resolve(); });
    expect(renderer!.root.findAllByProps({ className: "screen-result-conditions" })).toHaveLength(0);
  });
});
