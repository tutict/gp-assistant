import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getJson, getTauriInvoke, postJson, refreshTauriMarketData } from "../lib/tauri";
import { clampInt, formatBytes, formatNumber, formatPercent } from "../lib/format";
import type { DataStatus } from "../types";

export interface FilterCriteria {
  includeSt: boolean;
  requireInstitutionBuyRatio: boolean;
  minRoe: string;
  maxPe: string;
  maxPb: string;
  minMcap: string;
  industry: string;
  resultLimit: number;
  sortBy: string;
  sortDir: string;
}

interface FilterBarProps {
  criteria: FilterCriteria;
  onChange: (criteria: FilterCriteria) => void;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
  mobileRuntime: boolean;
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

const INDUSTRIES = [
  "",
  "银行",
  "证券",
  "保险",
  "房地产开发",
  "半导体",
  "消费电子",
  "医药生物",
  "化学制药",
  "中药",
  "医疗器械",
  "食品饮料",
  "白酒",
  "家用电器",
  "汽车整车",
  "零部件",
  "电力设备",
  "光伏",
  "风电",
  "有色金属",
  "钢铁",
  "煤炭",
  "石油",
  "化工",
  "建材",
  "建筑装饰",
  "计算机",
  "软件",
  "通信",
  "传媒",
  "国防军工",
  "航空航天",
  "机械设备",
  "环保",
  "农业",
  "纺织服装",
  "商贸零售",
  "社会服务",
];

const SORT_OPTIONS = [
  { value: "score", label: "综合评分" },
  { value: "market_cap", label: "市值" },
  { value: "pe", label: "市盈率" },
  { value: "pb", label: "市净率" },
  { value: "roe", label: "净资产收益率" },
  { value: "change_pct", label: "涨跌幅" },
];

export function FilterBar({ criteria, onChange, open, onToggle, onClose, mobileRuntime }: FilterBarProps) {
  const [status, setStatus] = useState<DataStatus | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshLog, setRefreshLog] = useState<RefreshLogEntry[]>([]);
  const [refreshLogOpen, setRefreshLogOpen] = useState(false);
  const refreshLogTimerRef = useRef<number | null>(null);
  const [progress, setProgress] = useState<{ label: string; value: number } | null>(null);
  const [batchCount, setBatchCount] = useState(mobileRuntime ? 12 : 32);
  const [maxCandidates, setMaxCandidates] = useState(15000);
  const [fullRebuild, setFullRebuild] = useState(true);
  const [sourceToolsOpen, setSourceToolsOpen] = useState(false);

  const update = (patch: Partial<FilterCriteria>) => onChange({ ...criteria, ...patch });

  useEffect(() => {
    setBatchCount(mobileRuntime ? 12 : 32);
    if (!mobileRuntime) setSourceToolsOpen(false);
  }, [mobileRuntime]);

  useEffect(() => {
    if (mobileRuntime && open) {
      setSourceToolsOpen(false);
      setRefreshLogOpen(false);
    }
  }, [mobileRuntime, open]);

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
    if (!Number.isFinite(done) || !Number.isFinite(total) || total <= 0) return;

    const value = Math.max(8, Math.min(98, Math.round((done / total) * 100)));
    setProgress({ label: `刷新批次 ${Math.min(done, total)}/${total}`, value });
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
    setRefreshLogOpen(!mobileRuntime);
    setRefreshLog([]);
    setProgress({ label: mobileRuntime ? "准备移动端股票池全量刷新..." : "准备股票池刷新...", value: 6 });

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
      const data = await postJson<{ status?: DataStatus; removed_files?: number; removed_bytes?: number; notes?: string[] }>(
        "/api/data-sources/prune-cache",
        { ...CACHE_POLICY, mode: "clear" },
      );
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

  const summary = [
    criteria.industry ? `行业 ${criteria.industry}` : "全部行业",
    criteria.minRoe ? `ROE >= ${formatPercent(Number(criteria.minRoe))}` : "",
    criteria.maxPe ? `PE <= ${formatNumber(criteria.maxPe)}` : "",
    criteria.maxPb ? `PB <= ${formatNumber(criteria.maxPb)}` : "",
    criteria.minMcap ? `市值 >= ${formatNumber(criteria.minMcap)} 亿` : "",
    "扣非净利润 > 0",
    "扣非净利润增速 > 10%",
    criteria.requireInstitutionBuyRatio ? "机构净买入" : "",
    `返回 ${clampInt(criteria.resultLimit, 1, 200, 10)} 只`,
  ].filter(Boolean).join(" · ");

  const sourceToolsVisible = !mobileRuntime;
  const statusMetricsClassName = mobileRuntime ? "screen-mobile-status-metrics" : "data-source-status screen-status-metrics";
  const statusMetricClassName = mobileRuntime ? "screen-mobile-status-metric" : "status-item screen-status-metric";

  const handleCriteriaToggle = useCallback(() => {
    if (mobileRuntime) {
      setSourceToolsOpen(false);
      setRefreshLogOpen(false);
    }
    onToggle();
  }, [mobileRuntime, onToggle]);

  const toolsContent = sourceToolsVisible ? (
    <div className={mobileRuntime ? "screen-mobile-tools" : "data-source-tools"}>
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

      <div className="data-source-actions">
        <button type="button" className="action-btn" onClick={refreshUniverse} disabled={refreshing}>
          {mobileRuntime ? "校验刷新" : "刷新并校验股票池"}
        </button>
        <button type="button" className="action-btn" onClick={pruneCache} disabled={refreshing}>
          {mobileRuntime ? "清缓存" : "清理缓存"}
        </button>
      </div>
    </div>
  ) : null;

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
        <>
          <section
            className={`research-context-bar screen-toolbar-card screen-mobile-toolbar-card ${refreshing ? "refreshing" : ""}`}
            aria-label="选股控制栏"
          >
            <div className="screen-mobile-toolbar-row">
              <button type="button" className="criteria-toggle" onClick={handleCriteriaToggle}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M3 6h18M6 12h12M10 18h4" />
                </svg>
                <span>筛选条件</span>
              </button>

              <div className="screen-mobile-toolbar-main">
                <span className="criteria-summary" title={summary}>{summary}</span>
                <div className={statusMetricsClassName} aria-label="股票池状态">
                  <span className={statusMetricClassName}><em>股票池</em><strong>{formatNumber(status?.universe_count)}</strong></span>
                  <span className={statusMetricClassName}><em>缓存</em><strong>{formatBytes(status?.cache_bytes)}</strong></span>
                </div>
              </div>

              <button
                type="button"
                className="screen-mobile-tools-toggle screen-mobile-refresh-btn"
                onClick={refreshUniverse}
                disabled={refreshing}
              >
                {refreshing ? "刷新中" : "刷新"}
              </button>
            </div>

            {progress && (
              <div className="screen-mobile-refresh-progress" aria-hidden="true">
                <div className="screen-mobile-refresh-progress-fill" style={{ width: `${progress.value}%` }} />
              </div>
            )}

            {progressAndLog}
          </section>
        </>
      ) : (
        <section className="research-context-bar screen-toolbar screen-toolbar-card tools-open" aria-label="选股控制栏">
          <div className="data-source-main screen-toolbar-main">
            <button type="button" className="criteria-toggle" onClick={handleCriteriaToggle}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M3 6h18M6 12h12M10 18h4" />
              </svg>
              <span>筛选条件</span>
            </button>
            <span className="criteria-summary" title={summary}>{summary}</span>
          </div>

          <div className="screen-toolbar-side">
            <div className="screen-status-row">
              <div className={statusMetricsClassName} aria-label="股票池状态">
                <span className={statusMetricClassName}><em>股票池</em><strong>{formatNumber(status?.universe_count)}</strong></span>
                <span className={statusMetricClassName}><em>缓存</em><strong>{formatBytes(status?.cache_bytes)}</strong></span>
              </div>
            </div>

            {toolsContent}
          </div>

          {progressAndLog}
        </section>
      )}

      {open && <div className={`criteria-overlay ${open ? "open" : ""}`} onClick={onClose} />}

      <aside className={`criteria-panel ${open ? "open" : ""}`}>
        <div className="criteria-panel-header">
          <h2>筛选条件</h2>
          <button type="button" className="criteria-close" onClick={onClose} aria-label="关闭">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="6" y1="6" x2="18" y2="18" />
              <line x1="18" y1="6" x2="6" y2="18" />
            </svg>
          </button>
        </div>

        <div className="criteria-panel-body">
          <div className="form-row">
            <label htmlFor="industry">行业</label>
            <select
              id="industry"
              value={criteria.industry}
              onChange={(event) => update({ industry: event.target.value })}
            >
              {INDUSTRIES.map((industry) => (
                <option key={industry} value={industry}>{industry || "全部行业"}</option>
              ))}
            </select>
          </div>

          <div className="form-row">
            <label htmlFor="minRoe">最低净资产收益率 (%)</label>
            <input
              id="minRoe"
              type="number"
              step="0.1"
              value={criteria.minRoe}
              onChange={(event) => update({ minRoe: event.target.value })}
              placeholder="如 15"
            />
          </div>

          <div className="form-row">
            <label htmlFor="maxPe">最高市盈率</label>
            <input
              id="maxPe"
              type="number"
              step="0.1"
              value={criteria.maxPe}
              onChange={(event) => update({ maxPe: event.target.value })}
              placeholder="如 30"
            />
          </div>

          <div className="form-row">
            <label htmlFor="maxPb">最高市净率</label>
            <input
              id="maxPb"
              type="number"
              step="0.1"
              value={criteria.maxPb}
              onChange={(event) => update({ maxPb: event.target.value })}
              placeholder="如 5"
            />
          </div>

          <div className="form-row">
            <label htmlFor="minMcap">最低市值 (亿)</label>
            <input
              id="minMcap"
              type="number"
              step="1"
              value={criteria.minMcap}
              onChange={(event) => update({ minMcap: event.target.value })}
              placeholder="如 50"
            />
          </div>

          <div className="form-row">
            <label htmlFor="resultLimit">返回数量</label>
            <input
              id="resultLimit"
              type="number"
              min="1"
              max="200"
              value={criteria.resultLimit}
              onChange={(event) => update({ resultLimit: clampInt(event.target.value, 1, 200, 10) })}
            />
          </div>

          <div className="form-row">
            <label htmlFor="sortBy">排序字段</label>
            <select
              id="sortBy"
              value={criteria.sortBy}
              onChange={(event) => update({ sortBy: event.target.value })}
            >
              {SORT_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>

          <div className="form-row">
            <label htmlFor="sortDir">排序方向</label>
            <select
              id="sortDir"
              value={criteria.sortDir}
              onChange={(event) => update({ sortDir: event.target.value })}
            >
              <option value="desc">降序</option>
              <option value="asc">升序</option>
            </select>
          </div>

          <div className="form-row checkbox-row">
            <label>
              <input
                type="checkbox"
                checked={criteria.includeSt}
                onChange={(event) => update({ includeSt: event.target.checked })}
              />
              包含 ST 股票
            </label>
          </div>

          <div className="form-row checkbox-row">
            <label>
              <input
                type="checkbox"
                checked={criteria.requireInstitutionBuyRatio}
                onChange={(event) => update({ requireInstitutionBuyRatio: event.target.checked })}
              />
              机构净买入
            </label>
          </div>
        </div>
      </aside>
    </>
  );
}

export type { FilterCriteria as FilterCriteriaType };
