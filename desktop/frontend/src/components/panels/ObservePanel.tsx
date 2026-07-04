import { useCallback, useEffect, useState } from "react";
import type { CapitalEvidenceSection, FinancialIndicatorItem, ObserveResult, TrendIndicatorPoint, WatchlistItem } from "../../types";
import { computeKdj, toDailyBars } from "../../lib/kline";
import { getJson } from "../../lib/tauri";
import { CollapsibleNotes } from "../CollapsibleNotes";
import { StockCodeInput } from "../StockCodeInput";
import { TrendCharts } from "../observe/ObserveCharts";
import {
  currentSystemDateInputValue,
  escapeHtml,
  formatNumber,
  formatPrice,
  formatRatioPercent,
  normalizeStockCode,
  reasonLabel,
} from "../../lib/format";

interface ObservePanelProps {
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  initialCode?: string;
}

const OBSERVE_FULL_HISTORY_START = "1990-01-01";
const OBSERVE_FULL_HISTORY_LIMIT = "10000";
export function ObservePanel({ initialCode }: ObservePanelProps) {
  const [code, setCode] = useState(initialCode || "");
  const [startDate, setStartDate] = useState(OBSERVE_FULL_HISTORY_START);
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
      setError("请输入有效股票代码。");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const query = new URLSearchParams({
        start_date: startDate.replace(/-/g, ""),
        end_date: endDate.replace(/-/g, ""),
        series_limit: OBSERVE_FULL_HISTORY_LIMIT,
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
          <label htmlFor="observeCode">股票代码</label>
          <StockCodeInput id="observeCode" value={code} onChange={setCode} placeholder="输入股票代码或名称" />
        </div>
        <div className="form-row inline"><label htmlFor="observeStart">开始日期</label><input id="observeStart" type="date" value={startDate} onChange={(e) => setStartDate(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="observeEnd">结束日期</label><input id="observeEnd" type="date" value={endDate} onChange={(e) => setEndDate(e.target.value)} /></div>
        <button type="button" className="run-btn" onClick={runObserve} disabled={loading}>{loading ? "观察中..." : "开始观察"}</button>
      </div>

      <div className="panel-result">
        {error && <div className="result-error"><strong>观察失败</strong><p>{escapeHtml(error)}</p></div>}
        {loading && !result && !error && <div className="result-loading"><div className="loader" /><span>正在加载观察结果...</span></div>}
        {result && !loading && <ObserveResultView result={result} />}
        {!result && !loading && !error && <div className="result-empty"><span>输入股票代码后开始观察。</span></div>}
      </div>
    </div>
  );
}

export function ObserveResultView({ result }: { result: ObserveResult }) {
  const stock = result.stock || { code: "", name: "" };
  const trend = result.trend;
  const signal = trend?.signal;
  const series = trend?.series || [];
  const displayKdj = kdjFromSignal(signal) || latestKdjFromSeries(series);
  const financial = result.financial_indicators;
  const capital = result.capital_evidence;

  return (
    <div className="observe-result">
      <div className="metric-strip">
        <div className="metric"><span>来源</span><strong>{result.source || "--"}</strong></div>
        <div className="metric"><span>价格</span><strong>{formatPrice(stock.price)}</strong></div>
        <div className="metric"><span>K/D/J</span><strong>{formatKdj(displayKdj)}</strong></div>
      </div>

      <section className="observe-overview">
        <header className="observe-overview-header">
          <div><h3>{stock.name || stock.code}</h3><p>{stock.code} {stock.industry || ""}</p></div>
          {signal?.status && <span className={`state-pill ${signalStatusTone(signal.status)}`}>{signalStatusLabel(signal.status)}</span>}
        </header>
        <div className="overview-metric-grid">
          <Metric label="市盈率" value={formatNumber(stock.pe)} />
          <Metric label="市净率" value={formatNumber(stock.pb)} />
          <Metric label="净资产收益率" value={formatRatioPercent(stock.roe)} />
          <Metric label="市值" value={formatNumber(stock.market_cap_billion)} />
        </div>
      </section>

      {!signal && series.length > 1 ? (
        <section className="signal-card observe-chart-card">
          <header><div><h3>行情图表</h3><p>K线、均线、KDJ 与 MACD</p></div></header>
          <TrendCharts series={series} />
        </section>
      ) : null}

      {signal && (
        <section className="signal-card observe-chart-card">
          <header><div><h3>趋势信号</h3><p>{signal.date || ""}</p></div><span className={`state-pill ${signalStatusTone(signal.status)}`}>{signalStatusLabel(signal.status)}</span></header>
          <div className="signal-grid">
            <Metric label="收盘" value={formatPrice(signal.close)} />
            <Metric label="SWL/SWS" value={`${formatNumber(signal.swl)} / ${formatNumber(signal.sws)}`} />
            <Metric label="KDJ" value={formatKdj(displayKdj || kdjFromSignal(signal))} />
            <Metric label="量化分" value={`${signal.quant_score ?? 0}/${signal.quant_score_max ?? 90}`} />
            <Metric label="形态分" value={`${signal.pattern_score ?? 0}/${signal.pattern_score_max ?? 100}`} />
            <Metric label="支撑" value={formatNumber(signal.support)} />
            <Metric label="压力" value={formatNumber(signal.resistance)} />
          </div>
          {signal.reasons?.length ? <div className="tag-row">{signal.reasons.map((reason) => <span key={reason}>{reasonLabel(reason)}</span>)}</div> : null}
          {series.length > 1 ? <TrendCharts series={series} /> : null}
        </section>
      )}

      {financial?.items?.length ? <FinancialIndicators items={financial.items} title={financial.title || "财务指标"} /> : null}
      {capital ? <CapitalEvidence sections={capital.sections || []} summary={capital.summary} notes={capital.notes || []} /> : null}

      <CollapsibleNotes notes={[...(result.notes || []), ...(signal?.notes || [])]} />
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}

function kdjFromSignal(signal: { k?: number | null; d?: number | null; j?: number | null } | null | undefined) {
  const k = finiteNumber(signal?.k);
  const d = finiteNumber(signal?.d);
  const j = finiteNumber(signal?.j);
  return k == null || d == null || j == null ? null : { k, d, j };
}

function latestKdjFromSeries(series: TrendIndicatorPoint[]) {
  for (let index = series.length - 1; index >= 0; index -= 1) {
    const kdj = kdjFromSignal(series[index]);
    if (kdj) return kdj;
  }
  const bars = toDailyBars(series);
  const computed = computeKdj(bars);
  return computed.length ? computed[computed.length - 1] : null;
}

function formatKdj(values: { k?: number | null; d?: number | null; j?: number | null } | null | undefined): string {
  return values ? `${formatNumber(values.k)} / ${formatNumber(values.d)} / ${formatNumber(values.j)}` : "-- / -- / --";
}

function finiteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
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
      <header className="capital-evidence-header">
        <div className="capital-evidence-heading">
          <h3>资金证据</h3>
          {summary && <p>{summary}</p>}
        </div>
      </header>
      {sections.filter((section) => section.items?.length || section.summary).slice(0, 6).map((section) => (
        <article key={section.key} className="capital-section">
          <h4>{section.title}</h4>
          {section.summary && <p>{section.summary}</p>}
          <div className="evidence-list">
            {(section.items || []).slice(0, 4).map((item, index) => {
              const metrics = capitalEvidenceMetricEntries(item.metrics);
              return (
                <article key={`${item.title}-${index}`}>
                  <strong>{item.title || item.source || "--"}</strong>
                  <span className="evidence-source">{[item.source, item.date].filter(Boolean).join(" ")}</span>
                  {metrics.length ? (
                    <div className="detail-grid capital-evidence-metrics">
                      {metrics.map(([label, value]) => (
                        <div key={`${label}-${value}`}>
                          <span>{label}</span>
                          <strong>{value}</strong>
                        </div>
                      ))}
                    </div>
                  ) : null}
                  {item.note && <p>{item.note}</p>}
                </article>
              );
            })}
          </div>
        </article>
      ))}
      <CollapsibleNotes notes={notes} />
    </section>
  );
}

function signalStatusLabel(status?: string): string {
  const labels: Record<string, string> = {
    neutral: "中性",
    positive: "偏强",
    negative: "偏弱",
    bullish: "看多",
    bearish: "看空",
    buy_setup: "买点预备",
    exit: "离场",
    uptrend: "上升趋势",
    hold: "持有",
    watch: "观察",
    oversold: "超卖",
  };
  return labels[String(status || "neutral")] || String(status || "中性");
}

function signalStatusTone(status?: string): "positive" | "negative" | "warning" | "neutral" {
  switch (status) {
    case "positive":
    case "bullish":
    case "buy_setup":
    case "uptrend":
    case "hold":
      return "positive";
    case "negative":
    case "bearish":
    case "exit":
      return "negative";
    case "watch":
    case "oversold":
      return "warning";
    default:
      return "neutral";
  }
}

function capitalEvidenceMetricEntries(metrics?: Record<string, unknown>): Array<[string, string]> {
  const entries = Object.entries(metrics || {})
    .map(([label, value]) => [label, capitalEvidenceMetricValue(label, value)] as [string, string])
    .filter(([, value]) => value.trim().length > 0);
  const order = new Map<string, number>([
    ["状态", 0],
    ["查询窗口", 1],
    ["证据类型", 2],
    ["收盘价", 3],
    ["涨跌幅", 4],
    ["机构买入额", 5],
    ["机构卖出额", 6],
    ["机构净买额", 7],
    ["净买额占成交额比", 8],
    ["买方机构数", 9],
    ["卖方机构数", 10],
    ["上榜原因", 11],
    ["量价热度", 12],
    ["吸筹强度", 13],
    ["吸筹指标", 14],
    ["趋势热度", 15],
    ["异动热度", 16],
    ["人气热度", 17],
    ["证据分", 18],
    ["标题", 19],
    ["评论数", 20],
    ["阅读数", 21],
    ["多空标记", 22],
    ["尝试源", 23],
    ["股票", 24],
  ]);
  return entries.sort(([leftLabel], [rightLabel]) => {
    const leftOrder = order.get(leftLabel) ?? 999;
    const rightOrder = order.get(rightLabel) ?? 999;
    if (leftOrder !== rightOrder) return leftOrder - rightOrder;
    return leftLabel.localeCompare(rightLabel, "zh-Hans-CN");
  });
}

function capitalEvidenceMetricValue(label: string, value: unknown): string {
  const text = String(value ?? "").trim();
  if (!text) return "";
  if (label === "状态") return signalStatusLabel(text);
  return text;
}
