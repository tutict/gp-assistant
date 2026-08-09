import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArrowLeft, CircleCheck, CircleHelp, CircleX, History, LoaderCircle, X } from "lucide-react";
import {
  getAgentRun,
  listAgentRuns,
  type AgentRunDetail,
  type AgentRunStatus,
  type AgentRunSummary,
} from "../../lib/agentRuns";
import type { AgentStreamEvent, StockRowView, WatchlistItem } from "../../types";
import { IconButton } from "../ui/IconButton";
import { AgentResultView } from "./AgentResultView";

export interface AgentRunDrawerProps {
  open: boolean;
  activeConversationId?: string;
  initialRunId?: string;
  finishedRunId?: string;
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
  return new Intl.DateTimeFormat("zh-CN", {
    dateStyle: "short",
    timeStyle: "medium",
  }).format(new Date(timestamp));
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

function requestErrorText(error: unknown) {
  return error instanceof Error && error.message.trim()
    ? error.message.trim().slice(0, ERROR_TEXT_MAX)
    : "运行记录加载失败";
}

export function AgentRunDrawer({
  open,
  activeConversationId,
  initialRunId,
  finishedRunId,
  returnFocusElement,
  watchlist,
  onToggleWatchlist,
  onClose,
}: AgentRunDrawerProps) {
  const [view, setView] = useState<DrawerView>("list");
  const [scope, setScope] = useState<RunScope>("current");
  const [runs, setRuns] = useState<AgentRunSummary[]>([]);
  const [listState, setListState] = useState<RequestState>("idle");
  const [listError, setListError] = useState<string>();
  const [selectedRunId, setSelectedRunId] = useState<string>();
  const [detail, setDetail] = useState<AgentRunDetail>();
  const [detailState, setDetailState] = useState<RequestState>("idle");
  const [detailError, setDetailError] = useState<string>();
  const closeControlRef = useRef<HTMLDivElement | null>(null);
  const listRequestTokenRef = useRef(0);
  const detailRequestTokenRef = useRef(0);
  const hasOpenedRef = useRef(false);
  const previousInitialRunIdRef = useRef<string | undefined>(undefined);
  const detailIntentRef = useRef(false);
  const observedCurrentConversationRef = useRef<string | undefined>(undefined);
  const listCacheRef = useRef(new Map<string, AgentRunSummary[]>());
  const completionRefreshRunRef = useRef<string | undefined>(undefined);
  const focusWasOpenRef = useRef(false);

  const loadList = useCallback((nextScope: RunScope, conversationId?: string) => {
    const requestToken = ++listRequestTokenRef.current;
    setListError(undefined);
    if (nextScope === "current" && !conversationId) {
      setRuns([]);
      setListState("empty");
      return;
    }

    setListState("loading");
    const options = nextScope === "current" ? { conversationId: conversationId! } : {};
    void listAgentRuns(options)
      .then((loadedRuns) => {
        if (requestToken !== listRequestTokenRef.current) return;
        listCacheRef.current.set(scopeKey(nextScope, conversationId), loadedRuns);
        setRuns(loadedRuns);
        setListState(loadedRuns.length ? "ready" : "empty");
      })
      .catch((error: unknown) => {
        if (requestToken !== listRequestTokenRef.current) return;
        setRuns([]);
        setListError(requestErrorText(error));
        setListState("error");
      });
  }, []);

  const loadDetail = useCallback((runId: string) => {
    const requestToken = ++detailRequestTokenRef.current;
    setSelectedRunId(runId);
    setDetail(undefined);
    setDetailError(undefined);
    setDetailState("loading");
    void getAgentRun(runId)
      .then((loadedDetail) => {
        if (requestToken !== detailRequestTokenRef.current) return;
        if (!loadedDetail) {
          setDetailState("missing");
          return;
        }
        setDetail(loadedDetail);
        setDetailState("ready");
      })
      .catch((error: unknown) => {
        if (requestToken !== detailRequestTokenRef.current) return;
        setDetailError(requestErrorText(error));
        setDetailState("error");
      });
  }, []);

  useEffect(() => {
    if (!open) {
      hasOpenedRef.current = false;
      detailIntentRef.current = false;
      observedCurrentConversationRef.current = undefined;
      previousInitialRunIdRef.current = undefined;
      completionRefreshRunRef.current = undefined;
      listRequestTokenRef.current += 1;
      detailRequestTokenRef.current += 1;
      return;
    }

    const isOpening = !hasOpenedRef.current;
    const changedInitialRun = Boolean(initialRunId && initialRunId !== previousInitialRunIdRef.current);
    hasOpenedRef.current = true;
    previousInitialRunIdRef.current = initialRunId;

    if (!isOpening && !changedInitialRun) return;
    if (initialRunId) {
      detailIntentRef.current = true;
      setView("detail");
      loadDetail(initialRunId);
      return;
    }

    detailIntentRef.current = false;
    setView("list");
    setScope("current");
    observedCurrentConversationRef.current = activeConversationId;
    loadList("current", activeConversationId);
  }, [activeConversationId, initialRunId, loadDetail, loadList, open]);

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
      || detail?.status !== "running"
      || finishedRunId !== selectedRunId
      || completionRefreshRunRef.current === selectedRunId
    ) return;
    completionRefreshRunRef.current = selectedRunId;
    loadDetail(selectedRunId);
  }, [detail?.status, finishedRunId, loadDetail, open, selectedRunId, view]);

  useEffect(() => {
    if (open) {
      closeControlRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    } else if (focusWasOpenRef.current) {
      returnFocusElement?.focus();
    }
    focusWasOpenRef.current = open;
  }, [open, returnFocusElement]);

  const selectScope = (nextScope: RunScope) => {
    setScope(nextScope);
    loadList(nextScope, activeConversationId);
  };

  const selectRun = (runId: string) => {
    detailIntentRef.current = true;
    setView("detail");
    loadDetail(runId);
  };

  const returnToList = () => {
    detailIntentRef.current = false;
    setView("list");
    const cachedRuns = listCacheRef.current.get(scopeKey(scope, activeConversationId));
    if (cachedRuns) {
      setRuns(cachedRuns);
      setListState(cachedRuns.length ? "ready" : "empty");
      setListError(undefined);
      return;
    }
    if (scope === "current") observedCurrentConversationRef.current = activeConversationId;
    loadList(scope, activeConversationId);
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
          runs={runs}
          scope={scope}
          state={listState}
          onRetry={() => loadList(scope, activeConversationId)}
          onSelectRun={selectRun}
          onSelectScope={selectScope}
        />
      ) : (
        <RunDetail
          detail={detail}
          error={detailError}
          state={detailState}
          watchlist={watchlist}
          onRetry={() => selectedRunId && loadDetail(selectedRunId)}
          onToggleWatchlist={onToggleWatchlist}
        />
      )}
    </aside>
  );
}

function RunList({
  activeConversationId,
  error,
  runs,
  scope,
  state,
  onRetry,
  onSelectRun,
  onSelectScope,
}: {
  activeConversationId?: string;
  error?: string;
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
      {state === "loading" && <p className="agent-run-state">正在加载运行记录</p>}
      {state === "error" && (
        <div className="agent-run-state agent-run-state-error" role="alert">
          <p>{error}</p>
          <button type="button" className="agent-run-retry" onClick={onRetry}>重试</button>
        </div>
      )}
      {state === "empty" && (
        <p className="agent-run-state">
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
    return <p className="agent-run-state">正在加载运行详情</p>;
  }

  if (state === "missing") {
    return <p className="agent-run-state">本次运行未成功留痕</p>;
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
      {detail.status !== "failed" && detail.result && (
        <section className="agent-run-result" aria-label="运行结果">
          <AgentResultView result={detail.result} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} />
        </section>
      )}
    </section>
  );
}
