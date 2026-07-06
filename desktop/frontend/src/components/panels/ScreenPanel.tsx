import { useCallback, useState } from "react";
import type { SectorScreenResult, StockRowView, WatchlistItem } from "../../types";
import type { FilterCriteria } from "../FilterBar";
import { CriteriaFields } from "../CriteriaFields";
import { postJson } from "../../lib/tauri";
import {
  buildCustomScreenRequest,
  buildScreenCriteria,
  buildSectorScreenRequest,
  buildTrendScreenRequest,
  normalizeScreenGroups,
  normalizeScreenRows,
  normalizeSectorGroups,
} from "../../lib/contracts";
import { currentSystemDateInputValue, defaultObserveStartDateInputValue, escapeHtml } from "../../lib/format";
import { StockList } from "../StockList";

interface ScreenPanelProps {
  criteria: FilterCriteria;
  onCriteriaChange: (criteria: FilterCriteria) => void;
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  onObserveStock?: (code: string) => void;
  onRunBacktest?: () => void;
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
  const [trendStart, setTrendStart] = useState(defaultObserveStartDateInputValue());
  const [trendEnd, setTrendEnd] = useState(currentSystemDateInputValue());
  const [maxSectors, setMaxSectors] = useState(12);
  const [perSectorLimit, setPerSectorLimit] = useState(5);

  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      let endpoint = "/api/screen";
      let payload: unknown = buildScreenCriteria(criteria);

      if (mode === "sectorScreen") {
        endpoint = "/api/sector-screen";
        payload = buildSectorScreenRequest(criteria, "concept", perSectorLimit, maxSectors);
      } else if (mode === "boardScreen") {
        endpoint = "/api/sector-screen";
        payload = buildSectorScreenRequest(criteria, "board", perSectorLimit, 5);
      } else if (mode === "customScreen") {
        endpoint = "/api/custom-screen";
        payload = buildCustomScreenRequest(criteria);
      } else if (mode === "trendScreen") {
        endpoint = "/api/trend-screen";
        payload = buildTrendScreenRequest(criteria, trendStart, trendEnd);
      }

      const data = await postJson(endpoint, payload);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [criteria, maxSectors, mode, perSectorLimit, trendEnd, trendStart]);

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

  const hasControlFields = mode !== "screen";
  const controlsClassName = `panel-controls screen-panel-controls ${mode === "customScreen" ? "custom-screen-controls" : mode === "sectorScreen" || mode === "boardScreen" ? "grouped-screen-controls" : ""}`;

  const controlFields = (
    <>
      {mode === "customScreen" && (
        <div className="custom-screen-criteria">
          <CriteriaFields criteria={criteria} onChange={onCriteriaChange} idPrefix="customScreen" />
        </div>
      )}
      {(mode === "sectorScreen" || mode === "boardScreen") && (
        <>
          <div className="form-row inline">
            <label htmlFor="perSectorLimit">每组数量</label>
            <input id="perSectorLimit" type="number" min="1" max="50" value={perSectorLimit} onChange={(e) => setPerSectorLimit(Number(e.target.value) || 5)} />
          </div>
          {mode === "sectorScreen" && (
            <div className="form-row inline">
              <label htmlFor="maxSectors">分组数</label>
              <input id="maxSectors" type="number" min="1" max="50" value={maxSectors} onChange={(e) => setMaxSectors(Number(e.target.value) || 12)} />
            </div>
          )}
        </>
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

  return (
    <div className={`panel-container screen-panel-container ${result != null ? "has-result" : ""}`}>
      <div className="panel-tabs screen-panel-tabs">
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

      {mobileRuntime ? (
        <>
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
        <div className={controlsClassName}>
          {controlFields}
          {runButton}
        </div>
      )}

      <div className="panel-result screen-panel-result">
        {error && (
          <div className="result-error">
            <strong>查询失败</strong>
            <p>{escapeHtml(error)}</p>
          </div>
        )}
        {loading && !result && !error && (
          <div className="result-loading">
            <div className="loader" />
            <span>正在分析...</span>
          </div>
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
          />
        )}
        {!result && !loading && !error && <div className="result-empty"><span>设置参数后运行查询。</span></div>}
      </div>
    </div>
  );
}

function ScreenResultView({
  result,
  grouped,
  watchlist,
  onToggleWatchlist,
  onObserveStock,
  onRunBacktest,
}: {
  result: unknown;
  grouped: boolean;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
  onRunBacktest?: () => void;
}) {
  const resultRecord = result as { total?: number; returned?: number; notes?: string[] };
  const groups = grouped ? normalizeSectorGroups(result as SectorScreenResult) : normalizeScreenGroups(result);
  const rows = normalizeScreenRows(result);

  return (
    <div className="result-list screen-result-list">
      <div className="metric-strip screen-result-metric-strip">
        <div className="metric"><span>返回数</span><strong>{resultRecord.returned ?? rows.length}</strong></div>
        <div className="metric"><span>总数</span><strong>{resultRecord.total ?? rows.length}</strong></div>
        <div className="metric"><span>最高分</span><strong>{rows[0]?.score?.toFixed(2) ?? "--"}</strong></div>
      </div>

      {onRunBacktest && rows.length > 0 && (
        <div className="result-actions screen-result-actions">
          <div><span>下一步</span><strong>用当前条件回测</strong></div>
          <button type="button" onClick={onRunBacktest}>回测</button>
        </div>
      )}

      {groups.length > 0 ? (
        <div className="sector-groups">
          {groups.map((group, index) => (
            <details key={`${group.title}-${index}`} className="sector-group">
              <summary>
                <div className="sector-group-head"><h3>{group.title}</h3></div>
                <span className="sector-group-meta"><strong>{group.rows.length}</strong><small>{group.meta}</small></span>
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
      <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}
