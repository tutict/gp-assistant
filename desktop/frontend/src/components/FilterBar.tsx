import { RefreshCw, Settings2, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getTauriInvoke, isMarketStatusStale, postJson, refreshTauriMarketData } from "../lib/tauri";
import { formatBytes, formatMarketRefreshDate } from "../lib/format";
import type { DataStatus } from "../types";

export interface FilterCriteria {
  includeSt: boolean;
  requireInstitutionBuyRatio: boolean;
  minRoe: string;
  maxPe: string;
  maxPb: string;
  minMcap: string;
  industry: string;
  marketScope: string;
  resultLimit: number;
  sortBy: string;
  sortDir: string;
  scoreProfile: "balanced" | "quality" | "trend" | "rotation" | string;
}

interface FilterBarProps {
  mobileRuntime: boolean;
  status: DataStatus | null;
  onStatusChange?: (status: DataStatus | null) => void;
}

interface RefreshLogEntry {
  time: string;
  message: string;
  tone: string;
}

const REFRESH_LOG_AUTO_COLLAPSE_MS = 4000;

const CACHE_POLICY = {
  mode: "light",
  max_bytes: 209715200,
  daily_days: 500,
  minute_days: 3,
};


export function FilterBar({ mobileRuntime, status, onStatusChange }: FilterBarProps) {
  const [refreshing, setRefreshing] = useState(false);
  const [refreshLog, setRefreshLog] = useState<RefreshLogEntry[]>([]);
  const [refreshLogOpen, setRefreshLogOpen] = useState(false);
  const refreshLogTimerRef = useRef<number | null>(null);
  const autoRefreshStartedRef = useRef(false);
  const [progress, setProgress] = useState<{ label: string; value: number } | null>(null);
  const [batchCount, setBatchCount] = useState(mobileRuntime ? 12 : 32);
  const [maxCandidates, setMaxCandidates] = useState(15000);
  const [fullRebuild, setFullRebuild] = useState(true);
useEffect(() => {
    setBatchCount(mobileRuntime ? 12 : 32);
  }, [mobileRuntime]);

  const clearRefreshLogTimer = useCallback(() => {
    if (refreshLogTimerRef.current !== null) {
      window.clearTimeout(refreshLogTimerRef.current);
      refreshLogTimerRef.current = null;
    }
  }, []);

  const scheduleRefreshLogCollapse = useCallback(() => {
    clearRefreshLogTimer();
    refreshLogTimerRef.current = window.setTimeout(() => {
      setRefreshLogOpen(false);
      refreshLogTimerRef.current = null;
    }, REFRESH_LOG_AUTO_COLLAPSE_MS);
  }, [clearRefreshLogTimer]);

  useEffect(() => () => clearRefreshLogTimer(), [clearRefreshLogTimer]);

  const appendLog = useCallback((message: string, tone = "info") => {
    setRefreshLog((prev) => [{ time: new Date().toLocaleTimeString("zh-CN"), message, tone }, ...prev].slice(0, 120));
  }, []);

  const commitStatus = useCallback((nextStatus: DataStatus | null) => {
    onStatusChange?.(nextStatus);
  }, [onStatusChange]);

  const updateProgressFromRefresh = useCallback((entry: RefreshLogEntry) => {
    appendLog(entry.message, entry.tone);
    const match = entry.message.match(/(\d+)[-/](\d+|\?)/);
    if (!match) return;

    const done = Number(match[1]);
    const total = Number(match[2]);
    if (!Number.isFinite(done) || !Number.isFinite(total) || total <= 0) return;

    const value = Math.max(8, Math.min(98, Math.round((done / total) * 100)));
    setProgress({ label: `刷新批次 ${Math.min(done, total)}/${total}`, value });
  }, [appendLog]);

  const refreshOptions = useMemo(() => ({
    ...CACHE_POLICY,
    mode: fullRebuild ? "full" : "light",
    batch_start: 0,
    batch_count: Math.max(1, Math.min(1000, Math.round(batchCount || 1))),
    max_candidates: Math.max(1000, Math.min(50000, Math.round(maxCandidates || 15000))),
    validate_after_write: true,
  }), [batchCount, fullRebuild, maxCandidates]);

  const autoRefreshOptions = useMemo(() => ({
    ...CACHE_POLICY,
    mode: "light",
    batch_start: 0,
    batch_count: mobileRuntime ? 12 : 32,
    max_candidates: 15000,
    validate_after_write: true,
  }), [mobileRuntime]);

  const refreshUniverse = useCallback(async () => {
    clearRefreshLogTimer();
    setRefreshing(true);
    setRefreshLogOpen(!mobileRuntime);
    setRefreshLog([]);
    setProgress({ label: mobileRuntime ? "准备移动端股票池全量刷新..." : "准备股票池刷新...", value: 6 });

    try {
      const invoke = getTauriInvoke();
      const data = invoke
        ? await refreshTauriMarketData(invoke, { ...refreshOptions, onLog: updateProgressFromRefresh })
        : await postJson<{ status?: DataStatus; notes?: string[]; refreshed?: boolean }>("/api/data-sources/refresh-universe", refreshOptions);
      const nextStatus = (data.status || data) as DataStatus;
      commitStatus(nextStatus);
      setProgress({ label: "刷新、落盘、校验完成", value: 100 });
      appendLog((Array.isArray(data.notes) ? data.notes.join(" ") : "") || (data.refreshed ? "刷新完成。" : "刷新检查完成。"), "success");
      scheduleRefreshLogCollapse();
    } catch (err) {
      appendLog(`刷新失败：${(err as Error).message}`, "error");
      setProgress({ label: "刷新失败", value: 100 });
      scheduleRefreshLogCollapse();
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 1400);
    }
  }, [appendLog, clearRefreshLogTimer, commitStatus, mobileRuntime, refreshOptions, scheduleRefreshLogCollapse, updateProgressFromRefresh]);

  useEffect(() => {
    if (autoRefreshStartedRef.current || refreshing) return;
    let cancelled = false;

    const autoRefreshIfNeeded = async () => {
      if (cancelled || !status || !isMarketStatusStale(status)) return;

      autoRefreshStartedRef.current = true;
      clearRefreshLogTimer();
      setRefreshLogOpen(!mobileRuntime);
      setProgress({ label: "正在后台同步上一开盘日行情...", value: 6 });
      appendLog("检测到行情缓存不是最新开盘日，已在后台触发自动刷新。", "info");

      try {
        const invoke = getTauriInvoke();
        if (!invoke) {
          setProgress({ label: "自动刷新已跳过", value: 100 });
          appendLog("当前网页环境不执行启动自动刷新，避免阻塞筛选运行。", "info");
          scheduleRefreshLogCollapse();
          return;
        }
        const data = await postJson<{ status?: DataStatus; notes?: string[]; refreshed?: boolean; background_refresh?: boolean }>("/api/data-sources/auto-refresh-universe", autoRefreshOptions);
        const nextStatus = (data.status || data) as DataStatus;
        commitStatus(nextStatus);
        setProgress({ label: data.background_refresh ? "后台刷新已启动" : "上一开盘日行情已同步", value: 100 });
        appendLog((Array.isArray(data.notes) ? data.notes.join(" ") : "") || (data.background_refresh ? "后台自动刷新已启动，筛选可继续运行。" : "上一开盘日行情已同步。"), "success");
        scheduleRefreshLogCollapse();
      } catch (err) {
        appendLog(`自动刷新失败：${(err as Error).message}`, "error");
        setProgress({ label: "自动刷新失败", value: 100 });
        scheduleRefreshLogCollapse();
      } finally {
        if (!cancelled) {
          window.setTimeout(() => setProgress(null), 1400);
        }
      }
    };

    void autoRefreshIfNeeded();
    return () => {
      cancelled = true;
    };
  }, [appendLog, autoRefreshOptions, clearRefreshLogTimer, commitStatus, mobileRuntime, refreshing, scheduleRefreshLogCollapse, status, updateProgressFromRefresh]);

  const pruneCache = useCallback(async () => {
    clearRefreshLogTimer();
    setRefreshing(true);
    setRefreshLogOpen(true);
    setProgress({ label: "清理可丢弃缓存...", value: 30 });

    try {
      const data = await postJson<{ status?: DataStatus; removed_files?: number; removed_bytes?: number; notes?: string[] }>(
        "/api/data-sources/prune-cache",
        { ...CACHE_POLICY, mode: "clear" },
      );
      commitStatus(data.status || null);
      setProgress({ label: "清理完成", value: 100 });
      appendLog(`已删除 ${data.removed_files || 0} 个文件，释放 ${formatBytes(data.removed_bytes)}。`, "success");
      scheduleRefreshLogCollapse();
    } catch (err) {
      appendLog(`清理失败：${(err as Error).message}`, "error");
      setProgress({ label: "清理失败", value: 100 });
      scheduleRefreshLogCollapse();
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 900);
    }
  }, [appendLog, clearRefreshLogTimer, commitStatus, scheduleRefreshLogCollapse]);
  const refreshDateSource = status?.quote_trade_date ?? status?.quote_generated_at ?? status?.generated_at ?? status?.universe_updated_at;
  const refreshDateText = formatMarketRefreshDate(refreshDateSource, mobileRuntime);
  const mobileUniverseCount = Number(status?.universe_count);
  const mobileUniverseText = Number.isFinite(mobileUniverseCount)
    ? `已同步 ${mobileUniverseCount >= 10000 ? `${(mobileUniverseCount / 10000).toFixed(1)}万` : Math.max(0, Math.round(mobileUniverseCount)).toString()}只`
    : "待同步";
  const mobileCacheText = status?.cache_bytes !== undefined ? `缓存 ${formatBytes(status.cache_bytes)}` : "缓存 --";
  const mobileRefreshSummary = `${mobileUniverseText} · ${refreshDateText} · ${mobileCacheText}`;
  const maintenanceContent = (
    <div className="screen-refresh-maintenance-panel">
      <div className="refresh-options">
        <label className="refresh-option refresh-option-boolean">
          <input type="checkbox" checked={fullRebuild} onChange={(event) => setFullRebuild(event.target.checked)} disabled={refreshing} />
          <span className="refresh-option-label">全量重建</span>
        </label>
        <label className="refresh-option">
          <span className="refresh-option-label">批次</span>
          <input type="number" min="1" max="1000" value={batchCount} onChange={(event) => setBatchCount(Number(event.target.value) || 1)} disabled={refreshing} />
        </label>
        <label className="refresh-option">
          <span className="refresh-option-label">候选</span>
          <input type="number" min="1000" max="50000" step="1000" value={maxCandidates} onChange={(event) => setMaxCandidates(Number(event.target.value) || 15000)} disabled={refreshing} />
        </label>
      </div>
      <button type="button" className="action-btn screen-refresh-clear-btn" onClick={pruneCache} disabled={refreshing}>
        <Trash2 size={15} aria-hidden="true" /><span>清理缓存</span>
      </button>
    </div>
  );

  const progressAndLog = mobileRuntime ? null : (
    <>
      {progress && (
        <div className="refresh-progress">
          <div className="refresh-progress-bar">
            <div className="refresh-progress-fill" style={{ width: `${progress.value}%` }} />
          </div>
          <span className="refresh-progress-label">{progress.label}</span>
        </div>
      )}

      {refreshLog.length > 0 && (
        <div className={`refresh-log-shell ${refreshLogOpen ? "open" : "collapsed"}`}>
          <div className="refresh-log-header">
            <span>刷新日志</span>
            <div className="refresh-log-header-actions">
              <strong>最近 {refreshLog.length} 条</strong>
              <button
                type="button"
                className="refresh-log-toggle"
                onClick={() => {
                  clearRefreshLogTimer();
                  setRefreshLogOpen((expanded) => !expanded);
                }}
              >
                {refreshLogOpen ? "收起" : "展开"}
              </button>
            </div>
          </div>
          {refreshLogOpen && (
            <div className="refresh-log">
              {refreshLog.map((entry, index) => (
                <div key={`${entry.time}-${index}`} className={`refresh-log-entry ${entry.tone}`}>
                  <span className="refresh-log-time">{entry.time}</span>
                  <span className="refresh-log-message">{entry.message}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </>
  );

  return (
    <>
      {mobileRuntime ? (
        <section
          className={`research-context-bar screen-toolbar-card screen-mobile-toolbar-card ${refreshing ? "refreshing" : ""}`}
          aria-label="股票池数据工具栏"
        >
          <div className="screen-mobile-toolbar-row">
            <div className="screen-mobile-toolbar-main">
              <div className="screen-mobile-status-inline" aria-label="股票池状态" title={mobileRefreshSummary}>
                <strong>{mobileUniverseText}</strong>
                <span aria-hidden="true">·</span>
                <strong className="refresh-date-value">{refreshDateText}</strong>
                <span aria-hidden="true">·</span>
                <strong>{mobileCacheText}</strong>
              </div>
            </div>

            <button
              type="button"
              className="screen-mobile-tools-toggle screen-mobile-refresh-btn"
              onClick={refreshUniverse}
              disabled={refreshing}
            >
              <span>{refreshing ? "刷新中" : "刷新"}</span>
            </button>
          </div>

          {progress && (
            <div className="screen-mobile-refresh-progress" aria-hidden="true">
              <div className="screen-mobile-refresh-progress-fill" style={{ width: `${progress.value}%` }} />
            </div>
          )}
        </section>
      ) : (
        <section className={`research-context-bar screen-toolbar screen-toolbar-card screen-toolbar-compact ${refreshing ? "refreshing" : ""}`} aria-label="股票池数据工具栏">
          <div className="screen-toolbar-compact-row">

            <div className="screen-toolbar-compact-actions">
              <button type="button" className="screen-toolbar-refresh-btn" onClick={refreshUniverse} disabled={refreshing}>
                <RefreshCw className={refreshing ? "spin" : ""} size={15} aria-hidden="true" />
                <span>{refreshing ? "刷新中" : "刷新"}</span>
              </button>
              <details className="screen-refresh-maintenance">
                <summary aria-label="股票池维护设置"><Settings2 size={15} aria-hidden="true" /><span>维护</span></summary>
                {maintenanceContent}
              </details>
            </div>
          </div>
          {progressAndLog}
        </section>
      )}
    </>
  );
}
export type { FilterCriteria as FilterCriteriaType };
