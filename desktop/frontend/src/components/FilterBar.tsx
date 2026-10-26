import { ChevronRight, RefreshCw, Settings, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getTauriInvoke, postJson, refreshTauriMarketData } from "../lib/tauri";
import { formatBytes, formatMarketRefreshDate } from "../lib/format";
import { useMediaQuery } from "../hooks/useMediaQuery";
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
  prefetch_history_limit: 80,
};

function marketFreshnessTone(status: DataStatus | null): "fresh" | "warning" | "neutral" {
  if (!status) return "neutral";
  if (status.stale === true) return "warning";
  if (status.quote_trade_date && status.current_trade_date) {
    return status.quote_trade_date === status.current_trade_date ? "fresh" : "warning";
  }
  return status.stale === false ? "fresh" : "neutral";
}


export function FilterBar({ status, onStatusChange }: FilterBarProps) {
  const mobileLayout = useMediaQuery("(max-width: 768px)");
  const [refreshing, setRefreshing] = useState(false);
  const [refreshLog, setRefreshLog] = useState<RefreshLogEntry[]>([]);
  const [refreshLogOpen, setRefreshLogOpen] = useState(false);
  const refreshLogTimerRef = useRef<number | null>(null);
  const refreshLogAutoCollapsePendingRef = useRef(false);
  const refreshLogInteractionPausedRef = useRef(false);
  const maintenanceRef = useRef<HTMLDetailsElement>(null);
  const [progress, setProgress] = useState<{ label: string; value: number } | null>(null);
  const [batchCount, setBatchCount] = useState(mobileLayout ? 12 : 32);
  const [maxCandidates, setMaxCandidates] = useState(15000);
  const [fullRebuild, setFullRebuild] = useState(true);
  const [maintenanceOpen, setMaintenanceOpen] = useState(false);
  const [cleanupDomains, setCleanupDomains] = useState<string[]>(["quote", "daily_history"]);
  const [cleanupCutoff, setCleanupCutoff] = useState("");
  const [cleanupPreview, setCleanupPreview] = useState<{
    preview_token: string;
    record_count: number;
    payload_bytes: number;
    domains: Array<{ domain: string; record_count: number; payload_bytes: number }>;
  } | null>(null);

  useEffect(() => {
    setBatchCount(mobileLayout ? 12 : 32);
  }, [mobileLayout]);

  const clearRefreshLogTimeout = useCallback(() => {
    if (refreshLogTimerRef.current !== null) {
      window.clearTimeout(refreshLogTimerRef.current);
      refreshLogTimerRef.current = null;
    }
  }, []);

  const cancelRefreshLogCollapse = useCallback(() => {
    clearRefreshLogTimeout();
    refreshLogAutoCollapsePendingRef.current = false;
  }, [clearRefreshLogTimeout]);

  const scheduleRefreshLogCollapse = useCallback(() => {
    clearRefreshLogTimeout();
    refreshLogAutoCollapsePendingRef.current = true;
    if (refreshLogInteractionPausedRef.current) return;
    refreshLogTimerRef.current = window.setTimeout(() => {
      setRefreshLogOpen(false);
      refreshLogTimerRef.current = null;
      refreshLogAutoCollapsePendingRef.current = false;
    }, REFRESH_LOG_AUTO_COLLAPSE_MS);
  }, [clearRefreshLogTimeout]);

  const pauseRefreshLogCollapse = useCallback(() => {
    refreshLogInteractionPausedRef.current = true;
    clearRefreshLogTimeout();
  }, [clearRefreshLogTimeout]);

  const resumeRefreshLogCollapse = useCallback(() => {
    refreshLogInteractionPausedRef.current = false;
    if (refreshLogAutoCollapsePendingRef.current && refreshLogOpen) scheduleRefreshLogCollapse();
  }, [refreshLogOpen, scheduleRefreshLogCollapse]);

  useEffect(() => () => clearRefreshLogTimeout(), [clearRefreshLogTimeout]);

  useEffect(() => {
    if (!maintenanceOpen || typeof document === "undefined") return;

    const handleMouseDown = (event: MouseEvent) => {
      const target = event.target as Node | null;
      if (target && maintenanceRef.current?.contains(target)) return;
      setMaintenanceOpen(false);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setMaintenanceOpen(false);
    };

    document.addEventListener("mousedown", handleMouseDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleMouseDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [maintenanceOpen]);

  const appendLog = useCallback((message: string, tone = "info") => {
    setRefreshLog((prev) => [{ time: new Date().toLocaleTimeString("zh-CN"), message, tone }, ...prev].slice(0, 120));
  }, []);

  const commitStatus = useCallback((nextStatus: DataStatus | null) => {
    onStatusChange?.(nextStatus);
  }, [onStatusChange]);

  const updateProgressFromRefresh = useCallback((entry: RefreshLogEntry) => {
    appendLog(entry.message, entry.tone);
    const match = entry.message.match(/(\d+)\s*\/\s*(\d+|\?)/);
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

  const refreshUniverse = useCallback(async () => {
    cancelRefreshLogCollapse();
    setRefreshing(true);
    setRefreshLogOpen(false);
    setRefreshLog([]);
    setProgress({ label: mobileLayout ? "准备移动端股票池全量刷新..." : "准备股票池刷新...", value: 6 });

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
  }, [appendLog, cancelRefreshLogCollapse, commitStatus, mobileLayout, refreshOptions, scheduleRefreshLogCollapse, updateProgressFromRefresh]);

  const previewCleanup = useCallback(async () => {
    cancelRefreshLogCollapse();
    setCleanupPreview(null);
    setRefreshLogOpen(true);
    setProgress({ label: "正在计算外部数据清理范围...", value: 30 });

    try {
      const data = await postJson<NonNullable<typeof cleanupPreview>>(
        "/api/data/cleanup/preview",
        { domains: cleanupDomains, cutoff: cleanupCutoff || undefined },
      );
      setCleanupPreview(data);
      setProgress({ label: "清理预览已就绪", value: 100 });
      appendLog(`预览范围包含 ${data.record_count} 条记录，约 ${formatBytes(data.payload_bytes)}。确认后才会删除。`, "info");
      scheduleRefreshLogCollapse();
    } catch (err) {
      appendLog(`清理预览失败：${(err as Error).message}`, "error");
      setProgress({ label: "清理预览失败", value: 100 });
      scheduleRefreshLogCollapse();
    } finally {
      window.setTimeout(() => setProgress(null), 900);
    }
  }, [appendLog, cancelRefreshLogCollapse, cleanupCutoff, cleanupDomains, scheduleRefreshLogCollapse]);

  const confirmCleanup = useCallback(async () => {
    if (!cleanupPreview || refreshing) return;
    cancelRefreshLogCollapse();
    setRefreshing(true);
    setRefreshLogOpen(true);
    setProgress({ label: "正在清理已确认的外部数据...", value: 35 });
    try {
      const data = await postJson<{ removed_records: number; status: DataStatus["external_data"] }>(
        "/api/data/cleanup",
        {
          domains: cleanupDomains,
          cutoff: cleanupCutoff || undefined,
          preview_token: cleanupPreview.preview_token,
        },
      );
      commitStatus({ ...(status || {}), external_data: data.status });
      setCleanupPreview(null);
      setProgress({ label: "清理完成", value: 100 });
      appendLog(`已清理 ${data.removed_records} 条外部数据记录。`, "success");
      scheduleRefreshLogCollapse();
    } catch (err) {
      setCleanupPreview(null);
      appendLog(`清理失败，请重新预览：${(err as Error).message}`, "error");
      setProgress({ label: "清理失败", value: 100 });
      scheduleRefreshLogCollapse();
    } finally {
      setRefreshing(false);
      window.setTimeout(() => setProgress(null), 900);
    }
  }, [appendLog, cancelRefreshLogCollapse, cleanupCutoff, cleanupDomains, cleanupPreview, commitStatus, refreshing, scheduleRefreshLogCollapse, status]);
  const refreshDateSource = status?.quote_trade_date ?? status?.quote_generated_at ?? status?.generated_at ?? status?.universe_updated_at;
  const refreshDateText = formatMarketRefreshDate(refreshDateSource, mobileLayout);
  const mobileUniverseCount = Number(status?.universe_count);
  const mobileUniverseText = Number.isFinite(mobileUniverseCount)
    ? `已同步 ${mobileUniverseCount >= 10000 ? `${(mobileUniverseCount / 10000).toFixed(1)}万` : Math.max(0, Math.round(mobileUniverseCount)).toString()}只`
    : "待同步";
  const mobileCacheText = status?.cache_bytes !== undefined ? `缓存 ${formatBytes(status.cache_bytes)}` : "缓存 --";
  const mobileRefreshSummary = `${mobileUniverseText} · ${refreshDateText} · ${mobileCacheText}`;
  const desktopStatusTone = marketFreshnessTone(status);
  const externalDataStatus = status?.external_data;
  const migrationDegraded = externalDataStatus?.migrations?.some((migration) => migration.state === "migration_degraded") === true;
  const externalSourceRows = (externalDataStatus?.sources ?? []).slice(0, 8).map((source) => ({
    source: String(source.source_id || "unknown"),
    domain: String(source.domain || "unknown"),
    lastSuccess: typeof source.last_success_at === "string" ? source.last_success_at : "尚无成功记录",
    error: typeof source.last_error_category === "string" ? source.last_error_category : "",
  }));
  const externalMigrationRows = (externalDataStatus?.migrations ?? []).filter((migration) => migration.state !== "validated");
  const cleanupDomainOptions = [
    ["security_master", "证券目录"],
    ["quote", "行情"],
    ["daily_history", "日 K"],
    ["financial_snapshot", "财务快照"],
    ["fx_rate", "汇率"],
    ["disclosure", "披露元数据"],
    ["news", "新闻公告"],
  ] as const;
  const maintenanceContent = (
    <div className="screen-refresh-maintenance-panel">
      <div className="external-data-maintenance-status" role="status">
        <strong>
          外部数据：{externalDataStatus?.available ? `${externalDataStatus.record_count ?? 0} 条 · ${formatBytes(externalDataStatus.database_bytes ?? 0)}` : "尚未初始化"}
        </strong>
        {(externalDataStatus?.capacity_warning || migrationDegraded || (externalDataStatus?.needs_user_retry ?? 0) > 0) && (
          <span className="external-data-maintenance-warning">
            {externalDataStatus?.capacity_warning ? "已达到 200 MB 容量提示线。" : ""}
            {migrationDegraded ? "部分旧缓存迁移待修复。" : ""}
            {(externalDataStatus?.needs_user_retry ?? 0) > 0 ? `${externalDataStatus?.needs_user_retry} 项刷新需要重试。` : ""}
          </span>
        )}
        {(externalDataStatus?.pending_foreground_retries ?? 0) > 0 && (
          <span>网络恢复并回到前台时，将尝试恢复 {externalDataStatus?.pending_foreground_retries} 项显式刷新。</span>
        )}
      </div>
      {externalSourceRows.length > 0 && (
        <div className="external-data-source-list" aria-label="数据来源状态">
          {externalSourceRows.map((source) => (
            <div className="external-data-source-row" key={`${source.domain}:${source.source}`}>
              <strong>{source.domain} · {source.source}</strong>
              <span>{source.error ? `最近错误 ${source.error}` : `最近成功 ${source.lastSuccess}`}</span>
            </div>
          ))}
        </div>
      )}
      {externalMigrationRows.length > 0 && (
        <div className="external-data-source-list" aria-label="缓存迁移状态">
          {externalMigrationRows.map((migration) => (
            <div className="external-data-source-row" key={`${String(migration.market)}:${String(migration.domain)}`}>
              <strong>{String(migration.market)} · {String(migration.domain)}</strong>
              <span>{String(migration.state)}</span>
            </div>
          ))}
        </div>
      )}
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
      <div className="refresh-cleanup-options" aria-label="选择清理的数据域">
        {cleanupDomainOptions.map(([domain, label]) => (
          <label className="refresh-option refresh-option-boolean" key={domain}>
            <input
              type="checkbox"
              checked={cleanupDomains.includes(domain)}
              disabled={refreshing}
              onChange={(event) => {
                setCleanupDomains((current) => event.target.checked
                  ? [...current, domain]
                  : current.filter((item) => item !== domain));
                setCleanupPreview(null);
              }}
            />
            <span className="refresh-option-label">{label}</span>
          </label>
        ))}
      </div>
      <label className="refresh-option refresh-cleanup-cutoff">
        <span className="refresh-option-label">仅清理此日期前的数据</span>
        <input type="date" value={cleanupCutoff} disabled={refreshing} onChange={(event) => {
          setCleanupCutoff(event.target.value);
          setCleanupPreview(null);
        }} />
      </label>
      <div className="refresh-cleanup-actions">
        <button type="button" className="action-btn screen-refresh-clear-btn" onClick={() => void previewCleanup()} disabled={refreshing || cleanupDomains.length === 0}>
          <Trash2 size={15} aria-hidden="true" /><span>{cleanupPreview ? "重新预览" : "预览清理"}</span>
        </button>
        {cleanupPreview && (
          <div className="refresh-cleanup-preview" role="status">
            <strong>{cleanupPreview.record_count} 条记录 · {formatBytes(cleanupPreview.payload_bytes)}</strong>
            <span>{cleanupPreview.domains.map((item) => `${item.domain}: ${item.record_count}`).join(" · ")}</span>
            <button type="button" className="action-btn screen-refresh-clear-btn" onClick={() => void confirmCleanup()} disabled={refreshing}>
              确认清理
            </button>
          </div>
        )}
      </div>
    </div>
  );

  const progressAndLog = mobileLayout ? null : (
    <>
      {refreshing && progress && (
        <div className="refresh-progress refresh-progress-red">
          <div
            className="refresh-progress-bar"
            role="progressbar"
            aria-label="正在刷新股票池"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(progress.value)}
          >
            <div className="refresh-progress-fill" style={{ width: `${progress.value}%` }} />
          </div>
        </div>
      )}

      {refreshLog.length > 0 && (
        <div
          id="refresh-log-panel"
          className={`refresh-log-shell ${refreshLogOpen ? "open" : "collapsed"}`}
          aria-hidden={!refreshLogOpen}
          tabIndex={refreshLogOpen ? 0 : -1}
          onMouseEnter={pauseRefreshLogCollapse}
          onMouseLeave={resumeRefreshLogCollapse}
          onFocus={pauseRefreshLogCollapse}
          onBlur={(event) => {
            if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
            resumeRefreshLogCollapse();
          }}
        >
          <div className="refresh-log-panel">
            <div className="refresh-log-header">
              <span>刷新日志</span>
              <strong>最近 {refreshLog.length} 条</strong>
            </div>
            <div className="refresh-log">
              {refreshLog.map((entry, index) => (
                <div key={`${entry.time}-${index}`} className={`refresh-log-entry ${entry.tone}`}>
                  <span className="refresh-log-time">{entry.time}</span>
                  <span className="refresh-log-message">{entry.message}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );

  return (
    <>
      {mobileLayout ? (
        <section
          className={`research-context-bar screen-toolbar-card screen-mobile-toolbar-card ${refreshing ? "refreshing" : ""}`}
          aria-label="股票池数据工具栏"
        >
          <div className="screen-mobile-toolbar-row">
            <div className="screen-mobile-toolbar-main">
              <div className={`screen-mobile-status-inline${desktopStatusTone === "warning" ? " screen-toolbar-status warning" : ""}`} aria-label="股票池状态" title={mobileRefreshSummary}>
                {desktopStatusTone === "warning" ? <strong>行情待更新</strong> : (
                  <>
                    <strong>{mobileUniverseText}</strong>
                    <span aria-hidden="true">·</span>
                    <strong className="refresh-date-value">{refreshDateText}</strong>
                    <span aria-hidden="true">·</span>
                    <strong>{mobileCacheText}</strong>
                  </>
                )}
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
            <p role="status">{progress.label} · {Math.round(progress.value)}%</p>
          )}
          {progress && (
            <div className="screen-mobile-refresh-progress" aria-hidden="true">
              <div className="screen-mobile-refresh-progress-fill" style={{ width: `${progress.value}%` }} />
            </div>
          )}
        </section>
      ) : (
        <section className={`research-context-bar screen-toolbar screen-toolbar-card screen-toolbar-compact ${refreshing ? "refreshing" : ""}`} aria-label="股票池数据工具栏">
          <div className="screen-toolbar-compact-row">
            {desktopStatusTone === "warning" && (
              <div className="screen-toolbar-status warning" aria-label="股票池状态">
                <span className="screen-toolbar-status-dot" aria-hidden="true" />
                <strong>行情待更新</strong>
              </div>
            )}

            <div className="screen-toolbar-compact-actions">
              <button type="button" className="screen-toolbar-refresh-btn" onClick={refreshUniverse} disabled={refreshing}>
                <RefreshCw className={refreshing ? "spin" : ""} size={15} aria-hidden="true" />
                <span>{refreshing ? "刷新中" : "刷新"}</span>
              </button>
              <details ref={maintenanceRef} className="screen-refresh-maintenance" open={maintenanceOpen}>
                <summary
                  aria-label="股票池维护设置"
                  aria-expanded={maintenanceOpen}
                  onClick={(event) => {
                    event.preventDefault();
                    setMaintenanceOpen((open) => !open);
                  }}
                >
                  <Settings size={15} aria-hidden="true" /><span>维护</span>
                </summary>
                {maintenanceContent}
              </details>
              <button
                type="button"
                className="refresh-log-toggle"
                disabled={refreshLog.length === 0}
                aria-expanded={refreshLogOpen}
                aria-controls="refresh-log-panel"
                onClick={() => {
                  cancelRefreshLogCollapse();
                  setRefreshLogOpen((expanded) => !expanded);
                }}
              >
                <ChevronRight className="refresh-log-toggle-icon" size={14} aria-hidden="true" />
                <span>日志</span>
                <span className="refresh-log-badge">{refreshLog.length}</span>
              </button>
            </div>
          </div>
          {progressAndLog}
        </section>
      )}
    </>
  );
}
export type { FilterCriteria as FilterCriteriaType };
