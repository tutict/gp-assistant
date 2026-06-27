import { useCallback, useEffect, useState } from "react";
import type { CapitalEvidenceSection, FinancialIndicatorItem, ObserveResult, WatchlistItem } from "../../types";
import { getJson } from "../../lib/tauri";
import { StockCodeInput } from "../StockCodeInput";
import {
  currentSystemDateInputValue,
  defaultObserveStartDateInputValue,
  escapeHtml,
  formatNumber,
  formatPercent,
  formatPrice,
  normalizeStockCode,
} from "../../lib/format";

interface ObservePanelProps {
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  initialCode?: string;
}

export function ObservePanel({ initialCode }: ObservePanelProps) {
  const [code, setCode] = useState(initialCode || "");
  const [startDate, setStartDate] = useState(defaultObserveStartDateInputValue());
  const [endDate, setEndDate] = useState(currentSystemDateInputValue());
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<ObserveResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (initialCode) setCode(initialCode);
  }, [initialCode]);

  const runObserve = useCallback(async () => {
    const normalizedCode = normalizeStockCode(code);
    if (!normalizedCode) {
      setError("Please enter a valid stock code.");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const query = new URLSearchParams({
        start_date: startDate.replace(/-/g, ""),
        end_date: endDate.replace(/-/g, ""),
        series_limit: "160",
        include_order_book: "false",
        include_chip_distribution: "true",
      });
      const data = await getJson<ObserveResult>(`/api/observe/${encodeURIComponent(normalizedCode)}?${query}`);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [code, startDate, endDate]);

  return (
    <div className="panel-container">
      <div className="panel-controls">
        <div className="form-row inline stock-code-row">
          <label htmlFor="observeCode">Code</label>
          <StockCodeInput id="observeCode" value={code} onChange={setCode} placeholder="输入股票代码或名称" />
        </div>
        <div className="form-row inline"><label htmlFor="observeStart">Start</label><input id="observeStart" type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="observeEnd">End</label><input id="observeEnd" type="date" value={endDate} onChange={(e) => setEndDate(e.target.value)} /></div>
        <button type="button" className="run-btn" onClick={runObserve} disabled={loading}>{loading ? "Observing..." : "Observe"}</button>
      </div>

      <div className="panel-result">
        {error && <div className="result-error"><strong>Observe failed</strong><p>{escapeHtml(error)}</p></div>}
        {loading && !result && !error && <div className="result-loading"><div className="loader" /><span>Loading observation...</span></div>}
        {result && !loading && <ObserveResultView result={result} />}
        {!result && !loading && !error && <div className="result-empty"><span>Enter a stock code, then observe.</span></div>}
      </div>
    </div>
  );
}

export function ObserveResultView({ result }: { result: ObserveResult }) {
  const stock = result.stock || { code: "", name: "" };
  const trend = result.trend;
  const signal = trend?.signal;
  const series = trend?.series || [];
  const financial = result.financial_indicators;
  const capital = result.capital_evidence;

  return (
    <div className="observe-result">
      <div className="metric-strip">
        <div className="metric"><span>Source</span><strong>{result.source || "--"}</strong></div>
        <div className="metric"><span>Price</span><strong>{formatPrice(stock.price)}</strong></div>
        <div className="metric"><span>K/D/J</span><strong>{formatNumber(signal?.k)} / {formatNumber(signal?.d)} / {formatNumber(signal?.j)}</strong></div>
      </div>

      <section className="observe-overview">
        <header className="observe-overview-header">
          <div><h3>{stock.name || stock.code}</h3><p>{stock.code} {stock.industry || ""}</p></div>
          {signal?.status && <span className={`state-pill ${signal.status}`}>{signal.status}</span>}
        </header>
        <div className="overview-metric-grid">
          <Metric label="PE" value={formatNumber(stock.pe)} />
          <Metric label="PB" value={formatNumber(stock.pb)} />
          <Metric label="ROE" value={formatPercent(stock.roe)} />
          <Metric label="Market cap" value={formatNumber(stock.market_cap_billion)} />
        </div>
      </section>

      {signal && (
        <section className="signal-card">
          <header><div><h3>Trend signal</h3><p>{signal.date || ""}</p></div><span className="state-pill">{signal.status || "neutral"}</span></header>
          <div className="signal-grid">
            <Metric label="Close" value={formatPrice(signal.close)} />
            <Metric label="SWL/SWS" value={`${formatNumber(signal.swl)} / ${formatNumber(signal.sws)}`} />
            <Metric label="KDJ" value={`${formatNumber(signal.k)} / ${formatNumber(signal.d)} / ${formatNumber(signal.j)}`} />
            <Metric label="Quant" value={`${signal.quant_score ?? 0}/${signal.quant_score_max ?? 90}`} />
            <Metric label="Pattern" value={`${signal.pattern_score ?? 0}/${signal.pattern_score_max ?? 100}`} />
            <Metric label="Support" value={formatNumber(signal.support)} />
            <Metric label="Resistance" value={formatNumber(signal.resistance)} />
          </div>
          {signal.reasons?.length ? <div className="tag-row">{signal.reasons.map((reason) => <span key={reason}>{reason}</span>)}</div> : null}
          {series.length > 1 ? <TrendCharts series={series} /> : null}
        </section>
      )}

      {financial?.items?.length ? <FinancialIndicators items={financial.items} title={financial.title || "Financial indicators"} /> : null}
      {capital ? <CapitalEvidence sections={capital.sections || []} summary={capital.summary} notes={capital.notes || []} /> : null}

      {[...(result.notes || []), ...(signal?.notes || [])].length ? (
        <div className="notes">{[...(result.notes || []), ...(signal?.notes || [])].map((note) => <p key={note}>{note}</p>)}</div>
      ) : null}
      <details className="raw-json"><summary>Raw JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string | number }) {
  return <div><span>{label}</span><strong>{value}</strong></div>;
}

function FinancialIndicators({ items, title }: { items: FinancialIndicatorItem[]; title: string }) {
  const filtered = items.filter((item) => item.metric_key !== "quarterly_eps" && item.value !== undefined && item.value !== null && item.value !== "");
  return (
    <section className="financial-indicators">
      <header><h3>{title}</h3></header>
      <div className="financial-indicator-grid">
        {filtered.slice(0, 16).map((item, index) => (
          <div key={`${item.label}-${index}`} className="financial-indicator-item">
            <span className="financial-indicator-label">{item.label || item.metric_key || "--"}</span>
            <strong className={`financial-indicator-value ${item.tone || "neutral"}`}>{String(item.value ?? "--")}</strong>
          </div>
        ))}
      </div>
    </section>
  );
}

function CapitalEvidence({ sections, summary, notes }: { sections: CapitalEvidenceSection[]; summary?: string | null; notes: string[] }) {
  return (
    <section className="capital-evidence-list">
      <header><h3>Capital evidence</h3>{summary && <p>{summary}</p>}</header>
      {sections.filter((section) => section.items?.length || section.summary).slice(0, 6).map((section) => (
        <article key={section.key} className="capital-section">
          <h4>{section.title}</h4>
          {section.summary && <p>{section.summary}</p>}
          <div className="evidence-list">
            {(section.items || []).slice(0, 4).map((item, index) => (
              <article key={`${item.title}-${index}`}>
                <strong>{item.title || item.source || "--"}</strong>
                <span className="evidence-source">{item.source || ""} {item.date || ""}</span>
                {item.note && <p>{item.note}</p>}
              </article>
            ))}
          </div>
        </article>
      ))}
      {notes.length ? <div className="notes">{notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
    </section>
  );
}

function TrendCharts({ series }: { series: { date: string; close?: number; swl?: number | null; sws?: number | null; k?: number | null; d?: number | null; j?: number | null }[] }) {
  return (
    <div className="signal-chart-stack">
      <LineChart title="Close / SWL / SWS" series={series} keys={["close", "swl", "sws"]} />
      <LineChart title="KDJ" series={series} keys={["k", "d", "j"]} />
    </div>
  );
}

function LineChart({ title, series, keys }: { title: string; series: Record<string, unknown>[]; keys: string[] }) {
  const width = 720;
  const height = 160;
  const points = series.map((point) => keys.map((key) => Number(point[key])).filter(Number.isFinite)).flat();
  if (points.length < 2) return null;
  const min = Math.min(...points);
  const max = Math.max(...points);
  const range = max - min || 1;
  const line = (key: string) => series.map((point, index) => {
    const value = Number(point[key]);
    if (!Number.isFinite(value)) return null;
    const x = (index / Math.max(series.length - 1, 1)) * width;
    const y = height - ((value - min) / range) * height;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  }).filter(Boolean).join(" ");
  return (
    <section className="chart-wrap trend-chart">
      <header><h4>{title}</h4></header>
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={title}>
        {keys.map((key) => <polyline key={key} className={`line-${key}`} points={line(key)} fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />)}
      </svg>
      <div className="chart-labels"><span>{String(series[0]?.date || "")}</span><span>{String(series[series.length - 1]?.date || "")}</span></div>
    </section>
  );
}
