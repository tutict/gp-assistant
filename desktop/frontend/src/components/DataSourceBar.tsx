import { useCallback, useEffect, useMemo, useState } from "react";
import { getJson, getTauriInvoke, isTauriRuntime, postJson, refreshTauriMarketData, validateTauriMarketCache } from "../lib/tauri";
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

interface RefreshLogEntry {
  time: string;
  message: string;
  tone: string;
}

export function DataSourceBar({ mobileRuntime }: DataSourceBarProps) {
  const [status, setStatus] = useState<DataStatus | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshLog, setRefreshLog] = useState<RefreshLogEntry[]>([]);
  const [progress, setProgress] = useState<{ label: string; value: number } | null>(null);
  const [batchCount, setBatchCount] = useState(mobileRuntime ? 8 : 32);
  const [maxCandidates, setMaxCandidates] = useState(15000);
  const [fullRebuild, setFullRebuild] = useState(true);

  useEffect(() => {
    setBatchCount(mobileRuntime ? 8 : 32);
  }, [mobileRuntime]);

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
    setRefreshing(true);
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
    } catch (err) {
      appendLog(`刷新失败：${(err as Error).message}`, "error");
      setProgress({ label: "刷新失败", value: 100 });
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 1400);
    }
  }, [appendLog, mobileRuntime, refreshOptions, updateProgressFromRefresh]);

  const validateCache = useCallback(async () => {
    setRefreshing(true);
    setProgress({ label: "回读并校验本地缓存...", value: 18 });
    try {
      const invoke = getTauriInvoke();
      if (!invoke) {
        await loadStatus();
        appendLog("Python Web 路径已读取数据源状态；Tauri 原生回读校验只在桌面/安卓应用内可用。", "warn");
        return;
      }
      const validation = await validateTauriMarketCache(invoke, (message, tone) => appendLog(message, tone));
      setProgress({ label: "校验完成", value: 100 });
      appendLog(`校验结果：${formatNumber(validation.stock_count)} 只股票，${formatBytes(validation.bytes)}。`, "success");
      await loadStatus();
    } catch (err) {
      appendLog(`校验失败：${(err as Error).message}`, "error");
      setProgress({ label: "校验失败", value: 100 });
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 1200);
    }
  }, [appendLog, loadStatus]);

  const pruneCache = useCallback(async () => {
    setRefreshing(true);
    setProgress({ label: "清理可丢弃缓存...", value: 30 });
    try {
      const data = await postJson<{ status?: DataStatus; removed_files?: number; removed_bytes?: number; notes?: string[] }>("/api/data-sources/prune-cache", { ...CACHE_POLICY, mode: "clear" });
      setStatus(data.status || null);
      setProgress({ label: "清理完成", value: 100 });
      appendLog(`已删除 ${data.removed_files || 0} 个文件，释放 ${formatBytes(data.removed_bytes)}。`, "success");
    } catch (err) {
      appendLog(`清理失败：${(err as Error).message}`, "error");
      setProgress({ label: "清理失败", value: 100 });
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 900);
    }
  }, [appendLog]);

  return (
    <div className="data-source-bar">
      <div className="data-source-main">
        <div className="data-source-status">
          <span className="status-item"><em>股票池</em><strong>{formatNumber(status?.universe_count)}</strong></span>
          <span className="status-item"><em>缓存</em><strong>{formatBytes(status?.cache_bytes)}</strong></span>
          <span className="status-item"><em>更新</em><strong>{formatDateTime(status?.universe_updated_at)}</strong></span>
          <span className={`status-item ${status?.stale ? "stale" : "fresh"}`}><em>状态</em><strong>{status?.stale ? "过期" : "可用"}</strong></span>
          <span className="status-item"><em>运行时</em><strong>{isTauriRuntime() ? (mobileRuntime ? "Android Tauri" : "Windows Tauri") : "Python Web"}</strong></span>
        </div>

        <div className="refresh-options">
          <label><input type="checkbox" checked={fullRebuild} onChange={(event) => setFullRebuild(event.target.checked)} disabled={refreshing} />全量重建</label>
          <label>批次<input type="number" min="1" max="1000" value={batchCount} onChange={(event) => setBatchCount(Number(event.target.value) || 1)} disabled={refreshing} /></label>
          <label>候选<input type="number" min="1000" max="50000" step="1000" value={maxCandidates} onChange={(event) => setMaxCandidates(Number(event.target.value) || 15000)} disabled={refreshing} /></label>
        </div>
      </div>

      <div className="data-source-actions">
        <button type="button" className="action-btn" onClick={refreshUniverse} disabled={refreshing}>{mobileRuntime ? "移动端刷新" : "刷新股票池"}</button>
        <button type="button" className="action-btn" onClick={validateCache} disabled={refreshing}>校验缓存</button>
        <button type="button" className="action-btn" onClick={pruneCache} disabled={refreshing}>清理缓存</button>
      </div>

      {progress && (
        <div className="refresh-progress">
          <div className="refresh-progress-bar"><div className="refresh-progress-fill" style={{ width: `${progress.value}%` }} /></div>
          <span className="refresh-progress-label">{progress.label}</span>
        </div>
      )}

      {refreshLog.length > 0 && (
        <div className="refresh-log-shell">
          <div className="refresh-log-header">
            <span>刷新日志</span>
            <strong>最近 {refreshLog.length} 条</strong>
          </div>
          <div className="refresh-log">
            {refreshLog.map((entry, i) => (
              <div key={`${entry.time}-${i}`} className={`refresh-log-entry ${entry.tone}`}>
                <span className="refresh-log-time">{entry.time}</span>
                <span className="refresh-log-message">{entry.message}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
