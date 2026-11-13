import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceProvider } from "../../hooks/useWorkspace";
import { workspaceTestHarness } from "../../test/workspace";
import type { FilterCriteria } from "../FilterBar";
import type { BacktestResult } from "../../types";
import type { BacktestRouteRequest } from "../../lib/viewNavigation";

const { getJsonMock, postJsonMock } = vi.hoisted(() => ({ getJsonMock: vi.fn(), postJsonMock: vi.fn() }));
vi.mock("../../lib/tauri", () => ({ getJson: getJsonMock, postJson: postJsonMock }));

let BacktestResultView: typeof import("./BacktestPanel").BacktestResultView;
let BacktestPanel: typeof import("./BacktestPanel").BacktestPanel;

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ BacktestResultView, BacktestPanel } = await import("./BacktestPanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

const result = {
  metrics: {
    total_return: 0.1,
    num_stocks: 1,
  },
  equity_curve: [],
  symbols: ["300750.SZ"],
  volatility_snapshots: [{
    symbol: "300750.SZ",
    name: "宁德时代",
    date: "2026-07-15",
    close: 268.5,
    atr: { period: 14, value: 8.2, percent_of_close: 3.054 },
    bollinger_bands: {
      period: 20,
      multiplier: 2,
      upper: 280,
      middle: 250,
      lower: 220,
      bandwidth_percent: 24,
      percent_b: 80.8333,
    },
    donchian_channel: {
      period: 20,
      upper: 285,
      middle: 250,
      lower: 215,
      width_percent: 28,
      position_percent: 76.4286,
    },
    keltner_channel: {
      ema_period: 20,
      atr_period: 10,
      multiplier: 2,
      upper: 276,
      middle: 250,
      lower: 224,
      width_percent: 20.8,
      position_percent: 85.5769,
    },
    chaikin_volatility: { ema_period: 10, roc_period: 10, value: 12.5 },
    rvi: { period: 14, value: 61.2 },
  }],
};

const unavailableResult = {
  ...result,
  volatility_snapshots: [{
    symbol: "300750.SZ",
    date: "2026-07-15",
    close: null,
    unavailable: [
      { indicator: "atr", reason: "区间末收盘价无效" },
      { indicator: "bollinger_bands", reason: "区间末收盘价无效" },
      { indicator: "donchian_channel", reason: "区间末收盘价无效" },
      { indicator: "keltner_channel", reason: "区间末收盘价无效" },
      { indicator: "chaikin_volatility", reason: "区间末收盘价无效" },
      { indicator: "rvi", reason: "区间末收盘价无效" },
    ],
  }],
};

describe("BacktestResultView volatility diagnostics", () => {
  it("clearly labels synthetic PIT returns with their frozen dataset identity", () => {
    const html=renderToStaticMarkup(<BacktestResultView result={{...result,parameter_set_id:"fixed-v1",scoring_revision:"unit-rank-fix/v1",
      pit_evaluation_report:{run_id:"sample-run",dataset_id:"sample-data",data_quality:{passed:true,real_data:false},parameters:{parameter_set_id:"fixed-v1"},metrics:{total_return:0.1}},
    }} />);
    expect(html).toContain("合成样例");
    expect(html).toContain("不提供真实收益证据");
    expect(html).toContain("sample-data");
  });
  it("identifies the portfolio curve, renders its benchmark, and scopes the symbol selector", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{
          ...result,
          equity_curve: [
            { date: "2026-07-01", equity: 1 },
            { date: "2026-07-02", equity: 1.08 },
          ],
          benchmark_curve: [
            { date: "2026-07-01", equity: 1 },
            { date: "2026-07-02", equity: 1.03 },
          ],
        }}
      />,
    );

    expect(html).toContain("组合净值曲线");
    expect(html).toContain("组合净值");
    expect(html).toContain("候选池基准");
    expect(html).toContain("波动率标的");
    expect(html).toContain('aria-label="组合与基准净值曲线"');
  });

  it("renders all six volatility indicators for the selected symbol", () => {
    const html = renderToStaticMarkup(<BacktestResultView result={result} />);

    expect(html).toContain("波动率快照");
    expect(html).toContain("ATR14");
    expect(html).toContain("布林带");
    expect(html).toContain("唐奇安通道");
    expect(html).toContain("凯尔特纳通道");
    expect(html).toContain("Chaikin 波动率");
    expect(html).toContain("RVI14");
    expect(html).toContain("宁德时代（300750.SZ）");
    expect(html).toContain("300750.SZ");
    expect(html).toContain("12.50%");
    expect(html).toContain("一句话看懂");
    expect(html).toContain("为什么这么说");
    expect(html).toContain("可以怎么调整");
    expect(html).toContain("这些判断怎么算的");
    expect(html).toContain("48–52（含端点）为方向均衡");
    expect(html).toContain("最近上涨更占优势");
    expect(html).toContain('<ul class="symbol-strip"');
    expect(html).toContain('<li class="symbol-chip"');
    expect(html).not.toContain("日内波幅扩张");
  });

  it("shows precise unavailable reasons instead of hiding the module", () => {
    const html = renderToStaticMarkup(<BacktestResultView result={unavailableResult} />);

    expect(html).toContain("波动率快照");
    expect(html).toContain("区间末收盘价无效");
    expect(html).toContain("历史行情还不完整");
    expect(html).not.toContain("历史数据不足");
  });

  it("renders an explicit empty state when no symbol snapshot is available", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{
          ...result,
          volatility_snapshots: [],
          volatility_message: "候选快照没有符合条件的标的。",
        }}
      />,
    );

    expect(html).toContain("无可用区间末标的");
    expect(html).toContain("候选快照没有符合条件的标的");
  });

  it("does not render an empty chart frame for a single equity point", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{ ...result, equity_curve: [{ date: "2026-07-17", equity: 1.12 }] }}
      />,
    );

    expect(html).not.toContain("backtest-primary-chart");
    expect(html).toContain("有效交易日不足，暂不绘制净值曲线");
  });

  it("shows the side-by-side legacy comparison and incomplete release gates", () => {
    const html = renderToStaticMarkup(
      <BacktestResultView
        result={{
          ...result,
          metrics: { ...result.metrics, strategy_mode: "adaptive_swing_v1", annualized_return: 0.12 },
          legacy_balanced_backtest: {
            ...result,
            metrics: { ...result.metrics, strategy_mode: "walk_forward", annualized_return: 0.10 },
          },
          adaptive_release_gate: {
            passed: false,
            checks: [
              {
                key: "annualized_return_delta",
                passed: true,
                actual: 0.02,
                requirement: "adaptive annualized return no worse than legacy by more than 1 percentage point",
              },
              {
                key: "cached_run_millis",
                passed: false,
                actual: null,
                requirement: "same-day cached run <= 2000 ms",
              },
            ],
          },
        }}
      />,
    );

    expect(html).toContain("自适应波段");
    expect(html).toContain("同样本旧 balanced 年化");
    expect(html).toContain("暂不切换默认");
    expect(html).toContain("缓存运行耗时");
    expect(html).toContain("待采集");
    expect(html).toContain('class="backtest-gate-status passed"');
    expect(html).toContain('class="backtest-gate-status failed"');
  });
});

describe("BacktestPanel completed result presentation", () => {
  const criteria: FilterCriteria = {
    includeSt: false, requireInstitutionBuyRatio: false, minRoe: "", maxPe: "", maxPb: "", minMcap: "",
    industry: "电池", marketScope: "创业板", resultLimit: 10, sortBy: "score", sortDir: "desc", scoreProfile: "quality",
  };
  const watchlist = [{ code: "300750.SZ", name: "宁德时代" }];
  let workspace: ReturnType<typeof workspaceTestHarness>;
  let renderer: ReactTestRenderer | undefined;
  let mobile: boolean;
  let reducedMotion: boolean;
  let mediaListeners: Map<string, () => void>;
  let resultHeading: { focus: ReturnType<typeof vi.fn>; scrollIntoView: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    workspace = workspaceTestHarness(undefined, { "backtest.start": "2025-01-01", "backtest.end": "2026-07-15" });
    mobile = true;
    reducedMotion = false;
    mediaListeners = new Map();
    resultHeading = { focus: vi.fn(), scrollIntoView: vi.fn() };
    vi.stubGlobal("document", { visibilityState: "visible" });
    vi.stubGlobal("window", {
      location: { href: "http://localhost/" },
      matchMedia: (query: string) => ({
        matches: query === "(max-width: 768px)" ? mobile : reducedMotion,
        addEventListener: (_event: string, listener: () => void) => mediaListeners.set(query, listener),
        removeEventListener: (_event: string, listener: () => void) => {
          if (mediaListeners.get(query) === listener) mediaListeners.delete(query);
        },
      }),
    });
    getJsonMock.mockResolvedValue({});
    postJsonMock.mockReset();
  });

  afterEach(async () => {
    if (renderer) await act(async () => renderer!.unmount());
    renderer = undefined;
    workspace.store.dispose();
    getJsonMock.mockReset();
    postJsonMock.mockReset();
    vi.unstubAllGlobals();
  });

  const panel = (preferredSource?: BacktestRouteRequest, market: "cn" | "hk" = "cn") => (
    <WorkspaceProvider store={workspace.store}>
      <BacktestPanel criteria={criteria} watchlist={watchlist} preferredSource={preferredSource} market={market} />
    </WorkspaceProvider>
  );
  async function startPanel(preferredSource?: BacktestRouteRequest, market: "cn" | "hk" = "cn") {
    await act(async () => {
      renderer = create(panel(preferredSource, market), { createNodeMock: (element) => {
        const props = element.props as { className?: string };
        return props.className === "backtest-result-heading" ? resultHeading : null;
      } });
    });
  }
  const groups = () => renderer!.root.findByProps({ className: "backtest-parameter-groups" });
  const runButton = () => renderer!.root.findByProps({ className: "backtest-run-button" });
  function text(node = renderer!.root): string {
    return node.children.map(child => typeof child === "string" ? child : text(child)).join(" ");
  }
  async function runBacktest() {
    await act(async () => { runButton().props.onClick(); await Promise.resolve(); });
  }
  async function edit(id: string, value: string) {
    await act(async () => renderer!.root.findByProps({ id }).props.onChange({ target: { value } }));
  }

  it("collapses mobile parameters only after success, reopens them locally, and focuses the result", async () => {
    postJsonMock.mockResolvedValue(result);
    await startPanel();
    expect(groups().props.hidden).not.toBe(true);
    expect(renderer!.root.findAllByProps({ className: "backtest-parameter-toggle" })).toHaveLength(0);
    await runBacktest();

    expect(groups().props.hidden).toBe(true);
    const toggle = renderer!.root.findByProps({ className: "backtest-parameter-toggle" });
    expect(text(toggle)).toBe("调整参数");
    expect(toggle.props["aria-expanded"]).toBe(false);
    expect(toggle.props["aria-controls"]).toBe(groups().props.id);
    expect(renderer!.root.findByProps({ className: "backtest-result-heading" }).props.tabIndex).toBe(-1);
    expect(resultHeading.focus).toHaveBeenCalledWith({ preventScroll: true });
    expect(resultHeading.scrollIntoView).toHaveBeenCalledWith({ block: "start", behavior: "smooth" });
    expect(text(renderer!.root.findByProps({ className: "backtest-completed-summary" }))).toContain("仅供研究，不构成投资建议。");
    await workspace.store.flush();
    const savedBeforeToggle = workspace.snapshot();
    await act(async () => toggle.props.onClick());
    expect(groups().props.hidden).toBe(false);
    expect(text(renderer!.root.findByProps({ className: "backtest-parameter-toggle" }))).toBe("收起参数");
    await workspace.store.flush();
    expect(workspace.snapshot()).toEqual(savedBeforeToggle);
    expect(postJsonMock).toHaveBeenCalledTimes(1);
    expect(resultHeading.focus).toHaveBeenCalledTimes(1);
  });

  it("retains desktop parameter groups and honors reduced motion when focusing a completed result", async () => {
    mobile = false;
    reducedMotion = true;
    postJsonMock.mockResolvedValue(result);
    await startPanel();
    await runBacktest();
    expect(groups().props.hidden).toBe(false);
    expect(renderer!.root.findAllByProps({ className: "backtest-parameter-toggle" })).toHaveLength(0);
    expect(resultHeading.scrollIntoView).toHaveBeenCalledWith({ block: "start", behavior: "auto" });
    await act(async () => { mobile = true; mediaListeners.get("(max-width: 768px)")?.(); });
    expect(groups().props.hidden).toBe(true);
    await act(async () => { mobile = false; mediaListeners.get("(max-width: 768px)")?.(); });
    expect(groups().props.hidden).toBe(false);
  });

  it("keeps the completed scope, dates, mode and cost through edits, recomputation and failure", async () => {
    postJsonMock.mockResolvedValueOnce(result);
    await startPanel();
    await runBacktest();
    await act(async () => renderer!.root.findByProps({ className: "backtest-parameter-toggle" }).props.onClick());
    const summary = () => text(renderer!.root.findByProps({ className: "backtest-completed-conditions" }));
    const completedSummary = summary();
    expect(completedSummary).toContain("当前条件 · 电池 / 创业板");
    expect(completedSummary).toContain("2025-01-01 — 2026-07-15");
    expect(completedSummary).toContain("候选快照");
    expect(completedSummary).toContain("10 基点");
    await edit("btIndustry", "传媒");
    await edit("btMarketScope", "北交所");
    await edit("btStart", "2026-01-01");
    await edit("btEnd", "2026-07-01");
    await edit("btStrategyMode", "walk_forward");
    await edit("btCostBps", "25");
    expect(summary()).toBe(completedSummary);
    expect(text()).toContain("参数已修改，结果待更新");
    expect(text(runButton())).toBe("重新回测");

    let rejectRequest!: (reason: Error) => void;
    postJsonMock.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectRequest = reject; }));
    await runBacktest();
    expect(renderer!.root.findAllByType(BacktestResultView)).toHaveLength(1);
    expect(summary()).toBe(completedSummary);
    expect(renderer!.root.findAllByProps({ className: "panel-feedback panel-feedback-loading panel-feedback-compact" })).toHaveLength(1);
    expect(renderer!.root.findAllByProps({ className: "skeleton-layout" })).toHaveLength(0);
    await act(async () => { rejectRequest(new Error("历史快照缺失")); await Promise.resolve(); });
    expect(renderer!.root.findAllByType(BacktestResultView)).toHaveLength(1);
    expect(summary()).toBe(completedSummary);
    expect(renderer!.root.findAllByProps({ className: "panel-feedback panel-feedback-error panel-feedback-compact" })).toHaveLength(1);
    expect(text()).toContain("历史快照缺失");
    expect(runButton().props.disabled).toBe(false);
    expect(resultHeading.focus).toHaveBeenCalledTimes(1);

    postJsonMock.mockResolvedValueOnce({ ...result, metrics: { ...result.metrics, total_return: 0.2 } });
    await runBacktest();
    expect(summary()).toContain("传媒 / 北交所");
    expect(summary()).toContain("2026-01-01 — 2026-07-01");
    expect(summary()).toContain("滚动验证");
    expect(summary()).toContain("25 基点");
    expect(text()).not.toContain("参数已修改，结果待更新");
    expect(groups().props.hidden).toBe(true);
    expect(resultHeading.focus).toHaveBeenCalledTimes(2);
  });

  it("blocks duplicate recomputations and discards a stale completion after a route reset", async () => {
    postJsonMock.mockResolvedValueOnce(result);
    await startPanel();
    await runBacktest();
    let resolveRequest!: (value: BacktestResult) => void;
    postJsonMock.mockReturnValueOnce(new Promise<BacktestResult>(resolve => { resolveRequest = resolve; }));
    const click = runButton().props.onClick;
    await act(async () => { click(); click(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledTimes(2);
    expect(runButton().props.disabled).toBe(true);
    expect(renderer!.root.findAllByType(BacktestResultView)).toHaveLength(1);
    await act(async () => renderer!.update(panel({ source: "watchlist", requestId: 9 })));
    await act(async () => { resolveRequest({ ...result, symbols: ["000001.SZ"] }); await Promise.resolve(); });
    expect(renderer!.root.findAllByType(BacktestResultView)).toHaveLength(0);
    expect(renderer!.root.findAllByProps({ className: "backtest-completed-conditions" })).toHaveLength(0);
    expect(groups().props.hidden).toBe(false);
    expect(text()).not.toContain("000001.SZ");
    expect(runButton().props.disabled).toBe(false);
    expect(resultHeading.focus).toHaveBeenCalledTimes(1);
  });

  it("keeps initial parameters open on failure and rejects a stale failure after a route reset", async () => {
    postJsonMock.mockRejectedValueOnce(new Error("首轮失败"));
    await startPanel();
    await runBacktest();
    expect(groups().props.hidden).toBe(false);
    expect(renderer!.root.findAllByProps({ className: "panel-feedback panel-feedback-error" })).toHaveLength(1);
    expect(resultHeading.focus).not.toHaveBeenCalled();
    let rejectRequest!: (reason: Error) => void;
    postJsonMock.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectRequest = reject; }));
    await runBacktest();
    await act(async () => renderer!.update(panel({ source: "watchlist", requestId: 10 })));
    await act(async () => { rejectRequest(new Error("过期错误")); await Promise.resolve(); });
    expect(text()).not.toContain("过期错误");
    expect(text()).toContain("选择股票来源并设置参数后运行回测");
    expect(runButton().props.disabled).toBe(false);
  });

  it("labels the completed calibrated mode and cost using the request contract without altering saved inputs", async () => {
    postJsonMock.mockResolvedValue(result);
    await startPanel();
    await act(async () => renderer!.update(panel({
      source: "criteria", requestId: 11,
      adaptiveScreenSpec: {
        criteria: { include_st: false, industry: "电池", market_scope: "创业板", limit: 80 },
        algorithm_version: "adaptive_swing_v2", parameter_set_id: "calibrated-v2",
        mode: "auto", horizon: "swing_10_30d", primary_limit: 10, exploration_limit: 10, run_id: "calibrated-run",
      },
    })));
    await edit("btCostBps", "900");
    await runBacktest();
    const summary = text(renderer!.root.findByProps({ className: "backtest-completed-conditions" }));
    expect(summary).toContain("校准自适应波段");
    expect(summary).toContain("500 基点");
    expect(postJsonMock.mock.calls[0][1]).toMatchObject({
      strategy_mode: "adaptive_swing_v2:auto", transaction_cost_bps: 500, parameter_set_id: "calibrated-v2",
    });
    expect(renderer!.root.findByProps({ id: "btCostBps" }).props.value).toBe(900);
  });

  it("describes the submitted HK scope when a fresh screen route carries a supported A-share scope", async () => {
    const routedCriteria = { ...criteria, industry: "", marketScope: "北交所" };
    postJsonMock.mockResolvedValue({ ...result, symbols: ["00700.HK"], volatility_snapshots: [] });
    await startPanel({ source: "criteria", requestId: 12, criteriaSnapshot: routedCriteria }, "hk");
    expect(renderer!.root.findByProps({ id: "btMarketScope" }).props.value).toBe("");
    expect(text(renderer!.root.findByProps({ className: "backtest-context-head" }))).toContain("全部行业 / 全部港币股票");
    expect(text()).not.toContain("北交所");
    await runBacktest();
    expect(postJsonMock.mock.calls[0][1]).toMatchObject({ market: "hk", source: "criteria" });
    expect(postJsonMock.mock.calls[0][1].criteria).not.toHaveProperty("market_scope");
    const summary = () => text(renderer!.root.findByProps({ className: "backtest-completed-conditions" }));
    expect(summary()).toContain("当前条件 · 全部行业 / 全部港币股票");
    expect(summary()).not.toContain("北交所");
    expect(workspace.store.get("hk.backtest.criteria", criteria)).toEqual(routedCriteria);
    await act(async () => renderer!.root.findByProps({ className: "backtest-parameter-toggle" }).props.onClick());
    await edit("btIndustry", "半导体");
    expect(text(renderer!.root.findByProps({ className: "backtest-context-head" }))).toContain("半导体 / 全部港币股票");
    expect(summary()).toContain("当前条件 · 全部行业 / 全部港币股票");
    expect(text()).toContain("参数已修改，结果待更新");
  });
});
