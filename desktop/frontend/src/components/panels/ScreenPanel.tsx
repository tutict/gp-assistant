import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  AdaptiveScreenMode,
  AdaptiveScreenRequest,
  ScreenResult,
  SectorScreenResult,
  StockRowView,
  WatchlistItem,
} from "../../types";
import type { FilterCriteria } from "../FilterBar";
import { CriteriaFields } from "../CriteriaFields";
import { getTauriListen, postJson } from "../../lib/tauri";
import {
  buildAdaptiveScreenRequest,
  buildCustomScreenRequest,
  buildSectorScreenRequest,
  buildTrendScreenRequest,
  isAdaptiveProgressForRun,
  normalizeScreenGroups,
  normalizeScreenRows,
  normalizeSectorGroups,
} from "../../lib/contracts";
import { currentSystemDateInputValue, defaultTrendStartDateInputValue } from "../../lib/format";
import { StockList } from "../StockList";
import { RawJson } from "../RawJson";
import { PanelFeedback } from "../ui/PanelFeedback";

interface ScreenPanelProps {
  criteria: FilterCriteria;
  onCriteriaChange: (criteria: FilterCriteria) => void;
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  onObserveStock?: (code: string) => void;
  onRunBacktest?: (screenSpec?: AdaptiveScreenRequest) => void;
  mobileRuntime?: boolean;
}

type ScreenMode = "screen" | "sectorScreen" | "boardScreen" | "customScreen" | "trendScreen";

function adaptiveRolloutEnabled(value: unknown): boolean {
  return typeof value === "object" && value !== null
    && (value as { algorithm_version?: unknown }).algorithm_version === "adaptive_swing_v1";
}

const TABS: { key: ScreenMode; label: string }[] = [
  { key: "screen", label: "智能选股" },
  { key: "sectorScreen", label: "概念分组" },
  { key: "boardScreen", label: "板块分组" },
  { key: "customScreen", label: "自定义选股" },
  { key: "trendScreen", label: "趋势选股" },
];

export function ScreenPanel({
  criteria,
  onCriteriaChange,
  watchlist,
  onWatchlistChange,
  onObserveStock,
  onRunBacktest,
  mobileRuntime = false,
}: ScreenPanelProps) {
  const [mode, setMode] = useState<ScreenMode>("screen");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [trendStart, setTrendStart] = useState(defaultTrendStartDateInputValue());
  const [trendEnd, setTrendEnd] = useState(currentSystemDateInputValue());
  const [adaptiveMode, setAdaptiveMode] = useState<AdaptiveScreenMode>("auto");
  const [adaptiveProgress, setAdaptiveProgress] = useState<{ percent: number; message: string } | null>(null);
  const [lastAdaptiveRequest, setLastAdaptiveRequest] = useState<AdaptiveScreenRequest | undefined>();
  const activeRunIdRef = useRef<string | null>(null);
  const manualAdaptiveModesEnabled = adaptiveRolloutEnabled(result);

  useEffect(() => {
    const listen = getTauriListen();
    if (!listen) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("adaptive-screen-progress", (event) => {
      const payload = (event as { payload?: { run_id?: string; percent?: number; message?: string } }).payload;
      if (!isAdaptiveProgressForRun(payload, activeRunIdRef.current)) return;
      setAdaptiveProgress({
        percent: Number(payload.percent) || 0,
        message: payload.message || "正在计算",
      });
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      let endpoint = "/api/screen";
      let payload: unknown;
      if (mode === "screen") {
        const request = buildAdaptiveScreenRequest(criteria, adaptiveMode);
        activeRunIdRef.current = request.run_id;
        setAdaptiveProgress({ percent: 2, message: "准备初选" });
        setLastAdaptiveRequest(request);
        payload = request;
      } else {
        payload = buildCustomScreenRequest(criteria);
      }

      if (mode === "sectorScreen") {
        endpoint = "/api/sector-screen";
        payload = buildSectorScreenRequest(criteria, "concept", 5, 12);
      } else if (mode === "boardScreen") {
        endpoint = "/api/sector-screen";
        payload = buildSectorScreenRequest(criteria, "board", 5, 5);
      } else if (mode === "customScreen") {
        endpoint = "/api/custom-screen";
        payload = buildCustomScreenRequest(criteria);
      } else if (mode === "trendScreen") {
        endpoint = "/api/trend-screen";
        payload = buildTrendScreenRequest(criteria, trendStart, trendEnd);
      }

      const data = await postJson(endpoint, payload);
      if (mode === "screen" && !adaptiveRolloutEnabled(data)) {
        setAdaptiveMode("auto");
      }
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
      activeRunIdRef.current = null;
    }
  }, [adaptiveMode, criteria, mode, trendEnd, trendStart]);

  const toggleWatchlist = useCallback((item: StockRowView) => {
    const exists = watchlist.some((w) => w.code === item.code);
    if (exists) {
      onWatchlistChange(watchlist.filter((w) => w.code !== item.code));
    } else {
      onWatchlistChange([
        { code: item.code, name: item.name, industry: item.industry, added_at: new Date().toISOString(), source: mode },
        ...watchlist,
      ]);
    }
  }, [mode, onWatchlistChange, watchlist]);

  const hasControlFields = mode !== "sectorScreen" && mode !== "boardScreen";
  const controlsClassName = `panel-controls screen-panel-controls ${mode === "customScreen" ? "custom-screen-controls" : mode === "sectorScreen" || mode === "boardScreen" ? "grouped-screen-controls" : ""}`;

  const controlFields = (
    <>
      {mode === "screen" && (
        <div className="adaptive-screen-controls">
          <div className="form-row inline">
            <label htmlFor="adaptiveMode">评分模式</label>
            <select
              id="adaptiveMode"
              value={adaptiveMode}
              disabled={loading || !manualAdaptiveModesEnabled}
              title={!manualAdaptiveModesEnabled ? "新版算法发布后可选择市场模式" : undefined}
              onChange={(event) => setAdaptiveMode(event.target.value as AdaptiveScreenMode)}
            >
              <option value="auto">自动识别</option>
              <option value="range">震荡</option>
              <option value="trend">趋势</option>
              <option value="defensive">防守</option>
            </select>
          </div>
          <p className="adaptive-horizon"><strong>10–30 日波段</strong><span>每次按当日宽基、市场宽度和波动状态重新判断</span></p>
          <details className="adaptive-advanced">
            <summary>高级过滤</summary>
            <CriteriaFields criteria={criteria} onChange={onCriteriaChange} idPrefix="adaptiveScreen" />
          </details>
        </div>
      )}

      {mode === "customScreen" && (
        <div className="custom-screen-criteria">
          <CriteriaFields criteria={criteria} onChange={onCriteriaChange} idPrefix="customScreen" />
        </div>
      )}



      {mode === "trendScreen" && (
        <>
          <div className="form-row inline">
            <label htmlFor="trendStart">开始日期</label>
            <input id="trendStart" type="date" value={trendStart} onChange={(e) => setTrendStart(e.target.value)} />
          </div>
          <div className="form-row inline">
            <label htmlFor="trendEnd">结束日期</label>
            <input id="trendEnd" type="date" value={trendEnd} onChange={(e) => setTrendEnd(e.target.value)} />
          </div>
        </>
      )}
    </>
  );

  const runButton = (
    <button type="button" className="run-btn" onClick={run} disabled={loading}>
      {loading ? "运行中..." : "运行"}
    </button>
  );

  const modeTabs = (
    <div className="panel-tabs screen-panel-tabs" role="tablist" aria-label="选股模式">
      {TABS.map((tab) => (
        <button
          key={tab.key}
          className={`panel-tab ${mode === tab.key ? "active" : ""}`}
          onClick={() => {
            setMode(tab.key);
            setResult(null);
            setError(null);
          }}
          type="button"
        >
          {tab.label}
        </button>
      ))}
    </div>
  );

  return (
    <div className={`panel-container screen-panel-container ${result != null ? "has-result" : ""}`}>
      {mobileRuntime ? (
        <>
          {modeTabs}
          {hasControlFields && (
            <div className={controlsClassName}>
              {controlFields}
            </div>
          )}
          <div className="panel-controls screen-panel-run-card">
            {runButton}
          </div>
        </>
      ) : (
        <>
          <div className="screen-panel-desktop-toolbar">
            {modeTabs}
            {runButton}
          </div>
          {hasControlFields && (
            <div className={controlsClassName}>
              {controlFields}
            </div>
          )}
        </>
      )}

      <div className="panel-result screen-panel-result">
        {error && <PanelFeedback kind="error" title="查询失败" description={error} />}
        {loading && !error && (
          <PanelFeedback
            kind="loading"
            description={mode === "screen" && adaptiveProgress
              ? adaptiveProgress.message + "（" + adaptiveProgress.percent + "%）"
              : "正在分析候选股票..."}
          />
        )}
        {result != null && !loading && (
          <ScreenResultView
            key={mode}
            result={result}
            grouped={mode === "sectorScreen" || mode === "boardScreen"}
            watchlist={watchlist}
            onToggleWatchlist={toggleWatchlist}
            onObserveStock={onObserveStock}
            onRunBacktest={onRunBacktest}
            adaptiveRequest={mode === "screen" ? lastAdaptiveRequest : undefined}
          />
        )}
        {!result && !loading && !error && <PanelFeedback kind="empty" description="设置筛选条件后运行查询。" />}
      </div>
    </div>
  );
}


function compactGroupMeta(meta: string) {
  const total = meta.match(/总数\s*([\d,]+)/)?.[1];
  return total ? `/ ${total}` : meta.replace(/返回\s*[\d,]+\s*\/\s*/, "");
}
export const ScreenResultView = memo(function ScreenResultView({
  result,
  grouped,
  watchlist,
  onToggleWatchlist,
  onObserveStock,
  onRunBacktest,
  adaptiveRequest,
}: {
  result: unknown;
  grouped: boolean;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
  onRunBacktest?: (screenSpec?: AdaptiveScreenRequest) => void;
  adaptiveRequest?: AdaptiveScreenRequest;
}) {
  const resultRecord = result as ScreenResult;
  const groups = useMemo(
    () => grouped ? normalizeSectorGroups(result as SectorScreenResult) : normalizeScreenGroups(result),
    [grouped, result],
  );
  const rows = useMemo(() => normalizeScreenRows(result), [result]);

  return (
    <div className="result-list screen-result-list">
      <div className="metric-strip screen-result-metric-strip">
        <div className="metric"><span>返回数</span><strong>{resultRecord.returned ?? rows.length}</strong></div>
        <div className="metric"><span>总数</span><strong>{resultRecord.total ?? rows.length}</strong></div>
        <div className="metric"><span>最高分</span><strong>{rows[0]?.score?.toFixed(2) ?? "--"}</strong></div>
      </div>

      {resultRecord.market_regime && (
        <section className="adaptive-regime-summary" aria-label="市场状态">
          <div>
            <span>系统识别</span>
            <strong>{regimeLabel(resultRecord.market_regime.detected)}</strong>
            {resultRecord.market_regime.overridden && (
              <em>人工覆盖为 {regimeLabel(resultRecord.market_regime.effective)}</em>
            )}
          </div>
          <p>
            置信度 {(resultRecord.market_regime.confidence * 100).toFixed(0)}% ·
            数据 {resultRecord.market_regime.as_of_date || "--"} ·
            候选覆盖 {(resultRecord.market_regime.coverage.candidate_ratio * 100).toFixed(0)}% ·
            宽基 {resultRecord.market_regime.coverage.benchmark_usable}/{resultRecord.market_regime.coverage.benchmark_requested} ·
            市场宽度 {resultRecord.market_regime.coverage.breadth_usable ? "有效" : "不足"}
            （{resultRecord.market_regime.coverage.breadth_observed}/{resultRecord.market_regime.coverage.breadth_requested}，
            {(resultRecord.market_regime.coverage.breadth_coverage_ratio * 100).toFixed(0)}%）
          </p>
          <ul>
            {resultRecord.market_regime.evidence.map((item) => (
              <li key={item.key}><span>{item.label}</span><strong>{formatRegimeEvidence(item.key, item.value)}</strong></li>
            ))}
          </ul>
        </section>
      )}


      {groups.length > 0 ? (
        <div className="sector-groups">
          {groups.map((group, index) => (
            <details
              key={`${group.title}-${index}`}
              className="sector-group"
              open={group.key === "primary"}
            >
              <summary>
                <div className="sector-group-head"><h3>{group.title}</h3></div>
                <span className="sector-group-meta" title={group.meta} aria-label={group.meta}><strong>{group.rows.length}</strong><small>{compactGroupMeta(group.meta)}</small></span>
              </summary>
              <div className="sector-group-content">
                <StockList items={group.rows} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} onObserveStock={onObserveStock} />
              </div>
            </details>
          ))}
        </div>
      ) : (
        <StockList items={rows} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} onObserveStock={onObserveStock} />
      )}

      {resultRecord.notes?.length ? <div className="notes">{resultRecord.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}

      {onRunBacktest && rows.length > 0 && (
        <div className="result-actions screen-result-actions">
          <div><span>下一步</span><strong>用当前条件回测</strong></div>
          <button
            type="button"
            onClick={() => onRunBacktest(
              resultRecord.algorithm_version === "adaptive_swing_v1" ? adaptiveRequest : undefined,
            )}
          >
            回测
          </button>
        </div>
      )}
      <RawJson result={result} />
    </div>
  );
});

function regimeLabel(mode: string): string {
  return {
    range: "震荡",
    trend: "趋势",
    defensive: "防守",
    transition: "过渡",
    insufficient: "数据不足",
  }[mode] || mode;
}

function formatRegimeEvidence(key: string, value: number): string {
  if (key === "breadth"
    || key === "return_20"
    || key === "ma_spread"
    || key === "atr_percentile"
    || key === "direction_consistency"
    || key === "breadth_coverage") {
    return (value * 100).toFixed(1) + "%";
  }
  return value.toFixed(2);
}
