import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
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
import { StockList, displayStockScore } from "../StockList";
import { useMediaQuery } from "../../hooks/useMediaQuery";
import { Sheet } from "../ui/Sheet";
import { RawJson } from "../RawJson";
import { PanelFeedback } from "../ui/PanelFeedback";

interface ScreenPanelProps {
  criteria: FilterCriteria;
  onCriteriaChange: (criteria: FilterCriteria) => void;
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  onObserveStock?: (code: string) => void;
  onNewsStock?: (code: string) => void;
  onRunBacktest?: (screenSpec?: AdaptiveScreenRequest, criteriaSnapshot?: FilterCriteria) => void;
  mobileRuntime?: boolean;
}

type ScreenMode = "screen" | "sectorScreen" | "boardScreen" | "customScreen" | "trendScreen";

const TABS: { key: ScreenMode; label: string }[] = [
  { key: "screen", label: "智能选股" },
  { key: "sectorScreen", label: "概念分组" },
  { key: "boardScreen", label: "板块分组" },
  { key: "customScreen", label: "自定义选股" },
  { key: "trendScreen", label: "趋势选股" },
];

const MODE_NOTES: Record<ScreenMode, string> = {
  screen: "按当前市场状态从全市场挑出综合评分较高的股票",
  sectorScreen: "按概念把候选股票分组查看",
  boardScreen: "按行业板块分组查看",
  customScreen: "用你设置的财务和市值条件筛选",
  trendScreen: "在指定日期区间里看趋势强度",
};

const FULL_UNIVERSE_CRITERIA: FilterCriteria = {
  includeSt: false,
  requireInstitutionBuyRatio: false,
  minRoe: "",
  maxPe: "",
  maxPb: "",
  minMcap: "",
  industry: "",
  marketScope: "",
  resultLimit: 10,
  sortBy: "score",
  sortDir: "desc",
  scoreProfile: "balanced",
};

type ScreenRunState = {
  result: unknown | null;
  error: string | null;
  loading: boolean;
  progress: { percent: number; message: string } | null;
  adaptiveRequest?: AdaptiveScreenRequest;
  criteriaSnapshot: FilterCriteria;
};

type ScreenRunOverride = {
  mode?: ScreenMode;
  criteria?: FilterCriteria;
  trendStart?: string;
  trendEnd?: string;
};

function criteriaForMode(mode: ScreenMode, customCriteria: FilterCriteria): FilterCriteria {
  return mode === "customScreen" ? customCriteria : FULL_UNIVERSE_CRITERIA;
}

function emptyRunState(criteriaSnapshot: FilterCriteria): ScreenRunState {
  return {
    result: null,
    error: null,
    loading: false,
    progress: null,
    criteriaSnapshot,
  };
}

export function ScreenPanel({
  criteria,
  onCriteriaChange,
  watchlist,
  onWatchlistChange,
  onObserveStock,
  onNewsStock,
  onRunBacktest,
}: ScreenPanelProps) {
  const mobileLayout = useMediaQuery("(max-width: 768px)");
  const [criteriaOpen, setCriteriaOpen] = useState(false);
  const [draftCriteria, setDraftCriteria] = useState(criteria);
  const [draftDates, setDraftDates] = useState({ start: "", end: "" });
  const [mode, setMode] = useState<ScreenMode>("screen");
  const [runs, setRuns] = useState<Record<ScreenMode, ScreenRunState>>(() => ({
    screen: emptyRunState(FULL_UNIVERSE_CRITERIA),
    sectorScreen: emptyRunState(FULL_UNIVERSE_CRITERIA),
    boardScreen: emptyRunState(FULL_UNIVERSE_CRITERIA),
    customScreen: emptyRunState(criteria),
    trendScreen: emptyRunState(FULL_UNIVERSE_CRITERIA),
  }));
  const [trendStart, setTrendStart] = useState(defaultTrendStartDateInputValue());
  const [trendEnd, setTrendEnd] = useState(currentSystemDateInputValue());
  const activeRunIdRef = useRef<string | null>(null);
  const requestVersions = useRef<Record<ScreenMode, number>>({
    screen: 0,
    sectorScreen: 0,
    boardScreen: 0,
    customScreen: 0,
    trendScreen: 0,
  });

  const updateRun = useCallback((
    target: ScreenMode,
    patch: Partial<ScreenRunState> | ((current: ScreenRunState) => ScreenRunState),
  ) => {
    setRuns((current) => ({
      ...current,
      [target]: typeof patch === "function" ? patch(current[target]) : { ...current[target], ...patch },
    }));
  }, []);

  useEffect(() => {
    const listen = getTauriListen();
    if (!listen) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("adaptive-screen-progress", (event) => {
      const payload = (event as { payload?: unknown }).payload;
      if (!isAdaptiveProgressForRun(payload, activeRunIdRef.current)) return;
      updateRun("screen", {
        progress: {
          percent: Number(payload.percent) || 0,
          message: payload.message || "正在计算",
        },
      });
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [updateRun]);

  const run = useCallback(async (override?: ScreenRunOverride) => {
    const requestMode = override?.mode ?? mode;
    const requestCriteria = criteriaForMode(requestMode, override?.criteria ?? criteria);
    const requestTrendStart = override?.trendStart ?? trendStart;
    const requestTrendEnd = override?.trendEnd ?? trendEnd;
    const requestVersion = ++requestVersions.current[requestMode];
    let adaptiveRequest: AdaptiveScreenRequest | undefined;
    updateRun(requestMode, (current) => ({
      ...current,
      loading: true,
      error: null,
      criteriaSnapshot: { ...requestCriteria },
      progress: requestMode === "screen" ? { percent: 2, message: "准备初选" } : null,
    }));
    try {
      let endpoint = "/api/screen";
      let payload: unknown;
      if (requestMode === "screen") {
        const request = buildAdaptiveScreenRequest(requestCriteria);
        adaptiveRequest = request;
        activeRunIdRef.current = request.run_id;
        payload = request;
      } else {
        payload = buildCustomScreenRequest(requestCriteria);
      }

      if (requestMode === "sectorScreen") {
        endpoint = "/api/sector-screen";
        payload = buildSectorScreenRequest(requestCriteria, "concept", 10, 12);
      } else if (requestMode === "boardScreen") {
        endpoint = "/api/sector-screen";
        payload = buildSectorScreenRequest(requestCriteria, "board", 5, 5);
      } else if (requestMode === "customScreen") {
        endpoint = "/api/custom-screen";
        payload = buildCustomScreenRequest(requestCriteria);
      } else if (requestMode === "trendScreen") {
        endpoint = "/api/trend-screen";
        payload = buildTrendScreenRequest(requestCriteria, requestTrendStart, requestTrendEnd);
      }

      const data = await postJson(endpoint, payload);
      if (requestVersion !== requestVersions.current[requestMode]) return;
      updateRun(requestMode, (current) => ({
        ...current,
        result: data,
        loading: false,
        error: null,
        progress: null,
        adaptiveRequest: requestMode === "screen" ? adaptiveRequest : current.adaptiveRequest,
        criteriaSnapshot: { ...requestCriteria },
      }));
    } catch (err) {
      if (requestVersion !== requestVersions.current[requestMode]) return;
      updateRun(requestMode, (current) => ({
        ...current,
        loading: false,
        error: (err as Error).message,
        progress: null,
      }));
    } finally {
      if (requestVersion === requestVersions.current[requestMode] && requestMode === "screen") {
        activeRunIdRef.current = null;
      }
    }
  }, [criteria, mode, trendEnd, trendStart, updateRun]);

  const applyDraftAndRun = () => {
    if (mode === "customScreen") {
      const nextCriteria = { ...draftCriteria };
      onCriteriaChange(nextCriteria);
      setCriteriaOpen(false);
      void run({ mode, criteria: nextCriteria });
      return;
    }
    const nextStart = draftDates.start;
    const nextEnd = draftDates.end;
    setTrendStart(nextStart);
    setTrendEnd(nextEnd);
    setCriteriaOpen(false);
    void run({ mode, trendStart: nextStart, trendEnd: nextEnd });
  };

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

  const current = runs[mode];
  const { result, error, loading } = current;
  const adaptiveProgress = current.progress;
  const hasControlFields = mode === "customScreen" || mode === "trendScreen";
  const appliedCriteriaSummary = [
    criteria.industry || "全部行业", criteria.marketScope || "全部范围",
    criteria.includeSt ? "包含 ST" : "排除 ST",
    criteria.maxPe && `PE≤${criteria.maxPe}`, criteria.maxPb && `PB≤${criteria.maxPb}`,
    criteria.minRoe && `ROE≥${criteria.minRoe}`, criteria.minMcap && `市值≥${criteria.minMcap}亿`,
    criteria.requireInstitutionBuyRatio && "机构买入占比高于卖出", `最多 ${criteria.resultLimit} 只`,
  ].filter(Boolean).join(" · ");
  const controlsClassName = `panel-controls screen-panel-controls ${mode === "customScreen" ? "custom-screen-controls" : mode === "sectorScreen" || mode === "boardScreen" ? "grouped-screen-controls" : ""}`;
  const emptyDescription = mode === "customScreen"
    ? "设置筛选条件后运行查询。"
    : "点击运行查看当前模式的全市场筛选结果。";

  const controlFields = (
    <>
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
    <button type="button" className="run-btn" onClick={() => void run()} disabled={loading}>
      {loading ? "运行中..." : "运行筛选"}
    </button>
  );

  const modeTabs = (
    <div className="panel-tabs screen-panel-tabs" role="tablist" aria-label="选股模式">
      {TABS.map((tab) => (
        <button
          key={tab.key}
          className={`panel-tab ${mode === tab.key ? "active" : ""}`}
          role="tab"
          aria-selected={mode === tab.key}
          onClick={(event) => {
            if (mobileLayout) event?.currentTarget?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
            setMode(tab.key);
            setCriteriaOpen(false);
          }}
          type="button"
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
  const modeNote = <p className="screen-mode-note">{MODE_NOTES[mode]}</p>;

  return (
    <div className={`panel-container screen-panel-container ${result != null ? "has-result" : ""}`}>
      {mobileLayout ? (
        <>
          {modeTabs}
          {modeNote}
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
          {modeNote}
          {hasControlFields && (
            <div className={controlsClassName}>
              {controlFields}
            </div>
          )}
        </>
      )}

      <div className="screen-criteria-summary">
        <span>{mode === "customScreen" ? `当前条件：${appliedCriteriaSummary}` : mode === "trendScreen" ? `趋势区间：${trendStart} 至 ${trendEnd}` : "当前条件：全市场 · 按综合评分排序"}</span>
        {mobileLayout && hasControlFields && <button type="button" className="action-btn" onClick={() => { setDraftCriteria({...criteria}); setDraftDates({start:trendStart,end:trendEnd}); setCriteriaOpen(true); }}>筛选条件</button>}
      </div>
      <Sheet open={mobileLayout && criteriaOpen} onClose={()=>setCriteriaOpen(false)} label="筛选条件" className="screen-criteria-sheet" backdropClassName="screen-criteria-sheet-backdrop">
        <header><h3>筛选条件</h3><button type="button" className="action-btn" onClick={()=>setCriteriaOpen(false)}>取消</button></header>
        {mode === "customScreen" ? <div className="custom-screen-criteria"><CriteriaFields criteria={draftCriteria} onChange={setDraftCriteria} idPrefix="mobileScreenDraft" /></div> : <>
          <label>开始日期<input type="date" value={draftDates.start} onChange={e=>setDraftDates({...draftDates,start:e.target.value})} /></label>
          <label>结束日期<input type="date" value={draftDates.end} onChange={e=>setDraftDates({...draftDates,end:e.target.value})} /></label>
        </>}
        <footer><button type="button" className="run-btn" onClick={applyDraftAndRun} disabled={loading}>应用并筛选</button></footer>
      </Sheet>

      <div className="panel-result screen-panel-result">
        {error && <PanelFeedback kind="error" title="查询失败" description={error} action={<button type="button" className="action-btn" onClick={() => void run()}>重试</button>} />}
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
            onNewsStock={onNewsStock}
            onRunBacktest={onRunBacktest}
            adaptiveRequest={mode === "screen" ? current.adaptiveRequest : undefined}
            criteriaSnapshot={current.criteriaSnapshot}
          />
        )}
        {!result && !loading && !error && <PanelFeedback kind="empty" description={emptyDescription} />}
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
  onNewsStock,
  onRunBacktest,
  adaptiveRequest,
  criteriaSnapshot,
}: {
  result: unknown;
  grouped: boolean;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
  onNewsStock?: (code: string) => void;
  onRunBacktest?: (screenSpec?: AdaptiveScreenRequest, criteriaSnapshot?: FilterCriteria) => void;
  adaptiveRequest?: AdaptiveScreenRequest;
  criteriaSnapshot?: FilterCriteria;
}) {
  const resultRecord = result as ScreenResult;
  const groups = useMemo(
    () => grouped ? normalizeSectorGroups(result as SectorScreenResult) : normalizeScreenGroups(result),
    [grouped, result],
  );
  const rows = useMemo(() => normalizeScreenRows(result), [result]);
  const highestScore = rows.reduce<number | undefined>((best, row) => {
    const score = displayStockScore(row);
    return score === undefined ? best : best === undefined ? score : Math.max(best, score);
  }, undefined);

  return (
    <div className="result-list screen-result-list">
      <div className="metric-strip screen-result-metric-strip">
        <div className="metric"><span>返回数</span><strong>{resultRecord.returned ?? rows.length}</strong></div>
        <div className="metric"><span>总数</span><strong>{resultRecord.total ?? rows.length}</strong></div>
        <div className="metric"><span>最高分</span><strong>{highestScore?.toFixed(2) ?? "—"}</strong></div>
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
                <StockList items={group.rows} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} onObserveStock={onObserveStock} onNewsStock={onNewsStock} />
              </div>
            </details>
          ))}
        </div>
      ) : (
        <StockList items={rows} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} onObserveStock={onObserveStock} onNewsStock={onNewsStock} />
      )}

      {resultRecord.notes?.length ? <div className="notes">{resultRecord.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}

      {onRunBacktest && rows.length > 0 && (
        <div className="result-actions screen-result-actions">
          <div><span>下一步</span><strong>用当前条件回测</strong></div>
          <button
            type="button"
            onClick={() => onRunBacktest(
              resultRecord.algorithm_version === "adaptive_swing_v1" ? adaptiveRequest : undefined,
              criteriaSnapshot || FULL_UNIVERSE_CRITERIA,
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
