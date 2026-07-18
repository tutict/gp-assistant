import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { BacktestResult, EquityPoint, VolatilitySnapshot, WatchlistItem } from "../../types";
import type { FilterCriteria } from "../FilterBar";
import { postJson } from "../../lib/tauri";
import { buildBacktestRequest, requireBacktestResult } from "../../lib/contracts";
import {
  currentSystemDateInputValue,
  formatMoney,
  formatNumber,
  formatPercent,
  formatSignedPercent,
  shortBenchmarkLabel,
  shortRebalanceLabel,
} from "../../lib/format";
import { RawJson } from "../RawJson";
import { PanelFeedback } from "../ui/PanelFeedback";
import type { BacktestRouteRequest, BacktestSource } from "../../lib/viewNavigation";

interface BacktestPanelProps {
  criteria: FilterCriteria;
  watchlist: WatchlistItem[];
  preferredSource?: BacktestRouteRequest | null;
  onPreferredSourceConsumed?: (requestId: number) => void;
}

export function BacktestPanel({ criteria, watchlist, preferredSource, onPreferredSourceConsumed }: BacktestPanelProps) {
  const [source, setSource] = useState<BacktestSource>("criteria");
  const [start, setStart] = useState("2020-01-01");
  const [end, setEnd] = useState(currentSystemDateInputValue());
  const [topN, setTopN] = useState(10);
  const [rebalance, setRebalance] = useState("monthly");
  const [benchmark, setBenchmark] = useState("candidate_equal_weight");
  const [strategyMode, setStrategyMode] = useState("candidate_snapshot");
  const [costBps, setCostBps] = useState(10);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestInFlightRef = useRef(false);
  const requestVersionRef = useRef(0);
  const watchlistSignature = watchlist.map((item) => item.code.toUpperCase()).join("|");
  const previousWatchlistSignatureRef = useRef(watchlistSignature);

  useEffect(() => {
    if (!preferredSource) return;
    requestVersionRef.current += 1;
    setSource(preferredSource.source);
    setResult(null);
    setError(null);
    onPreferredSourceConsumed?.(preferredSource.requestId);
  }, [onPreferredSourceConsumed, preferredSource]);

  useEffect(() => {
    if (previousWatchlistSignatureRef.current === watchlistSignature) return;
    previousWatchlistSignatureRef.current = watchlistSignature;
    if (source !== "watchlist") return;
    requestVersionRef.current += 1;
    setResult(null);
    setError(null);
  }, [source, watchlistSignature]);

  const selectSource = (nextSource: BacktestSource) => {
    if (nextSource === source) return;
    requestVersionRef.current += 1;
    setSource(nextSource);
    setResult(null);
    setError(null);
  };

  const run = async () => {
    if (requestInFlightRef.current) return;
    if (source === "watchlist" && watchlist.length === 0) {
      setResult(null);
      setError("自选股为空，请先从筛选结果中收藏股票。");
      return;
    }

    requestInFlightRef.current = true;
    const requestVersion = ++requestVersionRef.current;
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      const payload = buildBacktestRequest({
        source,
        criteria,
        watchlist,
        startDate: start,
        endDate: end,
        topN,
        rebalanceFrequency: rebalance,
        transactionCostBps: costBps,
        benchmark,
        strategyMode,
      });
      const data = await postJson<unknown>("/api/backtest", payload, { timeoutMs: 90_000 });
      const nextResult = requireBacktestResult(data);
      if (requestVersion === requestVersionRef.current) setResult(nextResult);
    } catch (err) {
      if (requestVersion === requestVersionRef.current) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      requestInFlightRef.current = false;
      setLoading(false);
    }
  };

  const sourceText = source === "watchlist"
    ? `${Math.min(topN, watchlist.length)} / ${watchlist.length} 只`
    : (criteria.industry || "全部行业");

  return (
    <div className="panel-container">
      <div className="backtest-context">
        <div className="backtest-context-head">
          <span>{source === "watchlist" ? "自选股" : "当前条件"}</span>
          <strong>{sourceText}</strong>
        </div>
        <div className="backtest-context-controls">
          <button
            type="button"
            className="source-toggle"
            aria-label="运行回测"
            aria-disabled={loading}
            disabled={loading}
            onClick={run}
          >
            {loading ? "回测计算中..." : "运行回测"}
          </button>
          <button type="button" className={`source-toggle ${source === "criteria" ? "active" : ""}`} disabled={loading} onClick={() => selectSource("criteria")}>当前条件</button>
          <button type="button" className={`source-toggle ${source === "watchlist" ? "active" : ""}`} disabled={loading} onClick={() => selectSource("watchlist")}>自选股</button>
        </div>
        <div className="backtest-param-strip">
          <span><b>持仓</b><strong>{topN}</strong></span>
          <span><b>区间</b><strong>{start}~{end}</strong></span>
          <span><b>调仓</b><strong>{shortRebalanceLabel(rebalance)}</strong></span>
          <span><b>成本</b><strong>{formatNumber(costBps)}bps</strong></span>
          <span><b>基准</b><strong>{shortBenchmarkLabel(benchmark)}</strong></span>
          <span><b>Mode</b><strong>{strategyMode === "walk_forward" ? "Walk-forward" : "Snapshot"}</strong></span>
        </div>
      </div>

      <div className="panel-controls backtest-controls">
        <div className="form-row inline"><label htmlFor="btStart">开始</label><input id="btStart" type="date" value={start} disabled={loading} onChange={(e) => setStart(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btEnd">结束</label><input id="btEnd" type="date" value={end} disabled={loading} onChange={(e) => setEnd(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btTopN">持仓</label><input id="btTopN" type="number" min="1" max="100" value={topN} disabled={loading} onChange={(e) => setTopN(Number(e.target.value) || 10)} /></div>
        <div className="form-row inline">
          <label htmlFor="btRebalance">调仓</label>
          <select id="btRebalance" value={rebalance} disabled={loading} onChange={(e) => setRebalance(e.target.value)}>
            <option value="none">买入持有</option>
            <option value="monthly">月度调仓</option>
            <option value="quarterly">季度调仓</option>
          </select>
        </div>
        <div className="form-row inline">
          <label htmlFor="btBenchmark">基准</label>
          <select id="btBenchmark" value={benchmark} disabled={loading} onChange={(e) => setBenchmark(e.target.value)}>
            <option value="candidate_equal_weight">候选池等权</option>
            <option value="none">无</option>
          </select>
        </div>
        <div className="form-row inline"><label htmlFor="btStrategyMode">模式</label><select id="btStrategyMode" value={strategyMode} disabled={loading} onChange={(e) => setStrategyMode(e.target.value)}><option value="candidate_snapshot">候选快照</option><option value="walk_forward">Walk-forward</option></select></div>
        <div className="form-row inline"><label htmlFor="btCostBps">成本</label><input id="btCostBps" type="number" min="0" max="500" value={costBps} disabled={loading} onChange={(e) => setCostBps(Number(e.target.value) || 0)} /></div>
      </div>

      <div className="panel-result">
        {error && <PanelFeedback
          kind="error"
          title="回测失败"
          description={strategyMode === "walk_forward"
            ? `${error} 请补齐历史因子快照，或切换到“候选快照”查看非严格历史组合表现。`
            : error}
        />}
        {loading && !result && !error && <PanelFeedback kind="loading" description="正在计算组合表现..." />}
        {result && !loading && <BacktestResultView result={result} />}
        {!result && !loading && !error && <PanelFeedback kind="empty" description="选择股票来源并设置参数后运行回测。" />}
      </div>
    </div>
  );
}

export function BacktestResultView({ result }: { result: BacktestResult }) {
  const metrics = result.metrics || {};
  const equityPointCount = result.equity_curve?.length ?? 0;
  return (
    <div className="backtest-result">
      <div className="metric-strip">
        <div className="metric"><span>总收益</span><strong className={metrics.total_return > 0 ? "positive" : "negative"}>{formatSignedPercent((metrics.total_return ?? 0) * 100)}</strong></div>
        <div className="metric"><span>年化收益</span><strong>{metrics.annualized_return != null ? formatSignedPercent(metrics.annualized_return * 100) : "--"}</strong></div>
        <div className="metric"><span>最大回撤</span><strong className="negative">{metrics.max_drawdown != null ? formatSignedPercent(metrics.max_drawdown * 100) : "--"}</strong></div>
        <div className="metric"><span>超额收益</span><strong>{metrics.excess_return != null ? formatSignedPercent(metrics.excess_return * 100) : "--"}</strong></div>
        <div className="metric"><span>Precision@N</span><strong>{metrics.precision_at_n != null ? formatPercent(metrics.precision_at_n * 100) : "--"}</strong></div>
      </div>

      {equityPointCount >= 2 ? (
        <section className="backtest-primary-chart">
          <EquityCurveChart
            portfolio={result.equity_curve}
            benchmark={result.benchmark_curve ?? []}
            symbolCount={metrics.num_stocks ?? result.symbols?.length ?? 0}
          />
        </section>
      ) : null}
      {equityPointCount === 1 ? (
        <p className="backtest-chart-empty">有效交易日不足，暂不绘制净值曲线。</p>
      ) : null}

      <section className="backtest-comparison">
        <div><span>股票数</span><strong>{metrics.num_stocks ?? result.symbols?.length ?? 0}</strong></div>
        <div><span>基准</span><strong>{result.benchmark_symbols?.length || "未启用"}</strong></div>
        <div><span>交易成本</span><strong>{formatMoney(metrics.total_transaction_cost)}</strong></div>
        <div><span>换手</span><strong>{formatNumber(metrics.total_turnover)}</strong></div>
        <div><span>调仓次数</span><strong>{metrics.rebalance_count ?? 0}</strong></div>
        <div><span>样本外折数</span><strong>{metrics.oos_fold_count ?? 0}</strong></div>
        <div><span>Mode</span><strong>{metrics.strategy_mode === "walk_forward" ? "Walk-forward" : "Snapshot"}</strong></div>
      </section>

      <VolatilityDiagnostics
        snapshots={result.volatility_snapshots ?? []}
        emptyMessage={result.volatility_message}
      />

      {result.walk_forward_folds?.length ? (
        <section className="backtest-holdings">
          <header><span>样本外逐折结果</span><strong>{metrics.oos_fold_count ?? 0}</strong></header>
          <div className="backtest-fold-list">
            {result.walk_forward_folds.map((fold) => (
              <div className="backtest-fold-row" key={`${fold.selection_date}-${fold.evaluation_end_date || "pending"}`}>
                <span>
                  <b>{fold.selection_date} → {fold.evaluation_end_date || "待评估"}</b>
                  <small>入选 {fold.selected_symbols.length} · 精度分母 {fold.evaluated_selection_count}/{fold.eligible_symbol_count}</small>
                </span>
                <span>
                  <b>{fold.precision_at_n != null ? `Precision ${formatPercent(fold.precision_at_n * 100)}` : "Precision --"}</b>
                  <small>超额 {fold.average_excess_return != null ? formatSignedPercent(fold.average_excess_return * 100) : "--"}</small>
                </span>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      {result.symbols?.length ? (
        <section className="backtest-holdings">
          <header><span>{result.strategy_mode === "walk_forward" ? "滚动入选标的" : "标的"}</span><strong>{result.symbols.length}</strong></header>
          <div className="symbol-strip">{result.symbols.join(" · ")}</div>
        </section>
      ) : null}

      {result.notes?.length ? <div className="notes">{result.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
      <RawJson result={result} />
    </div>
  );
}

function formatVolatilityPercent(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value)
    ? `${formatNumber(value)}%`
    : "无定义（零波动）";
}

const VolatilityDiagnostics = memo(function VolatilityDiagnostics({
  snapshots,
  emptyMessage,
}: {
  snapshots: VolatilitySnapshot[];
  emptyMessage?: string | null;
}) {
  const [selectedSymbol, setSelectedSymbol] = useState(() => snapshots[0]?.symbol ?? "");
  const snapshot = snapshots.find((item) => item.symbol === selectedSymbol) ?? snapshots[0];
  if (!snapshot) {
    return (
      <section className="backtest-volatility" aria-label="波动率快照">
        <header>
          <div>
            <span>波动率快照</span>
            <small>无可用区间末标的</small>
          </div>
        </header>
        <p className="volatility-empty">{emptyMessage || "波动率快照不可用。"}</p>
      </section>
    );
  }

  const unavailable = new Map((snapshot.unavailable ?? []).map((item) => [item.indicator, item.reason]));
  const reasonFor = (indicator: string) => unavailable.get(indicator) ?? "指标不可用";
  const atr = snapshot.atr;
  const bollinger = snapshot.bollinger_bands;
  const donchian = snapshot.donchian_channel;
  const keltner = snapshot.keltner_channel;
  const chaikin = snapshot.chaikin_volatility;
  const rvi = snapshot.rvi;
  const items = [
    {
      key: "atr",
      label: `ATR${atr?.period ?? 14}`,
      value: atr ? formatNumber(atr.value) : "--",
      detail: atr ? `占收盘 ${formatVolatilityPercent(atr.percent_of_close)}` : reasonFor("atr"),
    },
    {
      key: "bollinger_bands",
      label: `布林带 ${bollinger?.period ?? 20}/${bollinger?.multiplier ?? 2}`,
      value: bollinger ? `中轨 ${formatNumber(bollinger.middle)}` : "--",
      detail: bollinger
        ? `%B ${formatVolatilityPercent(bollinger.percent_b)} · 带宽 ${formatVolatilityPercent(bollinger.bandwidth_percent)}`
        : reasonFor("bollinger_bands"),
    },
    {
      key: "donchian_channel",
      label: `唐奇安通道 ${donchian?.period ?? 20}`,
      value: donchian ? `${formatNumber(donchian.lower)}–${formatNumber(donchian.upper)}` : "--",
      detail: donchian
        ? `位置 ${formatVolatilityPercent(donchian.position_percent)} · 宽度 ${formatVolatilityPercent(donchian.width_percent)}`
        : reasonFor("donchian_channel"),
    },
    {
      key: "keltner_channel",
      label: `凯尔特纳通道 ${keltner?.ema_period ?? 20}/${keltner?.atr_period ?? 10}/${keltner?.multiplier ?? 2}`,
      value: keltner ? `${formatNumber(keltner.lower)}–${formatNumber(keltner.upper)}` : "--",
      detail: keltner
        ? `位置 ${formatVolatilityPercent(keltner.position_percent)} · 宽度 ${formatVolatilityPercent(keltner.width_percent)}`
        : reasonFor("keltner_channel"),
    },
    {
      key: "chaikin_volatility",
      label: `Chaikin 波动率 ${chaikin?.ema_period ?? 10}/${chaikin?.roc_period ?? 10}`,
      value: chaikin ? formatVolatilityPercent(chaikin.value) : "--",
      detail: chaikin
        ? `${chaikin.ema_period} 日高低价差 EMA 相对 ${chaikin.roc_period} 日前`
        : reasonFor("chaikin_volatility"),
    },
    {
      key: "rvi",
      label: `相对波动率指数 RVI${rvi?.period ?? 14}`,
      value: rvi ? formatNumber(rvi.value) : "--",
      detail: rvi ? "范围 0–100，50 为方向均衡线" : reasonFor("rvi"),
    },
  ];

  return (
    <section className="backtest-volatility" aria-label="波动率快照">
      <header>
        <div>
          <span>波动率快照</span>
          <small>{snapshot.date} · 收盘 {formatNumber(snapshot.close)}</small>
        </div>
        <label className="volatility-symbol-control">
          <span>波动率标的</span>
          <select
            aria-label="波动率标的"
            value={snapshot.symbol}
            onChange={(event) => setSelectedSymbol(event.target.value)}
          >
            {snapshots.map((item) => <option key={item.symbol} value={item.symbol}>{item.symbol}</option>)}
          </select>
        </label>
      </header>
      <div className="volatility-grid">
        {items.map((item) => (
          <div key={item.key}>
            <span>{item.label}</span>
            <strong>{item.value}</strong>
            <small>{item.detail}</small>
          </div>
        ))}
      </div>
    </section>
  );
});

const EQUITY_CHART = {
  width: 760,
  height: 210,
  left: 46,
  right: 748,
  top: 14,
  bottom: 190,
} as const;

interface NormalizedEquityPoint {
  date: string;
  value: number;
}

function normalizeEquityCurve(curve: EquityPoint[]): NormalizedEquityPoint[] {
  const valid = curve
    .map((point) => ({ date: point.date, equity: Number(point.equity) }))
    .filter((point) => point.date && Number.isFinite(point.equity));
  const base = valid[0]?.equity;
  if (!Number.isFinite(base) || base === 0) return [];
  return valid.map((point) => ({ date: point.date, value: (point.equity / base) * 100 }));
}

function curveReturn(curve: NormalizedEquityPoint[]): number | null {
  const last = curve[curve.length - 1]?.value;
  return Number.isFinite(last) ? last - 100 : null;
}

function EquityCurveChart({
  portfolio,
  benchmark,
  symbolCount,
}: {
  portfolio: EquityPoint[];
  benchmark: EquityPoint[];
  symbolCount: number;
}) {
  const chart = useMemo(() => {
    const portfolioSeries = normalizeEquityCurve(portfolio);
    const benchmarkSeries = normalizeEquityCurve(benchmark);
    const dates = Array.from(new Set([
      ...portfolioSeries.map((point) => point.date),
      ...benchmarkSeries.map((point) => point.date),
    ])).sort();
    const dateIndexes = new Map(dates.map((date, index) => [date, index]));
    const values = [
      100,
      ...portfolioSeries.map((point) => point.value),
      ...benchmarkSeries.map((point) => point.value),
    ];
    const rawMin = Math.min(...values);
    const rawMax = Math.max(...values);
    const visibleRange = Math.max(rawMax - rawMin, 4);
    const min = Math.max(0, rawMin - visibleRange * 0.12);
    const max = rawMax + visibleRange * 0.12;
    const plotWidth = EQUITY_CHART.right - EQUITY_CHART.left;
    const plotHeight = EQUITY_CHART.bottom - EQUITY_CHART.top;
    const xForDate = (date: string) => {
      const index = dateIndexes.get(date) ?? 0;
      return EQUITY_CHART.left + (index / Math.max(dates.length - 1, 1)) * plotWidth;
    };
    const yForValue = (value: number) => EQUITY_CHART.top + ((max - value) / (max - min)) * plotHeight;
    const pointsFor = (series: NormalizedEquityPoint[]) => series
      .map((point) => `${xForDate(point.date).toFixed(2)},${yForValue(point.value).toFixed(2)}`)
      .join(" ");
    const ticks = Array.from({ length: 5 }, (_, index) => max - ((max - min) * index) / 4);

    return {
      portfolioSeries,
      benchmarkSeries,
      dates,
      portfolioByDate: new Map(portfolioSeries.map((point) => [point.date, point.value])),
      benchmarkByDate: new Map(benchmarkSeries.map((point) => [point.date, point.value])),
      portfolioPoints: pointsFor(portfolioSeries),
      benchmarkPoints: pointsFor(benchmarkSeries),
      ticks,
      xForDate,
      yForValue,
    };
  }, [benchmark, portfolio]);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);

  if (chart.portfolioSeries.length < 2 || chart.dates.length < 2) return null;

  const updateActivePoint = (clientX: number, bounds: DOMRect) => {
    const chartX = ((clientX - bounds.left) / Math.max(bounds.width, 1)) * EQUITY_CHART.width;
    const ratio = Math.max(0, Math.min(
      1,
      (chartX - EQUITY_CHART.left) / (EQUITY_CHART.right - EQUITY_CHART.left),
    ));
    setActiveIndex(Math.round(ratio * (chart.dates.length - 1)));
  };
  const activeDate = activeIndex == null ? null : chart.dates[activeIndex];
  const activePortfolio = activeDate ? chart.portfolioByDate.get(activeDate) : undefined;
  const activeBenchmark = activeDate ? chart.benchmarkByDate.get(activeDate) : undefined;
  const activeX = activeDate ? chart.xForDate(activeDate) : 0;
  const tooltipAlignment = activeIndex != null && activeIndex <= chart.dates.length * 0.18
    ? "is-start"
    : activeIndex != null && activeIndex >= chart.dates.length * 0.82
      ? "is-end"
      : "";

  return (
    <div className="equity-chart">
      <header className="equity-chart-header">
        <div>
          <strong>组合净值曲线</strong>
          <small>{symbolCount > 0 ? `${symbolCount} 只股票组合` : "回测组合"} · 起点归一为 100</small>
        </div>
        <div className="equity-chart-legend" aria-label="净值曲线图例">
          <span><i className="is-portfolio" />组合净值 <b>{formatSignedPercent(curveReturn(chart.portfolioSeries))}</b></span>
          {chart.benchmarkSeries.length ? (
            <span><i className="is-benchmark" />候选池基准 <b>{formatSignedPercent(curveReturn(chart.benchmarkSeries))}</b></span>
          ) : null}
        </div>
      </header>
      <div
        className="equity-chart-plot"
        tabIndex={0}
        aria-label="使用左右方向键查看净值数据点"
        onBlur={() => setActiveIndex(null)}
        onFocus={() => setActiveIndex((current) => current ?? chart.dates.length - 1)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault();
          const step = event.key === "ArrowLeft" ? -1 : 1;
          setActiveIndex((current) => Math.max(0, Math.min(chart.dates.length - 1, (current ?? chart.dates.length - 1) + step)));
        }}
        onPointerDown={(event) => updateActivePoint(event.clientX, event.currentTarget.getBoundingClientRect())}
        onPointerMove={(event) => updateActivePoint(event.clientX, event.currentTarget.getBoundingClientRect())}
        onPointerLeave={() => setActiveIndex(null)}
      >
        <svg
          viewBox={`0 0 ${EQUITY_CHART.width} ${EQUITY_CHART.height}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={chart.benchmarkSeries.length ? "组合与基准净值曲线" : "组合净值曲线"}
        >
          <title>组合净值曲线，起点归一为 100</title>
          <g className="equity-chart-grid" aria-hidden="true">
            {chart.ticks.map((tick) => {
              const y = chart.yForValue(tick);
              return (
                <g key={tick.toFixed(4)}>
                  <line x1={EQUITY_CHART.left} x2={EQUITY_CHART.right} y1={y} y2={y} />
                  <text x="2" y={y + 4}>{tick.toFixed(chart.ticks[0] - chart.ticks[4] < 20 ? 1 : 0)}</text>
                </g>
              );
            })}
          </g>
          <line
            className="equity-chart-baseline"
            x1={EQUITY_CHART.left}
            x2={EQUITY_CHART.right}
            y1={chart.yForValue(100)}
            y2={chart.yForValue(100)}
          />
          {chart.benchmarkPoints ? (
            <polyline className="equity-chart-line is-benchmark" points={chart.benchmarkPoints} />
          ) : null}
          <polyline className="equity-chart-line is-portfolio" points={chart.portfolioPoints} />
          {activeDate ? (
            <g className="equity-chart-cursor" aria-hidden="true">
              <line x1={activeX} x2={activeX} y1={EQUITY_CHART.top} y2={EQUITY_CHART.bottom} />
              {activePortfolio != null ? <circle className="is-portfolio" cx={activeX} cy={chart.yForValue(activePortfolio)} r="4" /> : null}
              {activeBenchmark != null ? <circle className="is-benchmark" cx={activeX} cy={chart.yForValue(activeBenchmark)} r="3.5" /> : null}
            </g>
          ) : null}
        </svg>
        {activeDate ? (
          <div
            className={`equity-chart-tooltip ${tooltipAlignment}`}
            style={{ left: `${(activeX / EQUITY_CHART.width) * 100}%` }}
          >
            <strong>{activeDate}</strong>
            <span><i className="is-portfolio" />组合 {activePortfolio != null ? formatNumber(activePortfolio) : "--"}</span>
            {chart.benchmarkSeries.length ? (
              <span><i className="is-benchmark" />基准 {activeBenchmark != null ? formatNumber(activeBenchmark) : "--"}</span>
            ) : null}
          </div>
        ) : null}
      </div>
      <div className="chart-labels">
        <span>{chart.dates[0]}</span>
        <span>净值指数</span>
        <span>{chart.dates[chart.dates.length - 1]}</span>
      </div>
    </div>
  );
}
