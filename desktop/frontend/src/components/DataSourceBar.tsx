import { useCallback, useEffect, useState } from "react";
import { getJson, postJson } from "../lib/tauri";
import { formatBytes, formatDateTime, formatNumber } from "../lib/format";
import type { DataStatus } from "../types";

interface DataSourceBarProps {
  mobileRuntime: boolean;
}

const CACHE_POLICY = {
  mode: "light",
  max_bytes: 209715200,
  daily_days: 500,
  minute_days: 3,
};

export function DataSourceBar({ mobileRuntime }: DataSourceBarProps) {
  const [status, setStatus] = useState<DataStatus | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshLog, setRefreshLog] = useState<{ time: string; message: string; tone: string }[]>([]);
  const [progress, setProgress] = useState<{ label: string; value: number } | null>(null);

  const appendLog = useCallback((message: string, tone = "info") => {
    setRefreshLog((prev) => [{ time: new Date().toLocaleTimeString("zh-CN"), message, tone }, ...prev].slice(0, 80));
  }, []);

  const loadStatus = useCallback(async () => {
    try {
      const data = await getJson<DataStatus>("/api/data-sources/status");
      setStatus(data);
    } catch (err) {
      appendLog(`Status failed: ${(err as Error).message}`, "error");
    }
  }, [appendLog]);

  useEffect(() => {
    loadStatus();
  }, [loadStatus]);

  const refreshUniverse = useCallback(async () => {
    setRefreshing(true);
    setProgress({ label: mobileRuntime ? "Refreshing local mobile universe..." : "Refreshing universe...", value: 12 });
    try {
      const data = await postJson<{ status?: DataStatus; notes?: string[]; refreshed?: boolean }>("/api/data-sources/refresh-universe", CACHE_POLICY);
      setStatus(data.status || data as DataStatus);
      setProgress({ label: "Refresh complete", value: 100 });
      appendLog((data.notes || []).join(" ") || (data.refreshed ? "Refresh complete." : "Refresh checked."), "success");
    } catch (err) {
      appendLog(`Refresh failed: ${(err as Error).message}`, "error");
      setProgress({ label: "Refresh failed", value: 100 });
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 900);
    }
  }, [appendLog, mobileRuntime]);

  const pruneCache = useCallback(async () => {
    setRefreshing(true);
    try {
      const data = await postJson<{ status?: DataStatus; removed_files?: number; removed_bytes?: number; notes?: string[] }>("/api/data-sources/prune-cache", { ...CACHE_POLICY, mode: "clear" });
      setStatus(data.status || null);
      appendLog(`Removed ${data.removed_files || 0} files, freed ${formatBytes(data.removed_bytes)}.`, "success");
    } catch (err) {
      appendLog(`Prune failed: ${(err as Error).message}`, "error");
    } finally {
      setRefreshing(false);
    }
  }, [appendLog]);

  return (
    <div className="data-source-bar">
      <div className="data-source-status">
        <span className="status-item"><em>Universe</em><strong>{formatNumber(status?.universe_count)}</strong></span>
        <span className="status-item"><em>Cache</em><strong>{formatBytes(status?.cache_bytes)}</strong></span>
        <span className="status-item"><em>Updated</em><strong>{formatDateTime(status?.universe_updated_at)}</strong></span>
        <span className={`status-item ${status?.stale ? "stale" : "fresh"}`}><em>State</em><strong>{status?.stale ? "stale" : "ready"}</strong></span>
      </div>

      <div className="data-source-actions">
        <button type="button" className="action-btn" onClick={refreshUniverse} disabled={refreshing}>
          {mobileRuntime ? "Mobile refresh" : "Refresh universe"}
        </button>
        <button type="button" className="action-btn" onClick={pruneCache} disabled={refreshing}>Prune cache</button>
      </div>

      {progress && (
        <div className="refresh-progress">
          <div className="refresh-progress-bar"><div className="refresh-progress-fill" style={{ width: `${progress.value}%` }} /></div>
          <span className="refresh-progress-label">{progress.label}</span>
        </div>
      )}

      {refreshLog.length > 0 && (
        <div className="refresh-log">
          {refreshLog.map((entry, i) => (
            <div key={i} className={`refresh-log-entry ${entry.tone}`}>
              <span className="refresh-log-time">{entry.time}</span>
              <span className="refresh-log-message">{entry.message}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
