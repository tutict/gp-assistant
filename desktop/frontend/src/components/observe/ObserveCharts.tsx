import { useEffect, useRef, useState } from "react";
import { LocateFixed, Maximize2, Minimize2, ZoomIn, ZoomOut } from "lucide-react";
import type { TrendIndicatorPoint } from "../../types";
import {
  aggregateBars,
  buildMacdScale,
  computeKdj,
  computeMacd,
  KLINE_PERIODS,
  marketDirection,
  movingAverage,
  toDailyBars,
  type KlineBar,
  type KlinePeriod,
} from "../../lib/kline";
import { formatNumber, formatPrice } from "../../lib/format";
import { isMobileTauriRuntime } from "../../lib/tauri";

import { exitChartFullscreen, requestChartFullscreen, unlockChartOrientation } from "../../lib/chartFullscreen";
import {
  chartVisibleBounds,
  clampChartVisibleCount,
  zoomChartVisibleCount,
  type ChartZoomDirection,
} from "../../lib/chartViewport";

const DEFAULT_VISIBLE_BARS = 96;
const MOBILE_DEFAULT_VISIBLE_BARS = 72;

type SeriesPoint = { date: string; [key: string]: number | string };
type LayoutMode = "mobile" | "desktop";

const klineDirectionClass = (value: number, reference: number | null | undefined) =>
  `kline-${marketDirection(value, reference)}`;
const macdDirectionClass = (value: number) => `macd-${marketDirection(value, 0)}`;

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
  priceTop: 18,
  priceHeight: 300,
  macdTop: 352,
  macdHeight: 96,
  rsiTop: 468,
  rsiHeight: 90,
  volumeTop: 580,
  volumeHeight: 118,
  height: 708,
};

const MOBILE_LANDSCAPE_LAYOUT: ChartLayout = {
  mode: "mobile",
  width: 1180,
  priceTop: 10,
  priceHeight: 160,
  macdTop: 198,
  macdHeight: 62,
  rsiTop: 282,
  rsiHeight: 58,
  volumeTop: 358,
  volumeHeight: 52,
  height: 420,
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
  const mobileRuntime = isMobileTauriRuntime();
  const workspaceRef = useRef<HTMLDivElement>(null);
  const nativeFullscreenRef = useRef(false);
  const [period, setPeriod] = useState<KlinePeriod>("daily");
  const [visibleCount, setVisibleCount] = useState(() => (mobileRuntime ? MOBILE_DEFAULT_VISIBLE_BARS : DEFAULT_VISIBLE_BARS));
  const [endIndex, setEndIndex] = useState<number | null>(null);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const dailyBars = toDailyBars(series);
  const bars = aggregateBars(dailyBars, period);

  useEffect(() => {
    setEndIndex(bars.length);
    setVisibleCount((count) => clampChartVisibleCount(count, bars.length));
  }, [period, bars.length]);

  useEffect(() => {
    const handleFullscreenChange = () => {
      const ownsFullscreen = document.fullscreenElement === workspaceRef.current;
      if (ownsFullscreen) {
        nativeFullscreenRef.current = true;
        setIsFullscreen(true);
      } else if (nativeFullscreenRef.current) {
        nativeFullscreenRef.current = false;
        setIsFullscreen(false);
        unlockChartOrientation();
      }
    };
    document.addEventListener("fullscreenchange", handleFullscreenChange);
    return () => {
      document.removeEventListener("fullscreenchange", handleFullscreenChange);
      document.documentElement.classList.remove("kline-mobile-fullscreen");
      if (nativeFullscreenRef.current) void exitChartFullscreen(workspaceRef.current);
    };
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("kline-mobile-fullscreen", isFullscreen);
    if (!isFullscreen) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !document.fullscreenElement) {
        setIsFullscreen(false);
        unlockChartOrientation();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isFullscreen]);

  const enterFullscreen = async () => {
    const workspace = workspaceRef.current;
    if (!workspace) return;
    setIsFullscreen(true);
    const fullscreenResult = await requestChartFullscreen(workspace, { lockLandscape: mobileRuntime });
    const needsRotationFallback = mobileRuntime
      && fullscreenResult.nativeFullscreen
      && !fullscreenResult.orientationLocked
      && window.innerWidth < window.innerHeight;
    if (needsRotationFallback) {
      nativeFullscreenRef.current = false;
      await exitChartFullscreen(workspace);
      setIsFullscreen(true);
      return;
    }
    nativeFullscreenRef.current = fullscreenResult.nativeFullscreen;
  };

  const leaveFullscreen = async () => {
    await exitChartFullscreen(workspaceRef.current);
    nativeFullscreenRef.current = false;
    setIsFullscreen(false);
  };

  const toggleFullscreen = () => {
    void (isFullscreen ? leaveFullscreen() : enterFullscreen());
  };

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

  const effectiveCount = clampChartVisibleCount(visibleCount, bars.length);
  const visibleBounds = chartVisibleBounds(bars.length);
  const effectiveEnd = Math.min(Math.max(endIndex ?? bars.length, Math.min(effectiveCount, bars.length)), bars.length);
  const visibleStart = Math.max(0, effectiveEnd - effectiveCount);
  const visibleBars = bars.slice(visibleStart, effectiveEnd);

  const dragTo = (targetEnd: number) => {
    const minEnd = Math.min(effectiveCount, bars.length);
    setEndIndex(Math.max(minEnd, Math.min(bars.length, targetEnd)));
  };

  const changeVisibleCount = (count: number) => {
    setVisibleCount(clampChartVisibleCount(count, bars.length));
  };

  const zoom = (direction: ChartZoomDirection) => {
    changeVisibleCount(zoomChartVisibleCount(effectiveCount, direction, bars.length));
  };

  return (
    <div
      ref={workspaceRef}
      className={`signal-chart-stack kline-workspace kline-market-workspace ${mobileRuntime ? "mobile-board" : "desktop-board"} ${isFullscreen ? "fullscreen" : ""}`}
      data-fullscreen={isFullscreen ? "true" : "false"}
    >
      <CandlestickChart
        period={period}
        onPeriodChange={setPeriod}
        allBars={bars}
        visibleBars={visibleBars}
        visibleStart={visibleStart}
        visibleEnd={effectiveEnd}
        onDragTo={dragTo}
        mobileRuntime={mobileRuntime}
        isFullscreen={isFullscreen}
        onToggleFullscreen={toggleFullscreen}
        onZoom={zoom}
        onVisibleCountChange={changeVisibleCount}
        onReturnLatest={() => setEndIndex(bars.length)}
        canZoomIn={effectiveCount > visibleBounds.minimum}
        canZoomOut={effectiveCount < visibleBounds.maximum}
        isAtLatest={effectiveEnd >= bars.length}
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
  isFullscreen,
  onToggleFullscreen,
  onZoom,
  onVisibleCountChange,
  onReturnLatest,
  canZoomIn,
  canZoomOut,
  isAtLatest,
}: {
  period: KlinePeriod;
  onPeriodChange: (period: KlinePeriod) => void;
  allBars: KlineBar[];
  visibleBars: KlineBar[];
  visibleStart: number;
  visibleEnd: number;
  onDragTo: (targetEnd: number) => void;
  mobileRuntime: boolean;
  isFullscreen: boolean;
  onToggleFullscreen: () => void;
  onZoom: (direction: ChartZoomDirection) => void;
  onVisibleCountChange: (count: number) => void;
  onReturnLatest: () => void;
  canZoomIn: boolean;
  canZoomOut: boolean;
  isAtLatest: boolean;
}) {
  const layout = mobileRuntime ? (isFullscreen ? MOBILE_LANDSCAPE_LAYOUT : MOBILE_LAYOUT) : DESKTOP_LAYOUT;
  const { width, priceTop, priceHeight, volumeTop, volumeHeight, macdTop, macdHeight, height } = layout;
  const plotInsetStart = mobileRuntime ? 8 : 52;
  const currentPriceLabelWidth = mobileRuntime ? 68 : 76;
  const plotInsetEnd = (mobileRuntime ? 8 : 48) + currentPriceLabelWidth;

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
  const dmaSeries = computeDmaLines(allBars).slice(visibleStart, visibleEnd);
  const mtmSeries = computeMtmLines(allBars).slice(visibleStart, visibleEnd);
  const kdjSeries = computeKdj(allBars).slice(visibleStart, visibleEnd);
  const volumes = visibleBars.map((bar) => (Number.isFinite(bar.volume) ? bar.volume : 0));
  const volumeMax = Math.max(...volumes, 1);
  const selectedIndex = selectedDate ? visibleBars.findIndex((bar) => bar.date === selectedDate) : -1;
  const selectedBar = selectedIndex >= 0 ? visibleBars[selectedIndex] : null;
  const latest = visibleBars[visibleBars.length - 1];
  const inspectedBar = selectedBar || latest;
  const inspectedAbsoluteIndex = selectedIndex >= 0 ? visibleStart + selectedIndex : visibleEnd - 1;
  const inspectedPrevious = allBars[inspectedAbsoluteIndex - 1];
  const inspectedChangePercent = inspectedPrevious?.close
    ? ((inspectedBar.close - inspectedPrevious.close) / inspectedPrevious.close) * 100
    : null;
  const latestPrevious = allBars[visibleEnd - 2];
  const latestDirectionClass = klineDirectionClass(latest.close, latestPrevious?.close);
  const selectedX = selectedIndex >= 0 ? center(selectedIndex) : null;
  const selectedY = selectedBar ? yPrice(selectedBar.close) : null;
  const startDate = visibleBars[0]?.date || "";
  const endDate = visibleBars[visibleBars.length - 1]?.date || "";
  const visibleRange = `${startDate} - ${endDate}`;
  const priceTicks = buildTicks(priceLower, priceUpper, 5);
  const currentPriceY = yPrice(latest.close);
  const currentPriceLabelX = width - plotInsetEnd;
  const currentPriceLabelY = Math.max(priceTop, Math.min(priceTop + priceHeight - 24, currentPriceY - 12));

  const getBarIndexFromPointer = (event: React.PointerEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const relativeX = ((event.clientX - bounds.left) / Math.max(bounds.width, 1)) * width;
    const index = Math.floor((relativeX - plotInsetStart) / Math.max(slot, 1));
    if (!Number.isFinite(index)) return null;
    return Math.max(0, Math.min(visibleBars.length - 1, index));
  };

  const dragRef = useRef<{ pointerId: number; startX: number; startEnd: number; lastTargetEnd: number; moved: boolean } | null>(null);
  const pointersRef = useRef(new Map<number, { x: number; y: number }>());
  const pinchRef = useRef<{ startDistance: number; startCount: number; lastCount: number } | null>(null);
  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    event.preventDefault();
    pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    setSelectedDate(null);
    event.currentTarget.setPointerCapture(event.pointerId);
    if (pointersRef.current.size >= 2) {
      const [first, second] = Array.from(pointersRef.current.values());
      pinchRef.current = {
        startDistance: Math.max(1, Math.hypot(first.x - second.x, first.y - second.y)),
        startCount: visibleBars.length,
        lastCount: visibleBars.length,
      };
      dragRef.current = null;
      return;
    }
    dragRef.current = { pointerId: event.pointerId, startX: event.clientX, startEnd: visibleEnd, lastTargetEnd: visibleEnd, moved: false };
  };
  const onPointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    if (pointersRef.current.has(event.pointerId)) {
      pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    }
    const pinch = pinchRef.current;
    if (pinch && pointersRef.current.size >= 2) {
      const [first, second] = Array.from(pointersRef.current.values());
      const distance = Math.max(1, Math.hypot(first.x - second.x, first.y - second.y));
      const targetCount = Math.round(pinch.startCount * (pinch.startDistance / distance));
      if (targetCount !== pinch.lastCount) {
        event.preventDefault();
        onVisibleCountChange(targetCount);
        pinch.lastCount = targetCount;
      }
      return;
    }
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
  const endGesture = (event: React.PointerEvent<SVGSVGElement>, commitSelection = true) => {
    const wasPinching = pinchRef.current !== null || pointersRef.current.size > 1;
    pointersRef.current.delete(event.pointerId);
    const drag = dragRef.current;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (wasPinching) {
      event.preventDefault();
      pinchRef.current = null;
      dragRef.current = null;
      return;
    }
    if (drag?.pointerId !== event.pointerId) return;
    event.preventDefault();
    dragRef.current = null;
    if (commitSelection && !drag.moved) {
      const targetIndex = getBarIndexFromPointer(event);
      if (targetIndex !== null) setSelectedDate(visibleBars[targetIndex]?.date || null);
    }
  };

  const inspectorWidth = mobileRuntime ? 142 : 168;
  const inspectorHeight = mobileRuntime ? 88 : 96;
  const mobileInspectorX = selectedX !== null && selectedX > width * 0.55 ? 10 : width - inspectorWidth - 10;
  const rawInspectorX = mobileRuntime
    ? mobileInspectorX
    : selectedX !== null && selectedX > width - inspectorWidth - 18
      ? selectedX - inspectorWidth - 10
      : (selectedX ?? 0) + 10;
  const inspectorX = Math.max(8, Math.min(width - inspectorWidth - 8, rawInspectorX));
  const inspectorY = mobileRuntime
    ? priceTop + 8
    : selectedY !== null && selectedY > priceTop + inspectorHeight + 16
      ? selectedY - inspectorHeight - 10
      : priceTop + 12;
  const inspectorTextX = inspectorX + (mobileRuntime ? 8 : 10);
  const inspectorTitleY = inspectorY + (mobileRuntime ? 16 : 18);
  const inspectorTextY = inspectorY + (mobileRuntime ? 32 : 38);
  const inspectorLineGap = mobileRuntime ? 12 : 14;

  return (
    <section className={`chart-wrap kline-chart kline-market-chart ${mobileRuntime ? "mobile-chart" : "desktop-chart"}`}>
      <header className="kline-market-header">
        <PeriodTabs period={period} onPeriodChange={onPeriodChange} />
        <div className="kline-nav" aria-label="K线视图工具">
          <button type="button" onClick={() => onZoom("in")} disabled={!canZoomIn} aria-label="放大K线" title="减少当前可见K线数量">
            <ZoomIn aria-hidden="true" /><span>放大</span>
          </button>
          <button type="button" onClick={() => onZoom("out")} disabled={!canZoomOut} aria-label="缩小K线" title="增加当前可见K线数量">
            <ZoomOut aria-hidden="true" /><span>缩小</span>
          </button>
          <button type="button" onClick={onReturnLatest} disabled={isAtLatest} aria-label="回到最新K线">
            <LocateFixed aria-hidden="true" /><span>最新</span>
          </button>
          <button type="button" className="kline-fullscreen-toggle" onClick={onToggleFullscreen} aria-pressed={isFullscreen}>
            {isFullscreen ? <Minimize2 aria-hidden="true" /> : <Maximize2 aria-hidden="true" />}
            <span>{isFullscreen ? "退出全屏" : mobileRuntime ? "横屏全屏" : "全屏"}</span>
          </button>
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
        onPointerUp={endGesture}
        onPointerCancel={(event) => endGesture(event, false)}
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
        <line className={`kline-current-line ${latestDirectionClass}`} x1={plotInsetStart} x2={currentPriceLabelX} y1={currentPriceY} y2={currentPriceY} />
        {visibleBars.map((bar, index) => {
          const cx = center(index);
          const bodyTop = Math.min(yPrice(bar.open), yPrice(bar.close));
          const bodyHeight = Math.max(1, Math.abs(yPrice(bar.close) - yPrice(bar.open)));
          return (
            <g key={`${bar.date}-${index}`} className={`kline-candle ${klineDirectionClass(bar.close, bar.open)} ${selectedDate === bar.date ? "selected" : ""}`}>
              <line className="kline-wick" x1={cx} x2={cx} y1={yPrice(bar.high)} y2={yPrice(bar.low)} stroke="currentColor" />
              <rect className="kline-body" x={cx - bodyWidth / 2} y={bodyTop} width={bodyWidth} height={bodyHeight} fill="currentColor" />
            </g>
          );
        })}
        {maLines.map((line) => (line.points ? <polyline key={`ma-${line.window}`} className={line.className} points={line.points} fill="none" /> : null))}
        <g className={`kline-last-price ${latestDirectionClass}`} pointerEvents="none">
          <rect x={currentPriceLabelX} y={currentPriceLabelY} width={currentPriceLabelWidth} height="24" rx="4" />
          <text x={currentPriceLabelX + currentPriceLabelWidth / 2} y={currentPriceLabelY + 16}>{formatPrice(latest.close)}</text>
        </g>
        {mobileRuntime ? (
          <>
            <rect className="kline-subpanel-bg" x="0" y={macdTop - 12} width={width} height={macdHeight + 24} />
            <PanelBorder width={width} y={macdTop - 12} />
            <MacdLayer series={macd} top={macdTop} height={macdHeight} width={width} center={center} barWidth={bodyWidth} />
            <PanelTitle x={10} y={macdTop + 14} label="MACD" />
            <rect className="kline-subpanel-bg" x="0" y={(layout.rsiTop || 0) - 12} width={width} height={(layout.rsiHeight || 0) + 24} />
            <PanelBorder width={width} y={(layout.rsiTop || 0) - 12} />
            <KdjLayer series={kdjSeries} top={layout.rsiTop || 0} height={layout.rsiHeight || 0} width={width} center={center} />
            <PanelTitle x={10} y={(layout.rsiTop || 0) + 14} label="KDJ" />
            <rect className="kline-subpanel-bg" x="0" y={volumeTop - 12} width={width} height={volumeHeight + 24} />
            <PanelBorder width={width} y={volumeTop - 12} />
            <VolumeLayer bars={visibleBars} volumes={volumes} max={volumeMax} top={volumeTop} height={volumeHeight} center={center} barWidth={bodyWidth} />
            <PanelTitle x={10} y={volumeTop + 14} label="VOL" />
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
            <rect className="kline-inspector-card" x={inspectorX} y={inspectorY} width={inspectorWidth} height={inspectorHeight} rx={mobileRuntime ? 6 : 5} />
            <text className="kline-inspector-title" x={inspectorTextX} y={inspectorTitleY}>{selectedBar.date}</text>
            <text className="kline-inspector-text" x={inspectorTextX} y={inspectorTextY}>
              <tspan x={inspectorTextX}>开 {formatPrice(selectedBar.open)}</tspan>
              <tspan x={inspectorTextX} dy={inspectorLineGap}>高 {formatPrice(selectedBar.high)}</tspan>
              <tspan x={inspectorTextX} dy={inspectorLineGap}>低 {formatPrice(selectedBar.low)}</tspan>
              <tspan x={inspectorTextX} dy={inspectorLineGap}>收 {formatPrice(selectedBar.close)}</tspan>
              <tspan x={inspectorTextX} dy={inspectorLineGap}>量 {formatNumber(selectedBar.volume)}</tspan>
            </text>
          </g>
        )}
      </svg>
      <div className="chart-labels"><span>{visibleRange}</span><span>{periodLabel(period)} · {visibleBars.length}/{allBars.length}</span></div>
      <div className="kline-stats" aria-label={selectedBar ? "选中K线" : isAtLatest ? "当前周期最新K线" : "当前历史区间末端K线"}>
        <span>{selectedBar ? "选中" : isAtLatest ? "最新" : "区间末"} {inspectedBar?.date || "--"}</span>
        <span>开 {formatPrice(inspectedBar?.open)}</span>
        <span>高 {formatPrice(inspectedBar?.high)}</span>
        <span>低 {formatPrice(inspectedBar?.low)}</span>
        <span>收 {formatPrice(inspectedBar?.close)}</span>
        <span className={klineDirectionClass(inspectedChangePercent ?? 0, 0)}>
          涨跌 {formatSignedPercent(inspectedChangePercent)}
        </span>
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
  const scale = buildMacdScale(series, top, height);
  const midY = scale.zeroY;
  const y = scale.y;
  const line = (key: "dif" | "dea") => series.map((point, index) => `${center(index).toFixed(2)},${y(point[key]).toFixed(2)}`).join(" ");
  const crosses = buildCrossMarkers(series, "dif", "dea");

  return (
    <g className="kline-macd-layer">
      <line className="trend-grid-line macd-zero-line" x1="0" x2={width} y1={midY} y2={midY} />
      {series.map((point, index) => {
        const barTop = y(Math.max(point.macd, 0));
        const barBottom = y(Math.min(point.macd, 0));
        const histogramWidth = Math.max(2.8, Math.min(barWidth * 1.08, 8.4));
        return (
          <rect
            key={`${point.date}-${index}`}
            className={`macd-bar ${macdDirectionClass(point.macd)}`}
            x={center(index) - histogramWidth / 2}
            y={Math.min(barTop, barBottom)}
            width={histogramWidth}
            height={Math.abs(barBottom - barTop)}
            fill="currentColor"
          />
        );
      })}
      <polyline className="trend-line line-dif" points={line("dif")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <polyline className="trend-line line-dea" points={line("dea")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <CrossMarkerLayer crosses={crosses} center={center} y={(point) => y((point.dif + point.dea) / 2)} />
    </g>
  );
}

function KdjLayer({
  series,
  top,
  height,
  width,
  center,
}: {
  series: ReturnType<typeof computeKdj>;
  top: number;
  height: number;
  width: number;
  center: (index: number) => number;
}) {
  if (series.length < 2) return null;
  const values = series.flatMap((point) => [point.k, point.d, point.j]).filter(Number.isFinite);
  values.push(0, 20, 50, 80, 100);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const y = (value: number) => top + (1 - (value - min) / range) * height;
  const line = (key: "k" | "d" | "j") =>
    series
      .map((point, index) => {
        const value = point[key];
        if (!Number.isFinite(value)) return null;
        return `${center(index).toFixed(2)},${y(value).toFixed(2)}`;
      })
      .filter(Boolean)
      .join(" ");
  const crosses = buildCrossMarkers(series, "k", "d");

  return (
    <g className="kline-kdj-layer">
      <line className="trend-grid-line kdj-guide-line" x1="0" x2={width} y1={y(80)} y2={y(80)} />
      <line className="trend-grid-line kdj-guide-line" x1="0" x2={width} y1={y(50)} y2={y(50)} />
      <line className="trend-grid-line kdj-guide-line" x1="0" x2={width} y1={y(20)} y2={y(20)} />
      <polyline className="trend-line line-k" points={line("k")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <polyline className="trend-line line-d" points={line("d")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <polyline className="trend-line line-j" points={line("j")} fill="none" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" />
      <CrossMarkerLayer crosses={crosses} center={center} y={(point) => y((point.k + point.d) / 2)} />
    </g>
  );
}

function buildCrossMarkers<T extends Record<string, number | string>>(series: T[], fastKey: string, slowKey: string) {
  const crosses: Array<{ index: number; type: "golden" | "death"; point: T }> = [];
  for (let index = 1; index < series.length; index += 1) {
    const prevFast = Number(series[index - 1][fastKey]);
    const prevSlow = Number(series[index - 1][slowKey]);
    const fast = Number(series[index][fastKey]);
    const slow = Number(series[index][slowKey]);
    if (![prevFast, prevSlow, fast, slow].every(Number.isFinite)) continue;
    const prevDiff = prevFast - prevSlow;
    const diff = fast - slow;
    if (prevDiff <= 0 && diff > 0) crosses.push({ index, type: "golden", point: series[index] });
    if (prevDiff >= 0 && diff < 0) crosses.push({ index, type: "death", point: series[index] });
  }
  return crosses;
}

function CrossMarkerLayer<T>({
  crosses,
  center,
  y,
}: {
  crosses: Array<{ index: number; type: "golden" | "death"; point: T }>;
  center: (index: number) => number;
  y: (point: T) => number;
}) {
  if (!crosses.length) return null;
  return (
    <g className="indicator-cross-layer" pointerEvents="none">
      {crosses.map((cross) => {
        const x = center(cross.index);
        const markerY = y(cross.point);
        const isGolden = cross.type === "golden";
        return (
          <g key={`${cross.type}-${cross.index}`} className={`indicator-cross-marker ${isGolden ? "golden" : "death"}`} transform={`translate(${x.toFixed(2)} ${markerY.toFixed(2)})`}>
            <circle r="5.8" />
            <path d={isGolden ? "M -5 3 L 0 -5 L 5 3" : "M -5 -3 L 0 5 L 5 -3"} />
          </g>
        );
      })}
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
            className={klineDirectionClass(bar.close, bar.open)}
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

function formatSignedPercent(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "--";
  return `${value > 0 ? "+" : ""}${value.toFixed(2)}%`;
}

function periodLabel(period: KlinePeriod): string {
  return KLINE_PERIODS.find((item) => item.key === period)?.label || period;
}
