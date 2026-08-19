import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArrowLeft, CircleCheck, CircleHelp, CircleX, History, LoaderCircle, X } from "lucide-react";
import {
  getAgentRun,
  getAgentRunMetrics,
  listAgentRuns,
  type AgentRunDetail,
  type AgentRunMetrics,
  type AgentRunStatus,
  type AgentRunSummary,
} from "../../lib/agentRuns";
import type { AgentStreamEvent, StockRowView, WatchlistItem } from "../../types";
import { IconButton } from "../ui/IconButton";
import { AGENT_RESULT_UNAVAILABLE_TEXT, AgentResultView } from "./AgentResultView";

export interface AgentRunDrawerProps {
  open: boolean;
  activeConversationId?: string;
  initialRunId?: string;
  finishedRunId?: string;
  finishedRunConversationId?: string;
  ledgerRevision?: number;
  returnFocusElement?: HTMLElement | null;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onClose: () => void;
}

export interface AgentRunTimelineItem {
  key: string;
  type: string;
  label: string;
  detail?: string;
  tone: "neutral" | "active" | "success" | "error";
}

type DrawerView = "list" | "detail";
type RunScope = "current" | "all";
type RequestState = "idle" | "loading" | "ready" | "empty" | "missing" | "error";
type TransitionFocusTarget = "back" | "list";

const TIMELINE_TEXT_MAX = 500;
const ERROR_TEXT_MAX = 2_000;
const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function displayText(value: unknown, fallback = ""): string {
  if (typeof value !== "string") return fallback;
  const text = value.trim().slice(0, TIMELINE_TEXT_MAX);
  return text || fallback;
}

function payloadRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function statusTone(value: string): AgentRunTimelineItem["tone"] {
  const normalized = value.trim().toLowerCase();
  if (["failed", "failure", "error"].includes(normalized)) return "error";
  if (["completed", "complete", "success", "ok", "done"].includes(normalized)) return "success";
  if (["running", "pending", "started", "active"].includes(normalized)) return "active";
  return "neutral";
}

export function buildAgentRunTimeline(events: AgentStreamEvent[] | undefined): AgentRunTimelineItem[] {
  if (!Array.isArray(events)) return [];

  return events.flatMap((event, index) => {
    const rawType = displayText(event?.type, "未知事件");
    const normalizedType = rawType.toLowerCase();
    if (normalizedType === "result") return [];

    const payload = payloadRecord(event?.payload);
    const key = `${rawType}-${index}`;
    if (normalizedType === "status") {
      const label = displayText(event.label) || displayText(event.stage, "状态更新");
      const detail = displayText(event.label) && displayText(event.stage) && event.label?.trim() !== event.stage?.trim()
        ? displayText(event.stage)
        : undefined;
      return [{ key, type: rawType, label, detail, tone: statusTone(event.stage || event.label || "") }];
    }

    if (normalizedType === "tool_start") {
      const label = displayText(payload.label)
        || displayText(payload.tool)
        || displayText(event.action, "工具调用");
      return [{ key, type: rawType, label, tone: "active" }];
    }

    if (normalizedType === "tool_result") {
      const outputSummary = displayText(payload.output_summary);
      const status = displayText(payload.status);
      return [{
        key,
        type: rawType,
        label: outputSummary || status || "工具执行完成",
        tone: statusTone(status),
      }];
    }

    if (normalizedType === "evidence") {
      return [{
        key,
        type: rawType,
        label: "证据整理",
        detail: displayText(event.label) || displayText(event.message) || undefined,
        tone: "active",
      }];
    }

    if (normalizedType === "final") {
      return [{
        key,
        type: rawType,
        label: "最终答复",
        detail: displayText(event.label) || displayText(event.message) || undefined,
        tone: "success",
      }];
    }

    if (normalizedType === "error") {
      return [{
        key,
        type: rawType,
        label: displayText(event.message, "运行错误"),
        tone: "error",
      }];
    }

    return [{ key, type: rawType, label: rawType, tone: "neutral" }];
  });
}

function statusLabel(status: AgentRunStatus) {
  if (status === "running") return "运行中";
  if (status === "completed") return "已完成";
  if (status === "failed") return "失败";
  return "状态未知";
}

function StatusIndicator({ status, className = "agent-run-status" }: {
  status: AgentRunStatus;
  className?: string;
}) {
  const Icon = status === "running"
    ? LoaderCircle
    : status === "completed"
      ? CircleCheck
      : status === "failed"
        ? CircleX
        : CircleHelp;
  return (
    <span className={className} data-status={status}>
      <span className="agent-run-status-icon"><Icon size={16} aria-hidden="true" /></span>
      <span className="agent-run-status-label">{statusLabel(status)}</span>
    </span>
  );
}

function formatTimestamp(timestamp: number | undefined) {
  if (!timestamp || !Number.isFinite(timestamp)) return "未知";
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "未知";
  return new Intl.DateTimeFormat("zh-CN", {
    dateStyle: "short",
    timeStyle: "medium",
  }).format(date);
}

function formatDuration(run: AgentRunSummary) {
  const duration = run.durationMs ?? (
    run.completedAtEpochMs && run.startedAtEpochMs
      ? Math.max(0, run.completedAtEpochMs - run.startedAtEpochMs)
      : undefined
  );
  if (duration === undefined) return run.status === "running" ? "进行中" : "未知";
  if (duration < 1_000) return `${duration} 毫秒`;
  if (duration < 60_000) return `${(duration / 1_000).toFixed(duration % 1_000 ? 1 : 0)} 秒`;
  const minutes = Math.floor(duration / 60_000);
  const seconds = Math.floor((duration % 60_000) / 1_000);
  return `${minutes} 分 ${seconds} 秒`;
}

function hasDuration(run: AgentRunSummary) {
  return run.durationMs !== undefined || Boolean(run.completedAtEpochMs && run.startedAtEpochMs);
}

function scopeKey(scope: RunScope, conversationId?: string) {
  return scope === "all" ? "all" : `current:${conversationId || ""}`;
}

interface RunListSnapshot {
  runs: AgentRunSummary[];
  metrics?: AgentRunMetrics;
}

function requestErrorText(error: unknown) {
  return error instanceof Error && error.message.trim()
    ? error.message.trim().slice(0, ERROR_TEXT_MAX)
    : "运行记录加载失败";
}

function isAbortError(error: unknown) {
  return typeof error === "object"
    && error !== null
    && "name" in error
    && error.name === "AbortError";
}

function summaryFromDetail(detail: AgentRunDetail): AgentRunSummary {
  return {
    runId: detail.runId,
    conversationId: detail.conversationId,
    question: detail.question,
    mode: detail.mode,
    status: detail.status,
    startedAtEpochMs: detail.startedAtEpochMs,
    completedAtEpochMs: detail.completedAtEpochMs,
    durationMs: detail.durationMs,
    error: detail.error,
  };
}

export function AgentRunDrawer({
  open,
  activeConversationId,
  initialRunId,
  finishedRunId,
  finishedRunConversationId,
  ledgerRevision = 0,
  returnFocusElement,
  watchlist,
  onToggleWatchlist,
  onClose,
}: AgentRunDrawerProps) {
  const [view, setView] = useState<DrawerView>("list");
  const [scope, setScope] = useState<RunScope>("current");
  const [runs, setRuns] = useState<AgentRunSummary[]>([]);
  const [metrics, setMetrics] = useState<AgentRunMetrics>();
  const [listState, setListState] = useState<RequestState>("idle");
  const [listError, setListError] = useState<string>();
  const [selectedRunId, setSelectedRunId] = useState<string>();
  const [detail, setDetail] = useState<AgentRunDetail>();
  const [detailState, setDetailState] = useState<RequestState>("idle");
  const [detailError, setDetailError] = useState<string>();
  const drawerRef = useRef<HTMLElement | null>(null);
  const closeControlRef = useRef<HTMLDivElement | null>(null);
  const listRequestTokenRef = useRef(0);
  const detailRequestTokenRef = useRef(0);
  const listAbortControllerRef = useRef<AbortController | undefined>(undefined);
  const detailAbortControllerRef = useRef<AbortController | undefined>(undefined);
  const detailRequestRunIdRef = useRef<string | undefined>(undefined);
  const hasOpenedRef = useRef(false);
  const previousInitialRunIdRef = useRef<string | undefined>(undefined);
  const detailIntentRef = useRef(false);
  const observedCurrentConversationRef = useRef<string | undefined>(undefined);
  const listCacheRef = useRef(new Map<string, RunListSnapshot>());
  const completionRefreshRunsRef = useRef(new Set<string>());
  const completionListRefreshesRef = useRef(new Set<string>());
  const observedLedgerRevisionRef = useRef(ledgerRevision);
  const focusWasOpenRef = useRef(false);
  const transitionFocusTargetRef = useRef<TransitionFocusTarget | undefined>(undefined);

  const cancelListRequest = useCallback(() => {
    listAbortControllerRef.current?.abort();
    listAbortControllerRef.current = undefined;
    listRequestTokenRef.current += 1;
  }, []);

  const cancelDetailRequest = useCallback(() => {
    detailAbortControllerRef.current?.abort();
    detailAbortControllerRef.current = undefined;
    detailRequestRunIdRef.current = undefined;
    detailRequestTokenRef.current += 1;
  }, []);

  const syncTerminalDetail = useCallback((loadedDetail: AgentRunDetail) => {
    if (loadedDetail.status === "running") return;
    const updatedSummary = summaryFromDetail(loadedDetail);
    for (const [key, snapshot] of listCacheRef.current) {
      if (!snapshot.runs.some((run) => run.runId === loadedDetail.runId)) continue;
      listCacheRef.current.set(key, {
        ...snapshot,
        runs: snapshot.runs.map((run) => (
          run.runId === loadedDetail.runId ? updatedSummary : run
        )),
      });
    }
    setRuns((currentRuns) => currentRuns.map((run) => (
      run.runId === loadedDetail.runId ? updatedSummary : run
    )));
  }, []);

  const loadList = useCallback((nextScope: RunScope, conversationId?: string) => {
    cancelListRequest();
    setListError(undefined);
    if (nextScope === "current" && !conversationId) {
      setRuns([]);
      setMetrics(undefined);
      setListState("empty");
      return;
    }

    const controller = new AbortController();
    listAbortControllerRef.current = controller;
    const requestToken = ++listRequestTokenRef.current;
    setListState("loading");
    setMetrics(undefined);
    const options = nextScope === "current"
      ? { conversationId: conversationId!, signal: controller.signal }
      : { signal: controller.signal };
    void Promise.all([
      listAgentRuns(options),
      getAgentRunMetrics({ ...options, limit: 200 }).catch(() => null),
    ])
      .then(([loadedRuns, loadedMetrics]) => {
        if (controller.signal.aborted || requestToken !== listRequestTokenRef.current) return;
        listCacheRef.current.set(scopeKey(nextScope, conversationId), {
          runs: loadedRuns,
          metrics: loadedMetrics || undefined,
        });
        setRuns(loadedRuns);
        setMetrics(loadedMetrics || undefined);
        setListState(loadedRuns.length ? "ready" : "empty");
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted || isAbortError(error) || requestToken !== listRequestTokenRef.current) return;
        setRuns([]);
        setMetrics(undefined);
        setListError(requestErrorText(error));
        setListState("error");
      })
      .finally(() => {
        if (listAbortControllerRef.current === controller) {
          listAbortControllerRef.current = undefined;
        }
      });
  }, [cancelListRequest]);

  const loadDetail = useCallback((runId: string) => {
    cancelDetailRequest();
    detailRequestRunIdRef.current = runId;
    const controller = new AbortController();
    detailAbortControllerRef.current = controller;
    const requestToken = ++detailRequestTokenRef.current;
    setSelectedRunId(runId);
    setDetail(undefined);
    setDetailError(undefined);
    setDetailState("loading");
    void getAgentRun(runId, controller.signal)
      .then((loadedDetail) => {
        if (controller.signal.aborted || requestToken !== detailRequestTokenRef.current) return;
        if (!loadedDetail) {
          setDetailState("missing");
          return;
        }
        syncTerminalDetail(loadedDetail);
        setDetail(loadedDetail);
        setDetailState("ready");
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted || isAbortError(error) || requestToken !== detailRequestTokenRef.current) return;
        setDetailError(requestErrorText(error));
        setDetailState("error");
      })
      .finally(() => {
        if (detailAbortControllerRef.current === controller) {
          detailAbortControllerRef.current = undefined;
        }
      });
  }, [cancelDetailRequest, syncTerminalDetail]);

  useEffect(() => {
    if (!open) {
      hasOpenedRef.current = false;
      detailIntentRef.current = false;
      observedCurrentConversationRef.current = undefined;
      previousInitialRunIdRef.current = undefined;
      completionRefreshRunsRef.current.clear();
      completionListRefreshesRef.current.clear();
      transitionFocusTargetRef.current = undefined;
      cancelListRequest();
      cancelDetailRequest();
      setListState("idle");
      setMetrics(undefined);
      setDetailState("idle");
      setSelectedRunId(undefined);
      setDetail(undefined);
      setDetailError(undefined);
      return;
    }

    const isOpening = !hasOpenedRef.current;
    const changedInitialRun = initialRunId !== previousInitialRunIdRef.current;
    hasOpenedRef.current = true;
    previousInitialRunIdRef.current = initialRunId;

    if (!isOpening && !changedInitialRun) return;
    if (initialRunId) {
      cancelListRequest();
      detailIntentRef.current = true;
      setView("detail");
      if (!isOpening) transitionFocusTargetRef.current = "back";
      loadDetail(initialRunId);
      return;
    }

    cancelDetailRequest();
    detailIntentRef.current = false;
    setView("list");
    setScope("current");
    if (!isOpening) transitionFocusTargetRef.current = "list";
    observedCurrentConversationRef.current = activeConversationId;
    loadList("current", activeConversationId);
  }, [activeConversationId, cancelDetailRequest, cancelListRequest, initialRunId, loadDetail, loadList, open]);

  useEffect(() => () => {
    cancelListRequest();
    cancelDetailRequest();
  }, [cancelDetailRequest, cancelListRequest]);

  useEffect(() => {
    if (!open || view !== "list" || scope !== "current" || detailIntentRef.current) return;
    if (observedCurrentConversationRef.current === activeConversationId) return;
    observedCurrentConversationRef.current = activeConversationId;
    loadList("current", activeConversationId);
  }, [activeConversationId, loadList, open, scope, view]);

  useEffect(() => {
    if (
      !open
      || view !== "detail"
      || !selectedRunId
      || detailRequestRunIdRef.current !== selectedRunId
      || (detail?.status !== "running" && detailState !== "missing")
      || finishedRunId !== selectedRunId
      || completionRefreshRunsRef.current.has(selectedRunId)
    ) return;
    completionRefreshRunsRef.current.add(selectedRunId);
    loadDetail(selectedRunId);
  }, [detail?.status, detailState, finishedRunId, loadDetail, open, selectedRunId, view]);

  useEffect(() => {
    if (
      !open
      || view !== "list"
      || !finishedRunId
      || (listState !== "ready" && listState !== "empty")
      || (scope === "current" && finishedRunConversationId && finishedRunConversationId !== activeConversationId)
    ) return;
    const cacheKey = scopeKey(scope, activeConversationId);
    const refreshKey = `${cacheKey}:${finishedRunId}`;
    if (completionListRefreshesRef.current.has(refreshKey)) return;
    const visibleRun = runs.find((run) => run.runId === finishedRunId);
    if (visibleRun && visibleRun.status !== "running") return;
    completionListRefreshesRef.current.add(refreshKey);
    listCacheRef.current.delete(cacheKey);
    loadList(scope, activeConversationId);
  }, [activeConversationId, finishedRunConversationId, finishedRunId, listState, loadList, open, runs, scope, view]);

  useEffect(() => {
    if (observedLedgerRevisionRef.current === ledgerRevision) return;
    observedLedgerRevisionRef.current = ledgerRevision;
    listCacheRef.current.clear();
    completionRefreshRunsRef.current.clear();
    completionListRefreshesRef.current.clear();
    if (!open) return;
    if (view === "detail" && selectedRunId) {
      loadDetail(selectedRunId);
      return;
    }
    loadList(scope, activeConversationId);
  }, [activeConversationId, ledgerRevision, loadDetail, loadList, open, scope, selectedRunId, view]);

  useEffect(() => {
    if (open) {
      closeControlRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    } else if (focusWasOpenRef.current) {
      returnFocusElement?.focus();
    }
    focusWasOpenRef.current = open;
  }, [open, returnFocusElement]);

  useEffect(() => {
    const target = transitionFocusTargetRef.current;
    if (!open || !target) return;
    const selector = target === "back"
      ? ".agent-run-drawer-back"
      : ".agent-run-scope-option[aria-pressed='true']";
    const focusTarget = drawerRef.current?.querySelector<HTMLElement>(selector)
      || closeControlRef.current?.querySelector<HTMLButtonElement>("button");
    focusTarget?.focus();
    transitionFocusTargetRef.current = undefined;
  }, [detailState, listState, open, view]);

  const selectScope = (nextScope: RunScope) => {
    setScope(nextScope);
    loadList(nextScope, activeConversationId);
  };

  const selectRun = (runId: string) => {
    cancelListRequest();
    detailIntentRef.current = true;
    transitionFocusTargetRef.current = "back";
    setView("detail");
    loadDetail(runId);
  };

  const returnToList = () => {
    cancelDetailRequest();
    detailIntentRef.current = false;
    transitionFocusTargetRef.current = "list";
    setView("list");
    const cachedSnapshot = listCacheRef.current.get(scopeKey(scope, activeConversationId));
    if (cachedSnapshot) {
      setRuns(cachedSnapshot.runs);
      setMetrics(cachedSnapshot.metrics);
      setListState(cachedSnapshot.runs.length ? "ready" : "empty");
      setListError(undefined);
      return;
    }
    if (scope === "current") observedCurrentConversationRef.current = activeConversationId;
    loadList(scope, activeConversationId);
  };

  const retryList = () => {
    transitionFocusTargetRef.current = "list";
    loadList(scope, activeConversationId);
  };

  const retryDetail = () => {
    if (!selectedRunId) return;
    transitionFocusTargetRef.current = "back";
    loadDetail(selectedRunId);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "Tab") return;

    const focusableElements = Array.from(event.currentTarget.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
      .filter((element) => element.getAttribute("aria-hidden") !== "true");
    if (!focusableElements.length) {
      event.preventDefault();
      return;
    }

    const activeElement = document.activeElement;
    const firstElement = focusableElements[0];
    const lastElement = focusableElements.at(-1)!;
    if (event.shiftKey) {
      if (activeElement !== firstElement && focusableElements.includes(activeElement as HTMLElement)) return;
      event.preventDefault();
      lastElement.focus();
      return;
    }
    if (activeElement !== lastElement && focusableElements.includes(activeElement as HTMLElement)) return;
    event.preventDefault();
    firstElement.focus();
  };

  if (!open) return null;

  return (
    <aside
      ref={drawerRef}
      className="agent-run-drawer"
      role="dialog"
      aria-modal={true}
      aria-label="Agent 运行复盘"
      onKeyDown={handleKeyDown}
    >
      <header className="agent-run-drawer-header">
        <div className="agent-run-drawer-title">
          <History size={18} aria-hidden="true" />
          <h2>Agent 运行复盘</h2>
        </div>
        {view === "detail" && (
          <IconButton
            className="agent-run-drawer-back"
            icon={<ArrowLeft size={18} />}
            label="返回运行列表"
            onClick={returnToList}
          />
        )}
        <div ref={closeControlRef} className="agent-run-drawer-close-control">
          <IconButton
            className="agent-run-drawer-close"
            icon={<X size={18} />}
            label="关闭复盘"
            onClick={onClose}
          />
        </div>
      </header>
      {view === "list" ? (
        <RunList
          activeConversationId={activeConversationId}
          error={listError}
          metrics={metrics}
          runs={runs}
          scope={scope}
          state={listState}
          onRetry={retryList}
          onSelectRun={selectRun}
          onSelectScope={selectScope}
        />
      ) : (
        <RunDetail
          detail={detail}
          error={detailError}
          state={detailState}
          watchlist={watchlist}
          onRetry={retryDetail}
          onToggleWatchlist={onToggleWatchlist}
        />
      )}
    </aside>
  );
}

function RunList({
  activeConversationId,
  error,
  metrics,
  runs,
  scope,
  state,
  onRetry,
  onSelectRun,
  onSelectScope,
}: {
  activeConversationId?: string;
  error?: string;
  metrics?: AgentRunMetrics;
  runs: AgentRunSummary[];
  scope: RunScope;
  state: RequestState;
  onRetry: () => void;
  onSelectRun: (runId: string) => void;
  onSelectScope: (scope: RunScope) => void;
}) {
  return (
    <section className="agent-run-list" aria-label="运行记录">
      <div className="agent-run-scope" role="group" aria-label="运行记录范围">
        <button
          type="button"
          className="agent-run-scope-option"
          aria-pressed={scope === "current"}
          onClick={() => onSelectScope("current")}
        >
          当前会话
        </button>
        <button
          type="button"
          className="agent-run-scope-option"
          aria-pressed={scope === "all"}
          onClick={() => onSelectScope("all")}
        >
          全部运行
        </button>
      </div>
      {metrics && metrics.sampleSize > 0 && <AgentMetricsSummary metrics={metrics} />}
      {state === "loading" && <p className="agent-run-state" role="status">正在加载运行记录</p>}
      {state === "error" && (
        <div className="agent-run-state agent-run-state-error" role="alert">
          <p>{error}</p>
          <button type="button" className="agent-run-retry" onClick={onRetry}>重试</button>
        </div>
      )}
      {state === "empty" && (
        <p className="agent-run-state" role="status">
          {scope === "current" && !activeConversationId ? "当前会话暂无运行记录" : (
            scope === "current" ? "当前会话暂无运行记录" : "暂无运行记录"
          )}
        </p>
      )}
      {state === "ready" && (
        <ol className="agent-run-list-items">
          {runs.map((run) => (
            <li key={run.runId} className="agent-run-list-item">
              <button
                type="button"
                className="agent-run-select"
                onClick={() => onSelectRun(run.runId)}
              >
                <strong>{run.question || "未命名问题"}</strong>
                <span>{run.mode}</span>
                <StatusIndicator status={run.status} />
                {hasDuration(run) && <span className="agent-run-duration">{formatDuration(run)}</span>}
                <time>{formatTimestamp(run.startedAtEpochMs)}</time>
              </button>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

function AgentMetricsSummary({ metrics }: { metrics: AgentRunMetrics }) {
  const completed = metrics.statusCounts.completed || 0;
  const failed = metrics.statusCounts.failed || 0;
  const modelSuccess = metrics.modelOutcomeCounts.model_success || 0;
  const fallback = Object.values(metrics.profileCounts)
    .reduce((total, profile) => total + profile.fallback, 0);
  return (
    <dl className="agent-run-metrics" aria-label="运行质量概览">
      <div><dt>样本</dt><dd>{metrics.sampleSize}</dd></div>
      <div><dt>完成 / 失败</dt><dd>{completed} / {failed}</dd></div>
      <div><dt>模型成功 / 降级</dt><dd>{modelSuccess} / {fallback}</dd></div>
      <div><dt>P95 耗时</dt><dd>{formatMetricDuration(metrics.durationMs.p95Ms)}</dd></div>
    </dl>
  );
}

function formatMetricDuration(duration: number | undefined) {
  if (duration === undefined) return "--";
  if (duration < 1_000) return `${duration} ms`;
  return `${(duration / 1_000).toFixed(1)} s`;
}

function RunDetail({
  detail,
  error,
  state,
  watchlist,
  onRetry,
  onToggleWatchlist,
}: {
  detail?: AgentRunDetail;
  error?: string;
  state: RequestState;
  watchlist: WatchlistItem[];
  onRetry: () => void;
  onToggleWatchlist: (item: StockRowView) => void;
}) {
  if (state === "loading" || state === "idle") {
    return <p className="agent-run-state" role="status">正在加载运行详情</p>;
  }

  if (state === "missing") {
    return (
      <div className="agent-run-state">
        <p role="status">本次运行未成功留痕</p>
        <button type="button" className="agent-run-retry" onClick={onRetry}>重试</button>
      </div>
    );
  }

  if (state === "error") {
    return (
      <div className="agent-run-state agent-run-state-error" role="alert">
        <p>{error}</p>
        <button type="button" className="agent-run-retry" onClick={onRetry}>重试</button>
      </div>
    );
  }

  if (!detail) return null;
  const timeline = buildAgentRunTimeline(detail.events);
  return (
    <section className="agent-run-detail" aria-label="运行详情">
      <section className="agent-run-overview" aria-label="运行概览">
        <h3>{detail.question || "未命名问题"}</h3>
        <dl>
          <div><dt>模式</dt><dd>{detail.mode}</dd></div>
          <div><dt>状态</dt><dd><StatusIndicator className="agent-run-overview-status" status={detail.status} /></dd></div>
          <div><dt>开始时间</dt><dd>{formatTimestamp(detail.startedAtEpochMs)}</dd></div>
          <div><dt>结束时间</dt><dd>{formatTimestamp(detail.completedAtEpochMs)}</dd></div>
          <div><dt>耗时</dt><dd>{formatDuration(detail)}</dd></div>
        </dl>
      </section>
      <section className="agent-run-timeline" aria-label="执行时间线">
        <h3>执行时间线</h3>
        {timeline.length ? (
          <ol>
            {timeline.map((item) => (
              <li key={item.key} className={`agent-run-timeline-item tone-${item.tone}`}>
                <strong>{item.label}</strong>
                {item.detail && <span>{item.detail}</span>}
              </li>
            ))}
          </ol>
        ) : <p>暂无执行事件</p>}
      </section>
      {detail.error && (
        <section className="agent-run-persisted-error" aria-label="持久化错误" role="alert">
          <h3>运行错误</h3>
          <p>{detail.error}</p>
        </section>
      )}
      {detail.resultUnavailable && (
        <section className="agent-run-result" aria-label="运行结果">
          <p role="status">{AGENT_RESULT_UNAVAILABLE_TEXT}</p>
        </section>
      )}
      {!detail.resultUnavailable && detail.status !== "failed" && detail.result && (
        <section className="agent-run-result" aria-label="运行结果">
          {detail.result.reply && (
            <p className="agent-final-reply agent-run-final-reply">{detail.result.reply}</p>
          )}
          <AgentResultView result={detail.result} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} />
        </section>
      )}
    </section>
  );
}
