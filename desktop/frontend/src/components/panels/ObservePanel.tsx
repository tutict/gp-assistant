import { useCallback, useEffect, useRef, useState } from "react";
import type { CapitalEvidenceSection, FinancialIndicatorItem, ObserveResult, TrendIndicatorPoint, WatchlistItem } from "../../types";
import { aggregateBars, computeKdj, computeMacd, KLINE_PERIODS, type KlineBar, type KlinePeriod, type MacdPoint, movingAverage, toDailyBars } from "../../lib/kline";
import { fetchObserveDailyHistoryForTauri, getJson, isMobileTauriRuntime } from "../../lib/tauri";
import { CollapsibleNotes } from "../CollapsibleNotes";
import { StockCodeInput } from "../StockCodeInput";
import {
  currentSystemDateInputValue,
  escapeHtml,
  formatNumber,
  formatPercent,
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
const DEFAULT_VISIBLE_BARS = 120;
const MIN_VISIBLE_BARS = 30;
const MAX_VISIBLE_BARS = 240;

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
      setResult(await hydrateMobileObserveTrend(data, normalizedCode, startDate, endDate));
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
          {signal?.status && <span className={`state-pill ${signal.status}`}>{signal.status}</span>}
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
          <header><div><h3>趋势信号</h3><p>{signal.date || ""}</p></div><span className="state-pill">{signalStatusLabel(signal.status)}</span></header>
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

async function hydrateMobileObserveTrend(result: ObserveResult, code: string, startDate: string, endDate: string): Promise<ObserveResult> {
  if ((result.trend?.series?.length || 0) > 1 || !isMobileTauriRuntime()) return result;
  const history = await fetchObserveDailyHistoryForTauri({
    code,
    start_date: startDate.replace(/\D/g, ""),
    end_date: endDate.replace(/\D/g, ""),
  }, 9000).catch(() => null);
  const series = historyRowsToTrendSeries(history || []);
  if (series.length < 2) return result;

  const latest = series[series.length - 1];
  const stock = {
    ...(result.stock || { code, name: code }),
    code: result.stock?.code || code,
    name: result.stock?.name || code,
    price: result.stock?.price ?? latest.close,
  };

  return {
    ...result,
    stock,
    trend: {
      stock,
      signal: {
        code: stock.code,
        date: latest.date,
        close: latest.close,
        k: latest.k ?? null,
        d: latest.d ?? null,
        j: latest.j ?? null,
        status: "neutral",
      },
      series,
    },
  };
}

function historyRowsToTrendSeries(rows: Record<string, unknown>[]): TrendIndicatorPoint[] {
  const bars = rows.map((row) => {
    const close = finiteNumber(row.close);
    if (!row.date || close == null) return null;
    const open = finiteNumber(row.open) ?? close;
    const high = finiteNumber(row.high) ?? Math.max(open, close);
    const low = finiteNumber(row.low) ?? Math.min(open, close);
    return {
      date: String(row.date),
      open,
      close,
      high,
      low,
      volume: finiteNumber(row.volume) ?? 0,
    } satisfies KlineBar;
  }).filter(Boolean) as KlineBar[];

  bars.sort((left, right) => left.date.localeCompare(right.date));
  const kdjByDate = new Map(computeKdj(bars).map((point) => [point.date, point]));
  return bars.map((bar) => {
    const kdj = kdjByDate.get(bar.date);
    return {
      ...bar,
      k: kdj?.k ?? null,
      d: kdj?.d ?? null,
      j: kdj?.j ?? null,
    };
  });
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
      <header><h3>资金证据</h3>{summary && <p>{summary}</p>}</header>
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
      <CollapsibleNotes notes={notes} />
    </section>
  );
}

function TrendCharts({ series }: { series: TrendIndicatorPoint[] }) {
  const [period, setPeriod] = useState<KlinePeriod>("daily");
  const [visibleCount, setVisibleCount] = useState(DEFAULT_VISIBLE_BARS);
  const [endIndex, setEndIndex] = useState<number | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const dailyBars = toDailyBars(series);
  const bars = aggregateBars(dailyBars, period);

  useEffect(() => {
    setEndIndex(bars.length);
    setVisibleCount((count) => Math.min(Math.max(count, MIN_VISIBLE_BARS), Math.max(MIN_VISIBLE_BARS, Math.min(MAX_VISIBLE_BARS, bars.length || DEFAULT_VISIBLE_BARS))));
  }, [period, bars.length]);

  if (dailyBars.length < 2) {
    return (
      <div className="signal-chart-stack">
        <LineChart title="收盘 / SWL / SWS" series={series.slice(-DEFAULT_VISIBLE_BARS)} keys={["close", "swl", "sws"]} />
        <LineChart title="KDJ" series={series.slice(-DEFAULT_VISIBLE_BARS)} keys={["k", "d", "j"]} />
      </div>
    );
  }

  const effectiveCount = Math.min(Math.max(visibleCount, MIN_VISIBLE_BARS), Math.max(MIN_VISIBLE_BARS, Math.min(MAX_VISIBLE_BARS, bars.length)));
  const effectiveEnd = Math.min(Math.max(endIndex ?? bars.length, Math.min(effectiveCount, bars.length)), bars.length);
  const visibleStart = Math.max(0, effectiveEnd - effectiveCount);
  const visibleBars = bars.slice(visibleStart, effectiveEnd);
  const startDate = visibleBars[0]?.date || "";
  const endDate = visibleBars[visibleBars.length - 1]?.date || "";
  const visibleSeries = sliceSeriesByDate(series, startDate, endDate);
  const panStep = Math.max(1, Math.round(effectiveCount * 0.35));
  const canPanLeft = visibleStart > 0;
  const canPanRight = effectiveEnd < bars.length;
  const zoomIn = () => setVisibleCount((count) => Math.max(MIN_VISIBLE_BARS, Math.round(count * 0.75)));
  const zoomOut = () => setVisibleCount((count) => Math.min(Math.min(MAX_VISIBLE_BARS, bars.length), Math.round(count * 1.35)));
  const panLeft = () => setEndIndex(Math.max(Math.min(effectiveCount, bars.length), effectiveEnd - panStep));
  const panRight = () => setEndIndex(Math.min(bars.length, effectiveEnd + panStep));
  const showLatest = () => setEndIndex(bars.length);
  const dragTo = (targetEnd: number) => {
    const minEnd = Math.min(effectiveCount, bars.length);
    setEndIndex(Math.max(minEnd, Math.min(bars.length, targetEnd)));
  };

  return (
    <div className={`signal-chart-stack kline-workspace ${fullscreen ? "fullscreen" : ""}`}>
      <CandlestickChart
        period={period}
        onPeriodChange={setPeriod}
        allBars={bars}
        visibleBars={visibleBars}
        visibleStart={visibleStart}
        visibleEnd={effectiveEnd}
        onZoomIn={zoomIn}
        onZoomOut={zoomOut}
        onPanLeft={panLeft}
        onPanRight={panRight}
        onLatest={showLatest}
        onDragTo={dragTo}
        fullscreen={fullscreen}
        onToggleFullscreen={() => setFullscreen((value) => !value)}
        canZoomIn={effectiveCount > MIN_VISIBLE_BARS}
        canZoomOut={effectiveCount < Math.min(MAX_VISIBLE_BARS, bars.length)}
        canPanLeft={canPanLeft}
        canPanRight={canPanRight}
      />
      <LineChart title="收盘 / SWL / SWS" series={visibleSeries} keys={["close", "swl", "sws"]} />
      <LineChart title="KDJ" series={visibleSeries} keys={["k", "d", "j"]} />
      <MacdChart series={computeMacd(bars).slice(visibleStart, effectiveEnd)} />
    </div>
  );
}

function CandlestickChart({
  period,
  onPeriodChange,
  allBars,
  visibleBars,
  visibleStart,
  visibleEnd,
  onZoomIn,
  onZoomOut,
  onPanLeft,
  onPanRight,
  onLatest,
  onDragTo,
  fullscreen,
  onToggleFullscreen,
  canZoomIn,
  canZoomOut,
  canPanLeft,
  canPanRight,
}: {
  period: KlinePeriod;
  onPeriodChange: (period: KlinePeriod) => void;
  allBars: KlineBar[];
  visibleBars: KlineBar[];
  visibleStart: number;
  visibleEnd: number;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onPanLeft: () => void;
  onPanRight: () => void;
  onLatest: () => void;
  onDragTo: (targetEnd: number) => void;
  fullscreen: boolean;
  onToggleFullscreen: () => void;
  canZoomIn: boolean;
  canZoomOut: boolean;
  canPanLeft: boolean;
  canPanRight: boolean;
}) {
  const width = 720;
  const priceTop = 6;
  const priceHeight = 218;
  const volumeTop = priceTop + priceHeight + 24;
  const volumeHeight = 58;
  const height = volumeTop + volumeHeight + 4;

  const priceMax = Math.max(...visibleBars.map((bar) => bar.high));
  const priceMin = Math.min(...visibleBars.map((bar) => bar.low));
  const priceRange = priceMax - priceMin || 1;
  const yPrice = (value: number) => priceTop + (1 - (value - priceMin) / priceRange) * priceHeight;

  const volumes = visibleBars.map((bar) => (Number.isFinite(bar.volume) ? bar.volume : 0));
  const volumeMax = Math.max(...volumes, 1);
  const hasVolume = volumes.some((value) => value > 0);

  const slot = width / visibleBars.length;
  const bodyWidth = Math.max(1, Math.min(slot * 0.62, 14));
  const center = (index: number) => (index + 0.5) * slot;
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const getBarIndexFromPointer = (event: React.PointerEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const relativeX = ((event.clientX - bounds.left) / Math.max(bounds.width, 1)) * width;
    const index = Math.floor(relativeX / Math.max(slot, 1));
    if (!Number.isFinite(index)) return null;
    return Math.max(0, Math.min(visibleBars.length - 1, index));
  };
  const dragRef = useRef<{ pointerId: number; startX: number; startEnd: number; lastTargetEnd: number; moved: boolean } | null>(null);
  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    dragRef.current = { pointerId: event.pointerId, startX: event.clientX, startEnd: visibleEnd, lastTargetEnd: visibleEnd, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const onPointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const deltaBars = Math.round((drag.startX - event.clientX) / Math.max(slot, 1));
    const targetEnd = drag.startEnd + deltaBars;
    if (targetEnd !== drag.lastTargetEnd) {
      drag.moved = true;
      onDragTo(targetEnd);
      drag.lastTargetEnd = targetEnd;
    }
  };
  const endDrag = (event: React.PointerEvent<SVGSVGElement>, commitSelection = true) => {
    const drag = dragRef.current;
    if (drag?.pointerId === event.pointerId) {
      dragRef.current = null;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      if (commitSelection && !drag.moved) {
        const targetIndex = getBarIndexFromPointer(event);
        if (targetIndex !== null) setSelectedDate(visibleBars[targetIndex]?.date || null);
      }
    }
  };

  const closes = allBars.map((bar) => bar.close);
  const maLines = [5, 10, 20].map((window) => ({
    window,
    points: movingAverage(closes, window)
      .map((value, index) => {
        if (value == null) return null;
        if (index < visibleStart || index >= visibleEnd) return null;
        return `${center(index - visibleStart).toFixed(2)},${yPrice(value).toFixed(2)}`;
      })
      .filter(Boolean)
      .join(" "),
  }));
  const latest = visibleBars[visibleBars.length - 1];
  const kdjSeries = computeKdj(allBars);
  const latestKdj = kdjSeries[visibleEnd - 1];
  const selectedIndex = selectedDate ? visibleBars.findIndex((bar) => bar.date === selectedDate) : -1;
  const selectedBar = selectedIndex >= 0 ? visibleBars[selectedIndex] : null;
  const inspectedBar = selectedBar || latest;
  const inspectedAbsoluteIndex = selectedIndex >= 0 ? visibleStart + selectedIndex : visibleEnd - 1;
  const inspectedKdj = kdjSeries[inspectedAbsoluteIndex];
  const selectedX = selectedIndex >= 0 ? center(selectedIndex) : null;
  const selectedY = selectedBar ? yPrice(selectedBar.close) : null;
  const inspectorWidth = 170;
  const inspectorHeight = 98;
  const rawInspectorX = selectedX !== null && selectedX > width - inspectorWidth - 18 ? selectedX - inspectorWidth - 10 : (selectedX ?? 0) + 10;
  const inspectorX = Math.max(6, Math.min(width - inspectorWidth - 6, rawInspectorX));
  const inspectorY = selectedY !== null && selectedY > priceTop + inspectorHeight + 16 ? selectedY - inspectorHeight - 10 : priceTop + 10;
  const sourceRange = `${allBars[0]?.date || "--"} 至 ${allBars[allBars.length - 1]?.date || "--"}`;
  const visibleRange = `${visibleBars[0]?.date || "--"} 至 ${visibleBars[visibleBars.length - 1]?.date || "--"}`;

  return (
    <section className="chart-wrap kline-chart">
      <header>
        <div>
          <h4>K线 / 均线 / 成交量</h4>
          <p>{visibleRange}，{periodLabel(period)} {visibleBars.length} / {allBars.length} 根，历史 {sourceRange}</p>
        </div>
        <div className="kline-period-tabs" role="tablist" aria-label="K线周期">
          {KLINE_PERIODS.map((item) => (
            <button
              key={item.key}
              type="button"
              role="tab"
              aria-selected={period === item.key}
              className={period === item.key ? "active" : ""}
              onClick={() => onPeriodChange(item.key)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="kline-nav" aria-label="K线视图控制">
          <button type="button" onClick={onPanLeft} disabled={!canPanLeft} title="向左翻动">‹</button>
          <button type="button" onClick={onZoomIn} disabled={!canZoomIn} title="放大">＋</button>
          <button type="button" onClick={onZoomOut} disabled={!canZoomOut} title="缩小">－</button>
          <button type="button" onClick={onPanRight} disabled={!canPanRight} title="向右翻动">›</button>
          <button type="button" onClick={onLatest} disabled={!canPanRight} title="回到最新">最新</button>
          <button type="button" className="kline-fullscreen-toggle" onClick={onToggleFullscreen} title={fullscreen ? "退出全屏" : "全屏观察"}>
            {fullscreen ? "退出" : "全屏"}
          </button>
        </div>
        <div className="kline-legend"><span className="ma-5">MA5</span><span className="ma-10">MA10</span><span className="ma-20">MA20</span></div>
      </header>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="K线图"
        className="kline-plot"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={(event) => endDrag(event, false)}
        onPointerLeave={(event) => endDrag(event, false)}
      >
        {visibleBars.map((bar, index) => {
          const cx = center(index);
          const bodyTop = Math.min(yPrice(bar.open), yPrice(bar.close));
          const bodyHeight = Math.max(1, Math.abs(yPrice(bar.close) - yPrice(bar.open)));
          return (
            <g key={`${bar.date}-${index}`} className={`kline-candle ${bar.close >= bar.open ? "kline-up" : "kline-down"} ${selectedDate === bar.date ? "selected" : ""}`}>
              <line className="kline-wick" x1={cx} x2={cx} y1={yPrice(bar.high)} y2={yPrice(bar.low)} stroke="currentColor" />
              <rect className="kline-body" x={cx - bodyWidth / 2} y={bodyTop} width={bodyWidth} height={bodyHeight} fill="currentColor" />
            </g>
          );
        })}
        {maLines.map((line) => (line.points ? <polyline key={`ma-${line.window}`} className={`ma-${line.window}`} points={line.points} fill="none" /> : null))}
        {hasVolume && visibleBars.map((bar, index) => {
          const barHeight = (volumes[index] / volumeMax) * volumeHeight;
          return (
            <rect
              key={`vol-${bar.date}-${index}`}
              className={bar.close >= bar.open ? "kline-up" : "kline-down"}
              x={center(index) - bodyWidth / 2}
              y={volumeTop + (volumeHeight - barHeight)}
              width={bodyWidth}
              height={Math.max(0.5, barHeight)}
              fill="currentColor"
            />
          );
        })}
        {selectedBar && selectedX !== null && selectedY !== null && (
          <g className="kline-inspector" pointerEvents="none">
            <line className="kline-inspector-line" x1={selectedX} x2={selectedX} y1={priceTop} y2={volumeTop + volumeHeight} />
            <circle className="kline-inspector-dot" cx={selectedX} cy={selectedY} r="3.8" />
            <rect className="kline-inspector-card" x={inspectorX} y={inspectorY} width={inspectorWidth} height={inspectorHeight} rx="7" />
            <text className="kline-inspector-title" x={inspectorX + 10} y={inspectorY + 18}>{selectedBar.date}</text>
            <text className="kline-inspector-text" x={inspectorX + 10} y={inspectorY + 38}>
              <tspan x={inspectorX + 10}>开盘价 {formatPrice(selectedBar.open)}</tspan>
              <tspan x={inspectorX + 10} dy="14">最高价 {formatPrice(selectedBar.high)}</tspan>
              <tspan x={inspectorX + 10} dy="14">最低价 {formatPrice(selectedBar.low)}</tspan>
              <tspan x={inspectorX + 10} dy="14">收盘价 {formatPrice(selectedBar.close)}</tspan>
              <tspan x={inspectorX + 10} dy="14">成交量 {formatNumber(selectedBar.volume)}</tspan>
            </text>
          </g>
        )}
      </svg>
      <div className="chart-labels"><span>{visibleBars[0]?.date}</span><span>{visibleBars[visibleBars.length - 1]?.date}</span></div>
      <div className="kline-stats" aria-label={selectedBar ? "选中K线" : "当前周期最新K线"}>
        <span>{selectedBar ? "选中" : "最新"} {inspectedBar?.date || "--"}</span>
        <span>开盘价 {formatPrice(inspectedBar?.open)}</span>
        <span>最高价 {formatPrice(inspectedBar?.high)}</span>
        <span>最低价 {formatPrice(inspectedBar?.low)}</span>
        <span>收盘价 {formatPrice(inspectedBar?.close)}</span>
        <span>成交量 {formatNumber(inspectedBar?.volume)}</span>
        {inspectedKdj ? <span>KDJ {formatNumber(inspectedKdj.k)} / {formatNumber(inspectedKdj.d)} / {formatNumber(inspectedKdj.j)}</span> : null}
      </div>
    </section>
  );
}
function periodLabel(period: KlinePeriod): string {
  return KLINE_PERIODS.find((item) => item.key === period)?.label || period;
}

function signalStatusLabel(status?: string): string {
  const labels: Record<string, string> = {
    neutral: "中性",
    positive: "偏强",
    negative: "偏弱",
    bullish: "看多",
    bearish: "看空",
  };
  return labels[String(status || "neutral")] || String(status || "中性");
}

function LineChart({ title, series, keys }: { title: string; series: Record<string, unknown>[]; keys: string[] }) {
  const width = 720;
  const height = 160;
  const gridLines = [0.25, 0.5, 0.75].map((ratio) => ratio * height);
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
      <header>
        <h4>{title}</h4>
        <div className="trend-legend">
          {keys.map((key) => <span key={key} className={`line-${key}`}>{lineLabel(key)}</span>)}
        </div>
      </header>
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={title}>
        {gridLines.map((y) => <line key={y} className="trend-grid-line" x1="0" x2={width} y1={y} y2={y} />)}
        {keys.map((key) => <polyline key={key} className={`trend-line line-${key}`} points={line(key)} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />)}
      </svg>
      <div className="chart-labels"><span>{String(series[0]?.date || "")}</span><span>{String(series[series.length - 1]?.date || "")}</span></div>
    </section>
  );
}

function MacdChart({ series }: { series: MacdPoint[] }) {
  const width = 720;
  const height = 150;
  if (series.length < 2) return null;
  const values = series.map((point) => [point.dif, point.dea, point.macd]).flat().filter(Number.isFinite);
  if (values.length < 2) return null;
  const absValues = values.map((value) => Math.abs(value)).filter((value) => value > 0);
  const maxAbs = absValues.length > 0 ? Math.max(...absValues) : 1;
  const midY = height / 2;
  const x = (index: number) => (index / Math.max(series.length - 1, 1)) * width;
  const y = (value: number) => midY - (value / maxAbs) * (height * 0.46);
  const barWidth = Math.max(2, Math.min((width / series.length) * 0.78, 9));
  const line = (key: "dif" | "dea") => series.map((point, index) => {
    const value = point[key];
    if (!Number.isFinite(value)) return null;
    return `${x(index).toFixed(2)},${y(value).toFixed(2)}`;
  }).filter(Boolean).join(" ");
  const latest = series[series.length - 1];

  return (
    <section className="chart-wrap macd-chart">
      <header>
        <h4>MACD</h4>
        <div className="trend-legend">
          <span className="line-dif">DIF</span>
          <span className="line-dea">DEA</span>
          <span className="macd-up">红柱</span>
          <span className="macd-down">绿柱</span>
        </div>
      </header>
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="MACD指标">
        <line className="trend-grid-line macd-zero-line" x1="0" x2={width} y1={midY} y2={midY} />
        {series.map((point, index) => {
          const barTop = y(Math.max(point.macd, 0));
          const barBottom = y(Math.min(point.macd, 0));
          const barHeight = Math.abs(barBottom - barTop);
          return (
            <rect
              key={`${point.date}-${index}`}
              className={`macd-bar ${point.macd >= 0 ? "macd-up" : "macd-down"}`}
              x={x(index) - barWidth / 2}
              y={Math.min(barTop, barBottom)}
              width={barWidth}
              height={Math.max(point.macd === 0 ? 1 : 2, barHeight)}
              fill="currentColor"
            />
          );
        })}
        <polyline className="trend-line macd-line-shadow line-dif" points={line("dif")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
        <polyline className="trend-line macd-line-shadow line-dea" points={line("dea")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
        <polyline className="trend-line macd-line line-dif" points={line("dif")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
        <polyline className="trend-line macd-line line-dea" points={line("dea")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <div className="chart-labels"><span>{String(series[0]?.date || "")}</span><span>{String(series[series.length - 1]?.date || "")}</span></div>
      <div className="kline-stats" aria-label="当前MACD">
        <span>DIF {formatNumber(latest?.dif)}</span>
        <span>DEA {formatNumber(latest?.dea)}</span>
        <span>MACD {formatNumber(latest?.macd)}</span>
      </div>
    </section>
  );
}

function lineLabel(key: string): string {
  const labels: Record<string, string> = {
    close: "收盘",
    swl: "SWL",
    sws: "SWS",
    k: "K",
    d: "D",
    j: "J",
    dif: "DIF",
    dea: "DEA",
  };
  return labels[key] || key.toUpperCase();
}

function sliceSeriesByDate(series: TrendIndicatorPoint[], startDate: string, endDate: string): TrendIndicatorPoint[] {
  if (!startDate || !endDate) return series.slice(-DEFAULT_VISIBLE_BARS);
  const sliced = series.filter((point) => point.date >= startDate && point.date <= endDate);
  return sliced.length ? sliced : series.slice(-DEFAULT_VISIBLE_BARS);
}
