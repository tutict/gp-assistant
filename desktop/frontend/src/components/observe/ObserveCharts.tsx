import { useCallback, useEffect, useRef, useState } from "react";
import type { TrendIndicatorPoint } from "../../types";
import { aggregateBars, computeKdj, computeMacd, KLINE_PERIODS, type KlineBar, type KlinePeriod, type MacdPoint, movingAverage, toDailyBars } from "../../lib/kline";
import { formatNumber, formatPrice } from "../../lib/format";
import { isMobileTauriRuntime } from "../../lib/tauri";

const DEFAULT_VISIBLE_BARS = 120;
const MIN_VISIBLE_BARS = 30;
const MAX_VISIBLE_BARS = 240;
const KLINE_MOBILE_FULLSCREEN_CLASS = "kline-mobile-fullscreen";

type LockableScreenOrientation = ScreenOrientation & {
  unlock?: () => void;
};
export function TrendCharts({ series }: { series: TrendIndicatorPoint[] }) {
  const [period, setPeriod] = useState<KlinePeriod>("daily");
  const [visibleCount, setVisibleCount] = useState(DEFAULT_VISIBLE_BARS);
  const [endIndex, setEndIndex] = useState<number | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const mobileRuntime = isMobileTauriRuntime();
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
  const canZoomIn = effectiveCount > MIN_VISIBLE_BARS;
  const canZoomOut = effectiveCount < Math.min(MAX_VISIBLE_BARS, bars.length);
  const zoomIn = () => setVisibleCount((count) => Math.max(MIN_VISIBLE_BARS, Math.round(count * 0.75)));
  const zoomOut = () => setVisibleCount((count) => Math.min(Math.min(MAX_VISIBLE_BARS, bars.length), Math.round(count * 1.35)));
  const panLeft = () => setEndIndex(Math.max(Math.min(effectiveCount, bars.length), effectiveEnd - panStep));
  const panRight = () => setEndIndex(Math.min(bars.length, effectiveEnd + panStep));
  const showLatest = () => setEndIndex(bars.length);
  const dragTo = (targetEnd: number) => {
    const minEnd = Math.min(effectiveCount, bars.length);
    setEndIndex(Math.max(minEnd, Math.min(bars.length, targetEnd)));
  };
  const toggleFullscreen = useCallback(() => {
    const next = !fullscreen;
    setFullscreen(next);
    if (!mobileRuntime) return;
    if (next) {
      void enterKlineMobileFullscreen();
      return;
    }
    void exitKlineMobileFullscreen();
  }, [fullscreen, mobileRuntime]);

  useEffect(() => {
    if (!mobileRuntime) return;
    const handleFullscreenChange = () => {
      if (!document.fullscreenElement && fullscreen) {
        document.documentElement.classList.remove(KLINE_MOBILE_FULLSCREEN_CLASS);
        const orientation = window.screen?.orientation as LockableScreenOrientation | undefined;
        try {
          orientation?.unlock?.();
        } catch {
          // Ignore orientation unlock failures in WebView runtimes.
        }
        setFullscreen(false);
      }
    };
    document.addEventListener("fullscreenchange", handleFullscreenChange);
    return () => document.removeEventListener("fullscreenchange", handleFullscreenChange);
  }, [fullscreen, mobileRuntime]);

  useEffect(() => () => {
    if (!mobileRuntime) return;
    void exitKlineMobileFullscreen();
  }, [mobileRuntime]);

  return (
    <>
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
        onToggleFullscreen={toggleFullscreen}
        canZoomIn={canZoomIn}
        canZoomOut={canZoomOut}
        canPanLeft={canPanLeft}
        canPanRight={canPanRight}
        mobileRuntime={mobileRuntime}
      />
      <LineChart title="收盘 / SWL / SWS" series={visibleSeries} keys={["close", "swl", "sws"]} />
      <LineChart title="KDJ" series={visibleSeries} keys={["k", "d", "j"]} />
      <MacdChart series={computeMacd(bars).slice(visibleStart, effectiveEnd)} />
      </div>
      {fullscreen && mobileRuntime ? (
        <div className="kline-mobile-fullscreen-toolbar" role="toolbar" aria-label="Kline fullscreen tools">
          <button
            type="button"
            className="kline-mobile-fullscreen-tool kline-mobile-fullscreen-zoom-out"
            onClick={zoomOut}
            disabled={!canZoomOut}
            aria-label="Zoom out chart"
            title="Zoom out chart"
          >
            -
          </button>
          <button
            type="button"
            className="kline-mobile-fullscreen-tool kline-mobile-fullscreen-zoom-in"
            onClick={zoomIn}
            disabled={!canZoomIn}
            aria-label="Zoom in chart"
            title="Zoom in chart"
          >
            +
          </button>
        <button
          type="button"
          className="kline-mobile-fullscreen-tool kline-mobile-fullscreen-exit"
          onClick={toggleFullscreen}
          aria-label="退出全屏"
          title="退出全屏"
        >
          退出
        </button>
        </div>
      ) : null}
    </>
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
  mobileRuntime,
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
  mobileRuntime: boolean;
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

  const plotInsetStart = mobileRuntime ? 16 : 8;
  const plotInsetEnd = mobileRuntime ? 40 : 12;
  const plotWidth = Math.max(width - plotInsetStart - plotInsetEnd, visibleBars.length);
  const slot = plotWidth / visibleBars.length;
  const dragStepPixels = Math.max(slot * (mobileRuntime ? 1.4 : 1), mobileRuntime ? 12 : 6);
  const dragDeadZonePixels = mobileRuntime ? 10 : 6;
  const bodyWidth = Math.max(1, Math.min(slot * 0.62, 14));
  const center = (index: number) => plotInsetStart + (index + 0.5) * slot;
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const getBarIndexFromPointer = (event: React.PointerEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const relativeX = ((event.clientX - bounds.left) / Math.max(bounds.width, 1)) * width;
    const index = Math.floor((relativeX - plotInsetStart) / Math.max(slot, 1));
    if (!Number.isFinite(index)) return null;
    return Math.max(0, Math.min(visibleBars.length - 1, index));
  };
  const dragRef = useRef<{ pointerId: number; startX: number; startEnd: number; lastTargetEnd: number; moved: boolean } | null>(null);
  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    event.preventDefault();
    dragRef.current = { pointerId: event.pointerId, startX: event.clientX, startEnd: visibleEnd, lastTargetEnd: visibleEnd, moved: false };
    setSelectedDate(null);
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const onPointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const deltaX = drag.startX - event.clientX;
    if (Math.abs(deltaX) < dragDeadZonePixels) return;
    event.preventDefault();
    const deltaBars = deltaX >= 0
      ? Math.floor(deltaX / dragStepPixels)
      : Math.ceil(deltaX / dragStepPixels);
    if (deltaBars === 0) return;
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
      event.preventDefault();
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
      {fullscreen ? (
        <button
          type="button"
          className="kline-fullscreen-exit"
          onClick={onToggleFullscreen}
          aria-label="退出全屏"
          title="退出全屏"
        >
          退出
        </button>
      ) : null}
      <svg
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="K线图"
        className="kline-plot"
        style={{ touchAction: mobileRuntime ? "none" : "pan-y" }}
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

async function enterKlineMobileFullscreen(): Promise<void> {
  const root = document.documentElement;
  root.classList.add(KLINE_MOBILE_FULLSCREEN_CLASS);

  try {
    if (!document.fullscreenElement && document.documentElement.requestFullscreen) {
      await document.documentElement.requestFullscreen();
    }
  } catch {
    // Fullscreen API is best-effort in Android WebView/Tauri.
  }

  const orientation = window.screen?.orientation as LockableScreenOrientation | undefined;
  try {
    await orientation?.lock?.("landscape");
  } catch {
    // Orientation lock is also best-effort; keep the fullscreen shell either way.
  }
}

async function exitKlineMobileFullscreen(): Promise<void> {
  document.documentElement.classList.remove(KLINE_MOBILE_FULLSCREEN_CLASS);

  const orientation = window.screen?.orientation as LockableScreenOrientation | undefined;
  try {
    orientation?.unlock?.();
  } catch {
    // Ignore unlock failures; the OS may keep the current orientation.
  }

  try {
    if (document.fullscreenElement && document.exitFullscreen) {
      await document.exitFullscreen();
    }
  } catch {
    // Ignore document fullscreen exit failures in constrained runtimes.
  }
}
