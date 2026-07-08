import { useEffect, useRef, useState } from "react";
import type { TrendIndicatorPoint } from "../../types";
import {
  aggregateBars,
  computeKdj,
  computeMacd,
  KLINE_PERIODS,
  movingAverage,
  toDailyBars,
  type KlineBar,
  type KlinePeriod,
} from "../../lib/kline";
import { formatNumber, formatPrice } from "../../lib/format";
import { isMobileTauriRuntime } from "../../lib/tauri";

const DEFAULT_VISIBLE_BARS = 96;
const MIN_VISIBLE_BARS = 30;
const MAX_VISIBLE_BARS = 180;

type SeriesPoint = { date: string; [key: string]: number | string };
type LayoutMode = "mobile" | "desktop";

type ChartLayout = {
  mode: LayoutMode;
  width: number;
  priceTop: number;
  priceHeight: number;
  volumeTop: number;
  volumeHeight: number;
  macdTop: number;
  macdHeight: number;
  rsiTop?: number;
  rsiHeight?: number;
  dmaTop?: number;
  dmaHeight?: number;
  mtmTop?: number;
  mtmHeight?: number;
  kdjTop?: number;
  kdjHeight?: number;
  height: number;
};

const MOBILE_LAYOUT: ChartLayout = {
  mode: "mobile",
  width: 720,
  priceTop: 22,
  priceHeight: 330,
  macdTop: 386,
  macdHeight: 94,
  rsiTop: 502,
  rsiHeight: 88,
  volumeTop: 612,
  volumeHeight: 106,
  height: 728,
};

const DESKTOP_LAYOUT: ChartLayout = {
  mode: "desktop",
  width: 1180,
  priceTop: 20,
  priceHeight: 260,
  volumeTop: 306,
  volumeHeight: 78,
  macdTop: 412,
  macdHeight: 74,
  dmaTop: 514,
  dmaHeight: 74,
  mtmTop: 616,
  mtmHeight: 74,
  kdjTop: 718,
  kdjHeight: 74,
  height: 802,
};

export function TrendCharts({ series }: { series: TrendIndicatorPoint[] }) {
  const [period, setPeriod] = useState<KlinePeriod>("daily");
  const [visibleCount, setVisibleCount] = useState(DEFAULT_VISIBLE_BARS);
  const [endIndex, setEndIndex] = useState<number | null>(null);
  const mobileRuntime = isMobileTauriRuntime();
  const dailyBars = toDailyBars(series);
  const bars = aggregateBars(dailyBars, period);

  useEffect(() => {
    setEndIndex(bars.length);
    setVisibleCount((count) =>
      Math.min(
        Math.max(count, MIN_VISIBLE_BARS),
        Math.max(MIN_VISIBLE_BARS, Math.min(MAX_VISIBLE_BARS, bars.length || DEFAULT_VISIBLE_BARS)),
      ),
    );
  }, [period, bars.length]);

  if (dailyBars.length < 2) {
    return (
      <div className="signal-chart-stack kline-workspace kline-market-workspace">
        <section className="chart-wrap kline-chart kline-market-chart">
          <header className="kline-market-header">
            <PeriodTabs period={period} onPeriodChange={setPeriod} />
          </header>
          <div className="kline-empty-state">暂无可用 K 线数据</div>
        </section>
      </div>
    );
  }

  const effectiveCount = Math.min(
    Math.max(visibleCount, MIN_VISIBLE_BARS),
    Math.max(MIN_VISIBLE_BARS, Math.min(MAX_VISIBLE_BARS, bars.length)),
  );
  const effectiveEnd = Math.min(Math.max(endIndex ?? bars.length, Math.min(effectiveCount, bars.length)), bars.length);
  const visibleStart = Math.max(0, effectiveEnd - effectiveCount);
  const visibleBars = bars.slice(visibleStart, effectiveEnd);

  const dragTo = (targetEnd: number) => {
    const minEnd = Math.min(effectiveCount, bars.length);
    setEndIndex(Math.max(minEnd, Math.min(bars.length, targetEnd)));
  };

  return (
    <div className={`signal-chart-stack kline-workspace kline-market-workspace ${mobileRuntime ? "mobile-board" : "desktop-board"}`}>
      <CandlestickChart
        period={period}
        onPeriodChange={setPeriod}
        allBars={bars}
        visibleBars={visibleBars}
        visibleStart={visibleStart}
        visibleEnd={effectiveEnd}
        onDragTo={dragTo}
        mobileRuntime={mobileRuntime}
      />
    </div>
  );
}

function PeriodTabs({ period, onPeriodChange }: { period: KlinePeriod; onPeriodChange: (period: KlinePeriod) => void }) {
  return (
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
  );
}

function CandlestickChart({
  period,
  onPeriodChange,
  allBars,
  visibleBars,
  visibleStart,
  visibleEnd,
  onDragTo,
  mobileRuntime,
}: {
  period: KlinePeriod;
  onPeriodChange: (period: KlinePeriod) => void;
  allBars: KlineBar[];
  visibleBars: KlineBar[];
  visibleStart: number;
  visibleEnd: number;
  onDragTo: (targetEnd: number) => void;
  mobileRuntime: boolean;
}) {
  const layout = mobileRuntime ? MOBILE_LAYOUT : DESKTOP_LAYOUT;
  const { width, priceTop, priceHeight, volumeTop, volumeHeight, macdTop, macdHeight, height } = layout;
  const plotInsetStart = mobileRuntime ? 8 : 52;
  const plotInsetEnd = mobileRuntime ? 8 : 48;

  const priceMax = Math.max(...visibleBars.map((bar) => bar.high));
  const priceMin = Math.min(...visibleBars.map((bar) => bar.low));
  const pricePadding = Math.max((priceMax - priceMin) * 0.08, 0.01);
  const priceUpper = priceMax + pricePadding;
  const priceLower = priceMin - pricePadding;
  const priceRange = priceUpper - priceLower || 1;
  const yPrice = (value: number) => priceTop + (1 - (value - priceLower) / priceRange) * priceHeight;

  const plotWidth = Math.max(width - plotInsetStart - plotInsetEnd, visibleBars.length);
  const slot = plotWidth / Math.max(visibleBars.length, 1);
  const dragStepPixels = Math.max(slot * (mobileRuntime ? 1.3 : 1), mobileRuntime ? 12 : 6);
  const dragDeadZonePixels = mobileRuntime ? 10 : 6;
  const bodyWidth = Math.max(1, Math.min(slot * 0.62, mobileRuntime ? 12 : 9));
  const center = (index: number) => plotInsetStart + (index + 0.5) * slot;
  const xByAbsoluteIndex = (index: number) => center(index - visibleStart);
  const [selectedDate, setSelectedDate] = useState<string | null>(null);

  const closes = allBars.map((bar) => bar.close);
  const maDefinitions = [
    { window: 5, label: "MA5", className: "ma-5" },
    { window: 10, label: "MA10", className: "ma-10" },
    { window: 20, label: "MA20", className: "ma-20" },
    { window: 30, label: "MA30", className: "ma-30" },
    { window: 60, label: "MA60", className: "ma-60" },
  ];
  const maLines = maDefinitions.map((definition) => {
    const values = movingAverage(closes, definition.window);
    return {
      ...definition,
      latest: values[visibleEnd - 1],
      points: values
        .map((value, index) => {
          if (value == null || index < visibleStart || index >= visibleEnd) return null;
          return `${xByAbsoluteIndex(index).toFixed(2)},${yPrice(value).toFixed(2)}`;
        })
        .filter(Boolean)
        .join(" "),
    };
  });

  const macd = computeMacd(allBars).slice(visibleStart, visibleEnd);
  const rsiSeries = computeRsiLines(allBars).slice(visibleStart, visibleEnd);
  const dmaSeries = computeDmaLines(allBars).slice(visibleStart, visibleEnd);
  const mtmSeries = computeMtmLines(allBars).slice(visibleStart, visibleEnd);
  const kdjSeries = computeKdj(allBars).slice(visibleStart, visibleEnd);
  const volumes = visibleBars.map((bar) => (Number.isFinite(bar.volume) ? bar.volume : 0));
  const volumeMax = Math.max(...volumes, 1);
  const selectedIndex = selectedDate ? visibleBars.findIndex((bar) => bar.date === selectedDate) : -1;
  const selectedBar = selectedIndex >= 0 ? visibleBars[selectedIndex] : null;
  const latest = visibleBars[visibleBars.length - 1];
  const inspectedBar = selectedBar || latest;
  const selectedX = selectedIndex >= 0 ? center(selectedIndex) : null;
  const selectedY = selectedBar ? yPrice(selectedBar.close) : null;
  const startDate = visibleBars[0]?.date || "";
  const endDate = visibleBars[visibleBars.length - 1]?.date || "";
  const visibleRange = `${startDate} - ${endDate}`;
  const priceTicks = buildTicks(priceLower, priceUpper, 5);

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
    const deltaBars = deltaX >= 0 ? Math.floor(deltaX / dragStepPixels) : Math.ceil(deltaX / dragStepPixels);
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
    if (drag?.pointerId !== event.pointerId) return;
    event.preventDefault();
    dragRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (commitSelection && !drag.moved) {
      const targetIndex = getBarIndexFromPointer(event);
      if (targetIndex !== null) setSelectedDate(visibleBars[targetIndex]?.date || null);
    }
  };

  const inspectorWidth = 168;
  const inspectorHeight = 96;
  const rawInspectorX = selectedX !== null && selectedX > width - inspectorWidth - 18 ? selectedX - inspectorWidth - 10 : (selectedX ?? 0) + 10;
  const inspectorX = Math.max(8, Math.min(width - inspectorWidth - 8, rawInspectorX));
  const inspectorY = selectedY !== null && selectedY > priceTop + inspectorHeight + 16 ? selectedY - inspectorHeight - 10 : priceTop + 12;

  return (
    <section className={`chart-wrap kline-chart kline-market-chart ${mobileRuntime ? "mobile-chart" : "desktop-chart"}`}>
      <header className="kline-market-header">
        <PeriodTabs period={period} onPeriodChange={onPeriodChange} />
        <div className="kline-desktop-tools" aria-hidden="true">
          <span>前复权</span><span>画线</span><span>工具</span>
        </div>
        <div className="kline-legend" aria-label="均线">
          <span>日线</span>
          {maLines.map((line) => (
            <span key={line.window} className={line.className}>{line.label}:{formatNumber(line.latest)}</span>
          ))}
        </div>
      </header>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={`${periodLabel(period)}K线图`}
        className="kline-plot kline-market-plot"
        style={{ touchAction: mobileRuntime ? "none" : "pan-y" }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={(event) => endDrag(event, false)}
        onPointerLeave={(event) => endDrag(event, false)}
      >
        <rect className="kline-plot-bg" x="0" y="0" width={width} height={height} />
        {priceTicks.map((tick) => {
          const y = yPrice(tick);
          return (
            <g key={`price-${tick}`}>
              <line className="kline-grid-line" x1={plotInsetStart} x2={width - plotInsetEnd} y1={y} y2={y} />
              <text className="kline-axis-label left" x="18" y={y + 4}>{formatPrice(tick)}</text>
              {!mobileRuntime ? <text className="kline-axis-label right" x={width - 18} y={y + 4}>{formatPrice(tick)}</text> : null}
            </g>
          );
        })}
        <line className="kline-current-line" x1={plotInsetStart} x2={width - plotInsetEnd} y1={yPrice(latest.close)} y2={yPrice(latest.close)} />
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
        {maLines.map((line) => (line.points ? <polyline key={`ma-${line.window}`} className={line.className} points={line.points} fill="none" /> : null))}
        {mobileRuntime ? (
          <>
            <PanelBorder width={width} y={macdTop - 12} />
            <MacdLayer series={macd} top={macdTop} height={macdHeight} width={width} center={center} barWidth={bodyWidth} />
            <PanelTitle x={8} y={macdTop + 16} label="MACD(12,26,9)" />
            <PanelBorder width={width} y={(layout.rsiTop || 0) - 12} />
            <RsiLayer series={rsiSeries} top={layout.rsiTop || 0} height={layout.rsiHeight || 0} width={width} center={center} />
            <PanelTitle x={8} y={(layout.rsiTop || 0) + 16} label="RSI(6,12,24)" />
            <PanelBorder width={width} y={volumeTop - 12} />
            <VolumeLayer bars={visibleBars} volumes={volumes} max={volumeMax} top={volumeTop} height={volumeHeight} center={center} barWidth={bodyWidth} />
            <PanelTitle x={8} y={volumeTop + 16} label="成交量" />
          </>
        ) : (
          <>
            <PanelBorder width={width} y={volumeTop - 12} />
            <VolumeLayer bars={visibleBars} volumes={volumes} max={volumeMax} top={volumeTop} height={volumeHeight} center={center} barWidth={bodyWidth} />
            <PanelTitle x={8} y={volumeTop + 14} label={`成交量  总量:${formatWan(volumeMax)}`} />
            <PanelBorder width={width} y={macdTop - 12} />
            <MacdLayer series={macd} top={macdTop} height={macdHeight} width={width} center={center} barWidth={bodyWidth} />
            <PanelTitle x={8} y={macdTop + 14} label="MACD  (12,26,9)" />
            <PanelBorder width={width} y={(layout.dmaTop || 0) - 12} />
            <MultiLineLayer series={dmaSeries} top={layout.dmaTop || 0} height={layout.dmaHeight || 0} width={width} center={center} keys={["ddd", "ama"]} classNames={["line-dma", "line-ama"]} includeZero />
            <PanelTitle x={8} y={(layout.dmaTop || 0) + 14} label="新DMA   AMA / DDD" />
            <PanelBorder width={width} y={(layout.mtmTop || 0) - 12} />
            <MultiLineLayer series={mtmSeries} top={layout.mtmTop || 0} height={layout.mtmHeight || 0} width={width} center={center} keys={["mtm", "mamtm"]} classNames={["line-mtm", "line-mamtm"]} includeZero />
            <PanelTitle x={8} y={(layout.mtmTop || 0) + 14} label="MTM   MTM / MAMTM" />
            <PanelBorder width={width} y={(layout.kdjTop || 0) - 12} />
            <MultiLineLayer series={kdjSeries} top={layout.kdjTop || 0} height={layout.kdjHeight || 0} width={width} center={center} keys={["k", "d", "j"]} classNames={["line-k", "line-d", "line-j"]} />
            <PanelTitle x={8} y={(layout.kdjTop || 0) + 14} label="KDJ   (9,3,3)" />
          </>
        )}
        {selectedBar && selectedX !== null && selectedY !== null && (
          <g className="kline-inspector" pointerEvents="none">
            <line className="kline-inspector-line" x1={selectedX} x2={selectedX} y1={priceTop} y2={height - 8} />
            <circle className="kline-inspector-dot" cx={selectedX} cy={selectedY} r="3.8" />
            <rect className="kline-inspector-card" x={inspectorX} y={inspectorY} width={inspectorWidth} height={inspectorHeight} rx="5" />
            <text className="kline-inspector-title" x={inspectorX + 10} y={inspectorY + 18}>{selectedBar.date}</text>
            <text className="kline-inspector-text" x={inspectorX + 10} y={inspectorY + 38}>
              <tspan x={inspectorX + 10}>开 {formatPrice(selectedBar.open)}</tspan>
              <tspan x={inspectorX + 10} dy="14">高 {formatPrice(selectedBar.high)}</tspan>
              <tspan x={inspectorX + 10} dy="14">低 {formatPrice(selectedBar.low)}</tspan>
              <tspan x={inspectorX + 10} dy="14">收 {formatPrice(selectedBar.close)}</tspan>
              <tspan x={inspectorX + 10} dy="14">量 {formatNumber(selectedBar.volume)}</tspan>
            </text>
          </g>
        )}
      </svg>
      <div className="chart-labels"><span>{visibleRange}</span><span>{periodLabel(period)} · {visibleBars.length}/{allBars.length}</span></div>
      <div className="kline-stats" aria-label={selectedBar ? "选中K线" : "当前周期最新K线"}>
        <span>{selectedBar ? "选中" : "最新"} {inspectedBar?.date || "--"}</span>
        <span>开 {formatPrice(inspectedBar?.open)}</span>
        <span>高 {formatPrice(inspectedBar?.high)}</span>
        <span>低 {formatPrice(inspectedBar?.low)}</span>
        <span>收 {formatPrice(inspectedBar?.close)}</span>
        <span>量 {formatNumber(inspectedBar?.volume)}</span>
      </div>
    </section>
  );
}

function PanelBorder({ width, y }: { width: number; y: number }) {
  return <line className="kline-section-border" x1="0" x2={width} y1={y} y2={y} />;
}

function PanelTitle({ x, y, label }: { x: number; y: number; label: string }) {
  return <text className="kline-subpanel-title" x={x} y={y}>{label}</text>;
}

function MacdLayer({
  series,
  top,
  height,
  width,
  center,
  barWidth,
}: {
  series: ReturnType<typeof computeMacd>;
  top: number;
  height: number;
  width: number;
  center: (index: number) => number;
  barWidth: number;
}) {
  if (series.length < 2) return null;
  const values = series.flatMap((point) => [point.dif, point.dea, point.macd]).filter(Number.isFinite);
  const maxAbs = Math.max(...values.map((value) => Math.abs(value)), 1);
  const midY = top + height / 2;
  const y = (value: number) => midY - (value / maxAbs) * (height * 0.46);
  const line = (key: "dif" | "dea") => series.map((point, index) => `${center(index).toFixed(2)},${y(point[key]).toFixed(2)}`).join(" ");

  return (
    <g className="kline-macd-layer">
      <line className="trend-grid-line macd-zero-line" x1="0" x2={width} y1={midY} y2={midY} />
      {series.map((point, index) => {
        const barTop = y(Math.max(point.macd, 0));
        const barBottom = y(Math.min(point.macd, 0));
        return (
          <rect
            key={`${point.date}-${index}`}
            className={`macd-bar ${point.macd >= 0 ? "macd-up" : "macd-down"}`}
            x={center(index) - barWidth / 2}
            y={Math.min(barTop, barBottom)}
            width={Math.max(1, Math.min(barWidth, 7))}
            height={Math.max(1, Math.abs(barBottom - barTop))}
            fill="currentColor"
          />
        );
      })}
      <polyline className="trend-line line-dif" points={line("dif")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <polyline className="trend-line line-dea" points={line("dea")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
    </g>
  );
}

function MultiLineLayer({
  series,
  top,
  height,
  width,
  center,
  keys,
  classNames,
  includeZero = false,
}: {
  series: SeriesPoint[];
  top: number;
  height: number;
  width: number;
  center: (index: number) => number;
  keys: string[];
  classNames: string[];
  includeZero?: boolean;
}) {
  if (series.length < 2) return null;
  const values = series.flatMap((point) => keys.map((key) => Number(point[key]))).filter(Number.isFinite);
  if (includeZero) values.push(0);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const y = (value: number) => top + (1 - (value - min) / range) * height;
  const zeroY = 0 >= min && 0 <= max ? y(0) : null;
  const line = (key: string) =>
    series
      .map((point, index) => {
        const value = Number(point[key]);
        if (!Number.isFinite(value)) return null;
        return `${center(index).toFixed(2)},${y(value).toFixed(2)}`;
      })
      .filter(Boolean)
      .join(" ");

  return (
    <g className="kline-line-panel">
      {zeroY !== null ? <line className="trend-grid-line" x1="0" x2={width} y1={zeroY} y2={zeroY} /> : null}
      {keys.map((key, index) => <polyline key={key} className={`trend-line ${classNames[index]}`} points={line(key)} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />)}
    </g>
  );
}

function RsiLayer({
  series,
  top,
  height,
  width,
  center,
}: {
  series: RsiPoint[];
  top: number;
  height: number;
  width: number;
  center: (index: number) => number;
}) {
  if (series.length < 2) return null;
  const y = (value: number) => top + (1 - Math.max(0, Math.min(100, value)) / 100) * height;
  const line = (key: keyof Pick<RsiPoint, "rsi6" | "rsi12" | "rsi24">) =>
    series
      .map((point, index) => {
        const value = point[key];
        if (!Number.isFinite(value)) return null;
        return `${center(index).toFixed(2)},${y(value).toFixed(2)}`;
      })
      .filter(Boolean)
      .join(" ");

  return (
    <g className="kline-rsi-layer">
      <line className="trend-grid-line" x1="0" x2={width} y1={y(70)} y2={y(70)} />
      <line className="trend-grid-line" x1="0" x2={width} y1={y(30)} y2={y(30)} />
      <polyline className="trend-line line-rsi6" points={line("rsi6")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <polyline className="trend-line line-rsi12" points={line("rsi12")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <polyline className="trend-line line-rsi24" points={line("rsi24")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
    </g>
  );
}

function VolumeLayer({
  bars,
  volumes,
  max,
  top,
  height,
  center,
  barWidth,
}: {
  bars: KlineBar[];
  volumes: number[];
  max: number;
  top: number;
  height: number;
  center: (index: number) => number;
  barWidth: number;
}) {
  return (
    <g className="kline-volume-layer">
      {bars.map((bar, index) => {
        const barHeight = (volumes[index] / max) * height;
        return (
          <rect
            key={`vol-${bar.date}-${index}`}
            className={bar.close >= bar.open ? "kline-up" : "kline-down"}
            x={center(index) - barWidth / 2}
            y={top + (height - barHeight)}
            width={barWidth}
            height={Math.max(0.5, barHeight)}
            fill="currentColor"
          />
        );
      })}
    </g>
  );
}

type RsiPoint = {
  date: string;
  rsi6: number;
  rsi12: number;
  rsi24: number;
};

function computeRsiLines(bars: KlineBar[]): RsiPoint[] {
  const closes = bars.map((bar) => bar.close);
  const rsi6 = computeRsi(closes, 6);
  const rsi12 = computeRsi(closes, 12);
  const rsi24 = computeRsi(closes, 24);
  return bars.map((bar, index) => ({ date: bar.date, rsi6: rsi6[index], rsi12: rsi12[index], rsi24: rsi24[index] }));
}

function computeRsi(values: number[], period: number): number[] {
  const result: number[] = [];
  let averageGain = 0;
  let averageLoss = 0;
  for (let index = 0; index < values.length; index += 1) {
    if (index === 0) {
      result.push(50);
      continue;
    }
    const change = values[index] - values[index - 1];
    const gain = Math.max(change, 0);
    const loss = Math.max(-change, 0);
    if (index <= period) {
      averageGain += gain;
      averageLoss += loss;
      const divisor = Math.max(index, 1);
      result.push(rsiValue(averageGain / divisor, averageLoss / divisor));
      continue;
    }
    averageGain = (averageGain * (period - 1) + gain) / period;
    averageLoss = (averageLoss * (period - 1) + loss) / period;
    result.push(rsiValue(averageGain, averageLoss));
  }
  return result;
}

function rsiValue(averageGain: number, averageLoss: number): number {
  if (averageLoss === 0) return averageGain === 0 ? 50 : 100;
  const relativeStrength = averageGain / averageLoss;
  return 100 - 100 / (1 + relativeStrength);
}

function computeDmaLines(bars: KlineBar[]): SeriesPoint[] {
  const closes = bars.map((bar) => bar.close);
  const ma10 = movingAverage(closes, 10);
  const ma50 = movingAverage(closes, 50);
  const ddd = closes.map((_, index) => valueOrNaN(ma10[index]) - valueOrNaN(ma50[index]));
  const ama = movingAverage(ddd.map((value) => (Number.isFinite(value) ? value : 0)), 10);
  return bars.map((bar, index) => ({ date: bar.date, ddd: ddd[index], ama: valueOrNaN(ama[index]) }));
}

function computeMtmLines(bars: KlineBar[]): SeriesPoint[] {
  const closes = bars.map((bar) => bar.close);
  const mtm = closes.map((close, index) => index >= 12 ? close - closes[index - 12] : NaN);
  const mamtm = movingAverage(mtm.map((value) => (Number.isFinite(value) ? value : 0)), 6);
  return bars.map((bar, index) => ({ date: bar.date, mtm: mtm[index], mamtm: valueOrNaN(mamtm[index]) }));
}

function valueOrNaN(value: number | null | undefined): number {
  return value == null ? NaN : value;
}

function buildTicks(min: number, max: number, count: number): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || count <= 1) return [];
  return Array.from({ length: count }, (_, index) => min + ((max - min) * index) / (count - 1)).reverse();
}

function formatWan(value: number): string {
  if (!Number.isFinite(value)) return "--";
  return `${formatNumber(value / 10_000)}万`;
}

function periodLabel(period: KlinePeriod): string {
  return KLINE_PERIODS.find((item) => item.key === period)?.label || period;
}