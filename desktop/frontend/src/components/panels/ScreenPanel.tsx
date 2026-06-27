import { useCallback, useState } from "react";
import type { SectorScreenResult, StockRowView, WatchlistItem } from "../../types";
import type { FilterCriteria } from "../FilterBar";
import { postJson } from "../../lib/tauri";
import {
  buildGraphScreenRequest,
  buildScreenCriteria,
  buildSectorScreenRequest,
  buildTrendAnalyzeRequest,
  buildTrendScreenRequest,
  normalizeScreenRows,
  normalizeSectorGroups,
} from "../../lib/contracts";
import { currentSystemDateInputValue, defaultObserveStartDateInputValue, escapeHtml, normalizeStockCode } from "../../lib/format";
import { StockList } from "../StockList";

interface ScreenPanelProps {
  criteria: FilterCriteria;
  onCriteriaChange: (criteria: FilterCriteria) => void;
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  onObserveStock?: (code: string) => void;
  onRunBacktest?: () => void;
}

type ScreenMode = "screen" | "sectorScreen" | "boardScreen" | "graph" | "trendAnalyze" | "trendScreen";

const TABS: { key: ScreenMode; label: string }[] = [
  { key: "screen", label: "Smart screen" },
  { key: "sectorScreen", label: "Concepts" },
  { key: "boardScreen", label: "Boards" },
  { key: "graph", label: "Relation graph" },
  { key: "trendAnalyze", label: "Trend analyze" },
  { key: "trendScreen", label: "Trend screen" },
];

export function ScreenPanel({
  criteria,
  watchlist,
  onWatchlistChange,
  onObserveStock,
  onRunBacktest,
}: ScreenPanelProps) {
  const [mode, setMode] = useState<ScreenMode>("screen");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [trendCode, setTrendCode] = useState("");
  const [trendStart, setTrendStart] = useState(defaultObserveStartDateInputValue());
  const [trendEnd, setTrendEnd] = useState(currentSystemDateInputValue());
  const [maxSectors, setMaxSectors] = useState(12);
  const [perSectorLimit, setPerSectorLimit] = useState(5);
  const [seedCodes, setSeedCodes] = useState("");
  const [relationDepth, setRelationDepth] = useState(1);
  const [relationWeight, setRelationWeight] = useState(0.4);

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
      } else if (mode === "graph") {
        endpoint = "/api/graph-screen";
        payload = buildGraphScreenRequest(criteria, seedCodes, relationDepth, relationWeight);
      } else if (mode === "trendScreen") {
        endpoint = "/api/trend-screen";
        payload = buildTrendScreenRequest(criteria, trendStart, trendEnd);
      } else if (mode === "trendAnalyze") {
        const code = normalizeStockCode(trendCode);
        if (!code) throw new Error("Please enter a valid stock code.");
        endpoint = "/api/trend";
        payload = buildTrendAnalyzeRequest(code, trendStart, trendEnd);
      }

      const data = await postJson(endpoint, payload);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [criteria, maxSectors, mode, perSectorLimit, relationDepth, relationWeight, seedCodes, trendCode, trendEnd, trendStart]);

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

  return (
    <div className="panel-container">
      <div className="panel-tabs">
        {TABS.map((tab) => (
          <button
            key={tab.key}
            className={`panel-tab ${mode === tab.key ? "active" : ""}`}
            onClick={() => { setMode(tab.key); setResult(null); setError(null); }}
            type="button"
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div className="panel-controls">
        {(mode === "sectorScreen" || mode === "boardScreen") && (
          <>
            <div className="form-row inline">
              <label htmlFor="perSectorLimit">Per group</label>
              <input id="perSectorLimit" type="number" min="1" max="50" value={perSectorLimit} onChange={(e) => setPerSectorLimit(Number(e.target.value) || 5)} />
            </div>
            {mode === "sectorScreen" && (
              <div className="form-row inline">
                <label htmlFor="maxSectors">Groups</label>
                <input id="maxSectors" type="number" min="1" max="50" value={maxSectors} onChange={(e) => setMaxSectors(Number(e.target.value) || 12)} />
              </div>
            )}
          </>
        )}

        {mode === "graph" && (
          <>
            <div className="form-row">
              <label htmlFor="seedCodes">Seed codes</label>
              <input id="seedCodes" type="text" value={seedCodes} onChange={(e) => setSeedCodes(e.target.value)} placeholder="600519.SH, 300750.SZ" />
            </div>
            <div className="form-row inline">
              <label htmlFor="relationDepth">Depth</label>
              <input id="relationDepth" type="number" min="1" max="3" value={relationDepth} onChange={(e) => setRelationDepth(Number(e.target.value) || 1)} />
            </div>
            <div className="form-row inline">
              <label htmlFor="relationWeight">Weight</label>
              <input id="relationWeight" type="number" min="0" max="1" step="0.05" value={relationWeight} onChange={(e) => setRelationWeight(Number(e.target.value) || 0.4)} />
            </div>
          </>
        )}

        {(mode === "trendAnalyze" || mode === "trendScreen") && (
          <>
            {mode === "trendAnalyze" && (
              <div className="form-row inline">
                <label htmlFor="trendCode">Code</label>
                <input id="trendCode" type="text" value={trendCode} onChange={(e) => setTrendCode(e.target.value)} placeholder="600519.SH" />
              </div>
            )}
            <div className="form-row inline">
              <label htmlFor="trendStart">Start</label>
              <input id="trendStart" type="date" value={trendStart} onChange={(e) => setTrendStart(e.target.value)} />
            </div>
            <div className="form-row inline">
              <label htmlFor="trendEnd">End</label>
              <input id="trendEnd" type="date" value={trendEnd} onChange={(e) => setTrendEnd(e.target.value)} />
            </div>
          </>
        )}

        <button type="button" className="run-btn" onClick={run} disabled={loading}>
          {loading ? "Running..." : "Run"}
        </button>
      </div>

      <div className="panel-result">
        {error && (
          <div className="result-error">
            <strong>Query failed</strong>
            <p>{escapeHtml(error)}</p>
          </div>
        )}
        {loading && !result && !error && (
          <div className="result-loading">
            <div className="loader" />
            <span>Running analysis...</span>
          </div>
        )}
        {result != null && !loading && (
          <ScreenResultView
            result={result}
            grouped={mode === "sectorScreen" || mode === "boardScreen"}
            watchlist={watchlist}
            onToggleWatchlist={toggleWatchlist}
            onObserveStock={onObserveStock}
            onRunBacktest={onRunBacktest}
          />
        )}
        {!result && !loading && !error && <div className="result-empty"><span>Run a query to see results.</span></div>}
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
  const groups = grouped ? normalizeSectorGroups(result as SectorScreenResult) : [];
  const rows = normalizeScreenRows(result);

  return (
    <div className="result-list">
      <div className="metric-strip">
        <div className="metric"><span>Returned</span><strong>{resultRecord.returned ?? rows.length}</strong></div>
        <div className="metric"><span>Total</span><strong>{resultRecord.total ?? rows.length}</strong></div>
        <div className="metric"><span>Top score</span><strong>{rows[0]?.score?.toFixed(2) ?? "--"}</strong></div>
      </div>

      {onRunBacktest && rows.length > 0 && (
        <div className="result-actions">
          <div><span>Next step</span><strong>Backtest current criteria</strong></div>
          <button type="button" onClick={onRunBacktest}>Backtest</button>
        </div>
      )}

      {groups.length > 0 ? (
        <div className="sector-groups">
          {groups.map((group, i) => (
            <details key={`${group.title}-${i}`} className="sector-group" open={i === 0}>
              <summary>
                <div><h3>{group.title}</h3><p>{group.meta}</p></div>
                <span className="sector-group-meta"><strong>{group.rows.length}</strong><i /></span>
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
      <details className="raw-json"><summary>Raw JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>
    </div>
  );
}
