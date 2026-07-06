import { useCallback, useEffect, useState } from "react";
import type { BacktestResult, WatchlistItem } from "../../types";
import type { FilterCriteria } from "../FilterBar";
import { postJson } from "../../lib/tauri";
import { buildBacktestRequest } from "../../lib/contracts";
import {
  currentSystemDateInputValue,
  escapeHtml,
  formatMoney,
  formatNumber,
  formatSignedPercent,
  shortBenchmarkLabel,
  shortRebalanceLabel,
} from "../../lib/format";

interface BacktestPanelProps {
  criteria: FilterCriteria;
  watchlist: WatchlistItem[];
  preferredSource?: BacktestSource | null;
}

type BacktestSource = "criteria" | "watchlist";

export function BacktestPanel({ criteria, watchlist, preferredSource }: BacktestPanelProps) {
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

  useEffect(() => {
    if (preferredSource) setSource(preferredSource);
  }, [preferredSource]);

  const run = useCallback(async () => {
    if (source === "watchlist" && watchlist.length === 0) {
      setError("自选股为空，请先从筛选结果中收藏股票。");
      return;
    }
    setLoading(true);
    setError(null);
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
      const data = await postJson<BacktestResult>("/api/backtest", payload);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [benchmark, costBps, criteria, end, rebalance, source, start, strategyMode, topN, watchlist]);

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
          <button type="button" className={`source-toggle ${source === "criteria" ? "active" : ""}`} onClick={() => setSource("criteria")}>当前条件</button>
          <button type="button" className={`source-toggle ${source === "watchlist" ? "active" : ""}`} onClick={() => setSource("watchlist")}>自选股</button>
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
        <div className="form-row inline"><label htmlFor="btStart">开始</label><input id="btStart" type="date" value={start} onChange={(e) => setStart(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btEnd">结束</label><input id="btEnd" type="date" value={end} onChange={(e) => setEnd(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btTopN">持仓</label><input id="btTopN" type="number" min="1" max="100" value={topN} onChange={(e) => setTopN(Number(e.target.value) || 10)} /></div>
        <div className="form-row inline">
          <label htmlFor="btRebalance">调仓</label>
          <select id="btRebalance" value={rebalance} onChange={(e) => setRebalance(e.target.value)}>
            <option value="none">买入持有</option>
            <option value="monthly">月度调仓</option>
            <option value="quarterly">季度调仓</option>
          </select>
        </div>
        <div className="form-row inline">
          <label htmlFor="btBenchmark">基准</label>
          <select id="btBenchmark" value={benchmark} onChange={(e) => setBenchmark(e.target.value)}>
            <option value="candidate_equal_weight">候选池等权</option>
            <option value="none">无</option>
          </select>
        </div>
        <div className="form-row inline"><label htmlFor="btStrategyMode">模式</label><select id="btStrategyMode" value={strategyMode} onChange={(e) => setStrategyMode(e.target.value)}><option value="candidate_snapshot">候选快照</option><option value="walk_forward">Walk-forward</option></select></div>
        <div className="form-row inline"><label htmlFor="btCostBps">成本</label><input id="btCostBps" type="number" min="0" max="500" value={costBps} onChange={(e) => setCostBps(Number(e.target.value) || 0)} /></div>
        <button type="button" className="run-btn" onClick={run} disabled={loading}>{loading ? "回测中..." : "运行回测"}</button>
      </div>

      <div className="panel-result">
        {error && <div className="result-error"><strong>回测失败</strong><p>{escapeHtml(error)}</p></div>}
        {loading && !result && !error && <div className="result-loading"><div className="loader" /><span>正在回测...</span></div>}
        {result && !loading && <BacktestResultView result={result} />}
        {!result && !loading && !error && <div className="result-empty"><span>设置参数后运行回测。</span></div>}
      </div>
    </div>
  );
}

export function BacktestResultView({ result }: { result: BacktestResult }) {
  const metrics = result.metrics || {};
  return (
    <div className="backtest-result">
      <div className="metric-strip">
        <div className="metric"><span>总收益</span><strong className={metrics.total_return > 0 ? "positive" : "negative"}>{formatSignedPercent((metrics.total_return ?? 0) * 100)}</strong></div>
        <div className="metric"><span>年化收益</span><strong>{metrics.annualized_return != null ? formatSignedPercent(metrics.annualized_return * 100) : "--"}</strong></div>
        <div className="metric"><span>最大回撤</span><strong className="negative">{metrics.max_drawdown != null ? formatSignedPercent(metrics.max_drawdown * 100) : "--"}</strong></div>
        <div className="metric"><span>超额收益</span><strong>{metrics.excess_return != null ? formatSignedPercent(metrics.excess_return * 100) : "--"}</strong></div>
      </div>

      {result.equity_curve?.length ? <section className="backtest-primary-chart"><Sparkline curve={result.equity_curve} /></section> : null}

      <section className="backtest-comparison">
        <div><span>股票数</span><strong>{metrics.num_stocks ?? result.symbols?.length ?? 0}</strong></div>
        <div><span>基准</span><strong>{result.benchmark_symbols?.length || "未启用"}</strong></div>
        <div><span>交易成本</span><strong>{formatMoney(metrics.total_transaction_cost)}</strong></div>
        <div><span>换手</span><strong>{formatNumber(metrics.total_turnover)}</strong></div>
        <div><span>调仓次数</span><strong>{metrics.rebalance_count ?? 0}</strong></div>
        <div><span>Mode</span><strong>{metrics.strategy_mode === "walk_forward" ? "Walk-forward" : "Snapshot"}</strong></div>
      </section>

      {result.symbols?.length ? (
        <section className="backtest-holdings">
          <header><span>标的</span><strong>{result.symbols.length}</strong></header>
          <div className="symbol-strip">{result.symbols.join(" · ")}</div>
        </section>
      ) : null}

      {result.notes?.length ? <div className="notes">{result.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function Sparkline({ curve }: { curve: { date: string; equity: number }[] }) {
  if (curve.length < 2) return null;
  const values = curve.map((point) => Number(point.equity)).filter(Number.isFinite);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const points = values.map((value, index) => {
    const x = (index / Math.max(values.length - 1, 1)) * 720;
    const y = 150 - ((value - min) / range) * 150;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  }).join(" ");
  return (
    <div className="chart-wrap">
      <svg viewBox="0 0 720 150" role="img" aria-label="净值曲线">
        <polyline points={points} fill="none" stroke="currentColor" strokeWidth="4" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <div className="chart-labels"><span>{curve[0]?.date}</span><span>{curve[curve.length - 1]?.date}</span></div>
    </div>
  );
}
