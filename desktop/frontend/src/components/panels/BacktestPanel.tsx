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
  const [costBps, setCostBps] = useState(10);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (preferredSource) setSource(preferredSource);
  }, [preferredSource]);

  const run = useCallback(async () => {
    if (source === "watchlist" && watchlist.length === 0) {
      setError("Watchlist is empty. Save stocks from screening first.");
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
      });
      const data = await postJson<BacktestResult>("/api/backtest", payload);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [benchmark, costBps, criteria, end, rebalance, source, start, topN, watchlist]);

  const sourceText = source === "watchlist"
    ? `${Math.min(topN, watchlist.length)} / ${watchlist.length} stocks`
    : (criteria.industry || "All industries");

  return (
    <div className="panel-container">
      <div className="backtest-context">
        <div className="backtest-context-head">
          <span>{source === "watchlist" ? "Watchlist" : "Current criteria"}</span>
          <strong>{sourceText}</strong>
        </div>
        <div className="backtest-context-controls">
          <button type="button" className={`source-toggle ${source === "criteria" ? "active" : ""}`} onClick={() => setSource("criteria")}>Criteria</button>
          <button type="button" className={`source-toggle ${source === "watchlist" ? "active" : ""}`} onClick={() => setSource("watchlist")}>Watchlist</button>
        </div>
        <div className="backtest-param-strip">
          <span><b>Hold</b><strong>{topN}</strong></span>
          <span><b>Range</b><strong>{start}~{end}</strong></span>
          <span><b>Rebalance</b><strong>{shortRebalanceLabel(rebalance)}</strong></span>
          <span><b>Cost</b><strong>{formatNumber(costBps)}bps</strong></span>
          <span><b>Benchmark</b><strong>{shortBenchmarkLabel(benchmark)}</strong></span>
        </div>
      </div>

      <div className="panel-controls">
        <div className="form-row inline"><label htmlFor="btStart">Start</label><input id="btStart" type="date" value={start} onChange={(e) => setStart(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btEnd">End</label><input id="btEnd" type="date" value={end} onChange={(e) => setEnd(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btTopN">Top N</label><input id="btTopN" type="number" min="1" max="100" value={topN} onChange={(e) => setTopN(Number(e.target.value) || 10)} /></div>
        <div className="form-row inline">
          <label htmlFor="btRebalance">Rebalance</label>
          <select id="btRebalance" value={rebalance} onChange={(e) => setRebalance(e.target.value)}>
            <option value="none">Buy & hold</option>
            <option value="monthly">Monthly</option>
            <option value="quarterly">Quarterly</option>
          </select>
        </div>
        <div className="form-row inline">
          <label htmlFor="btBenchmark">Benchmark</label>
          <select id="btBenchmark" value={benchmark} onChange={(e) => setBenchmark(e.target.value)}>
            <option value="candidate_equal_weight">Candidate equal weight</option>
            <option value="none">None</option>
          </select>
        </div>
        <div className="form-row inline"><label htmlFor="btCostBps">Cost bps</label><input id="btCostBps" type="number" min="0" max="500" value={costBps} onChange={(e) => setCostBps(Number(e.target.value) || 0)} /></div>
        <button type="button" className="run-btn" onClick={run} disabled={loading}>{loading ? "Running..." : "Run backtest"}</button>
      </div>

      <div className="panel-result">
        {error && <div className="result-error"><strong>Backtest failed</strong><p>{escapeHtml(error)}</p></div>}
        {loading && !result && !error && <div className="result-loading"><div className="loader" /><span>Running backtest...</span></div>}
        {result && !loading && <BacktestResultView result={result} />}
        {!result && !loading && !error && <div className="result-empty"><span>Configure parameters, then run backtest.</span></div>}
      </div>
    </div>
  );
}

export function BacktestResultView({ result }: { result: BacktestResult }) {
  const metrics = result.metrics || {};
  return (
    <div className="backtest-result">
      <div className="metric-strip">
        <div className="metric"><span>Total return</span><strong className={metrics.total_return > 0 ? "positive" : "negative"}>{formatSignedPercent((metrics.total_return ?? 0) * 100)}</strong></div>
        <div className="metric"><span>Annualized</span><strong>{metrics.annualized_return != null ? formatSignedPercent(metrics.annualized_return * 100) : "--"}</strong></div>
        <div className="metric"><span>Max drawdown</span><strong className="negative">{metrics.max_drawdown != null ? formatSignedPercent(metrics.max_drawdown * 100) : "--"}</strong></div>
        <div className="metric"><span>Excess</span><strong>{metrics.excess_return != null ? formatSignedPercent(metrics.excess_return * 100) : "--"}</strong></div>
      </div>

      {result.equity_curve?.length ? <section className="backtest-primary-chart"><Sparkline curve={result.equity_curve} /></section> : null}

      <section className="backtest-comparison">
        <div><span>Stocks</span><strong>{metrics.num_stocks ?? result.symbols?.length ?? 0}</strong></div>
        <div><span>Benchmark</span><strong>{result.benchmark_symbols?.length || "disabled"}</strong></div>
        <div><span>Transaction cost</span><strong>{formatMoney(metrics.total_transaction_cost)}</strong></div>
        <div><span>Turnover</span><strong>{formatNumber(metrics.total_turnover)}</strong></div>
        <div><span>Rebalances</span><strong>{metrics.rebalance_count ?? 0}</strong></div>
      </section>

      {result.symbols?.length ? (
        <section className="backtest-holdings">
          <header><span>Symbols</span><strong>{result.symbols.length}</strong></header>
          <div className="symbol-strip">{result.symbols.join(" · ")}</div>
        </section>
      ) : null}

      {result.notes?.length ? <div className="notes">{result.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
      <details className="raw-json"><summary>Raw JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
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
      <svg viewBox="0 0 720 150" role="img" aria-label="Equity curve">
        <polyline points={points} fill="none" stroke="currentColor" strokeWidth="4" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <div className="chart-labels"><span>{curve[0]?.date}</span><span>{curve[curve.length - 1]?.date}</span></div>
    </div>
  );
}
