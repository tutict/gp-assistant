import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getJson, getTauriInvoke, postJson, refreshTauriMarketData } from "../lib/tauri";
import { formatBytes, formatNumber } from "../lib/format";
import type { DataStatus } from "../types";

interface DataSourceBarProps {
  mobileRuntime: boolean;
}

const REFRESH_LOG_AUTO_COLLAPSE_MS = 4000;

const CACHE_POLICY = {
  mode: "light",
  max_bytes: 209715200,
  daily_days: 500,
  minute_days: 3,
};

interface RefreshLogEntry {
  time: string;
  message: string;
  tone: string;
}

export function DataSourceBar({ mobileRuntime }: DataSourceBarProps) {
  const [status, setStatus] = useState<DataStatus | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshLog, setRefreshLog] = useState<RefreshLogEntry[]>([]);
  const [refreshLogOpen, setRefreshLogOpen] = useState(false);
  const refreshLogTimerRef = useRef<number | null>(null);
  const [progress, setProgress] = useState<{ label: string; value: number } | null>(null);
  const [batchCount, setBatchCount] = useState(mobileRuntime ? 8 : 32);
  const [maxCandidates, setMaxCandidates] = useState(15000);
  const [fullRebuild, setFullRebuild] = useState(true);
  const [sourceToolsOpen, setSourceToolsOpen] = useState(false);

  useEffect(() => {
    setBatchCount(mobileRuntime ? 8 : 32);
    if (!mobileRuntime) setSourceToolsOpen(false);
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

  const updateProgressFromRefresh = useCallback((entry: RefreshLogEntry) => {
    appendLog(entry.message, entry.tone);
    const match = entry.message.match(/(\d+)[-/](\d+|\?)/);
    if (!match) return;
    const done = Number(match[1]);
    const total = Number(match[2]);
    if (Number.isFinite(done) && Number.isFinite(total) && total > 0) {
      const value = Math.max(8, Math.min(98, Math.round((done / total) * 100)));
      setProgress({ label: `刷新批次 ${Math.min(done, total)}/${total}`, value });
    }
  }, [appendLog]);

  const loadStatus = useCallback(async () => {
    try {
      const data = await getJson<DataStatus>("/api/data-sources/status");
      setStatus(data);
    } catch (err) {
      appendLog(`状态读取失败：${(err as Error).message}`, "error");
    }
  }, [appendLog]);

  useEffect(() => {
    loadStatus();
  }, [loadStatus]);

  const refreshOptions = useMemo(() => ({
    ...CACHE_POLICY,
    mode: fullRebuild ? "full" : "light",
    batch_start: 0,
    batch_count: Math.max(1, Math.min(1000, Math.round(batchCount || 1))),
    max_candidates: Math.max(1000, Math.min(50000, Math.round(maxCandidates || 15000))),
    validate_after_write: true,
  }), [batchCount, fullRebuild, maxCandidates]);

  const refreshUniverse = useCallback(async () => {
    clearRefreshLogTimer();
    setRefreshing(true);
    setRefreshLogOpen(true);
    setRefreshLog([]);
    setProgress({ label: mobileRuntime ? "准备移动端股票池全量重建..." : "准备股票池刷新...", value: 6 });
    try {
      const invoke = getTauriInvoke();
      const data = invoke
        ? await refreshTauriMarketData(invoke, { ...refreshOptions, onLog: updateProgressFromRefresh })
        : await postJson<{ status?: DataStatus; notes?: string[]; refreshed?: boolean }>("/api/data-sources/refresh-universe", refreshOptions);
      const nextStatus = (data.status || data) as DataStatus;
      setStatus(nextStatus);
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
  }, [appendLog, clearRefreshLogTimer, mobileRuntime, refreshOptions, scheduleRefreshLogCollapse, updateProgressFromRefresh]);

  const pruneCache = useCallback(async () => {
    clearRefreshLogTimer();
    setRefreshing(true);
    setRefreshLogOpen(true);
    setProgress({ label: "清理可丢弃缓存...", value: 30 });
    try {
      const data = await postJson<{ status?: DataStatus; removed_files?: number; removed_bytes?: number; notes?: string[] }>("/api/data-sources/prune-cache", { ...CACHE_POLICY, mode: "clear" });
      setStatus(data.status || null);
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
  }, [appendLog, clearRefreshLogTimer, scheduleRefreshLogCollapse]);

  const sourceToolsVisible = !mobileRuntime || sourceToolsOpen || refreshing;

  return (
    <div className={`data-source-bar ${mobileRuntime ? "mobile-source-bar" : ""} ${sourceToolsVisible ? "tools-open" : "tools-closed"}`}>
      <div className="data-source-main">
        <div className="data-source-status">
          <span className="status-item"><em>股票池</em><strong>{formatNumber(status?.universe_count)}</strong></span>
          <span className="status-item"><em>缓存</em><strong>{formatBytes(status?.cache_bytes)}</strong></span>
        </div>

        {mobileRuntime && (
          <button
            type="button"
            className="source-tools-toggle"
            onClick={() => setSourceToolsOpen((open) => !open)}
            aria-expanded={sourceToolsVisible}
          >
            {sourceToolsVisible ? "收起" : "刷新"}
          </button>
        )}
      </div>

      <div className="data-source-tools">
        <div className="refresh-options">
          <label><input type="checkbox" checked={fullRebuild} onChange={(event) => setFullRebuild(event.target.checked)} disabled={refreshing} />全量重建</label>
          <label>批次<input type="number" min="1" max="1000" value={batchCount} onChange={(event) => setBatchCount(Number(event.target.value) || 1)} disabled={refreshing} /></label>
          <label>候选<input type="number" min="1000" max="50000" step="1000" value={maxCandidates} onChange={(event) => setMaxCandidates(Number(event.target.value) || 15000)} disabled={refreshing} /></label>
        </div>

        <div className="data-source-actions">
          <button type="button" className="action-btn" onClick={refreshUniverse} disabled={refreshing}>{mobileRuntime ? "刷新并校验" : "刷新并校验股票池"}</button>
          <button type="button" className="action-btn" onClick={pruneCache} disabled={refreshing}>清理缓存</button>
        </div>
      </div>

      {progress && (
        <div className="refresh-progress">
          <div className="refresh-progress-bar"><div className="refresh-progress-fill" style={{ width: `${progress.value}%` }} /></div>
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
                  setRefreshLogOpen((open) => !open);
                }}
              >
                {refreshLogOpen ? "收起" : "展开"}
              </button>
            </div>
          </div>
          {refreshLogOpen && (
            <div className="refresh-log">
              {refreshLog.map((entry, i) => (
                <div key={`${entry.time}-${i}`} className={`refresh-log-entry ${entry.tone}`}>
                  <span className="refresh-log-time">{entry.time}</span>
                  <span className="refresh-log-message">{entry.message}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}