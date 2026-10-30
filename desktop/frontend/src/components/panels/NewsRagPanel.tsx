import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import {
  BookOpen, ChevronLeft, ChevronRight, Database, Download, ExternalLink, FileText, Menu,
  MessageSquareText, Plus, RefreshCw, RotateCcw, Search, Send, Upload, X, Trash2,
} from "lucide-react";
import { createPortal } from "react-dom";
import type {
  LlmSettings, NewsRagResult, ResearchAnswer, ResearchCitation,
  ResearchIndexStatus, ResearchMessage, ResearchOverview, ResearchQueryResult,
  ResearchThread, WatchlistItem,
} from "../../types";
import { buildLlmConfig, buildNewsRagRequest, normalizeNewsGroups } from "../../lib/contracts";
import { formatBytes, formatDateTime, normalizeStockCode } from "../../lib/format";
import { getJson, isMobileTauriRuntime, postJson } from "../../lib/tauri";
import { applyMarkRead, pushCitation, useEventSelection } from "../../lib/newsInteractions";
import { LlmSettingsPanel } from "./LlmSettingsPanel";
import { PanelFeedback } from "../ui/PanelFeedback";
import { useMobileComposer } from "../../hooks/useMobileComposer";
import { useMediaQuery } from "../../hooks/useMediaQuery";
import { Sheet } from "../ui/Sheet";
import { ResearchMore } from "./ResearchMore";
import { StockCodeInput } from "../StockCodeInput";
import { sentimentCivilDate } from "../../lib/sentimentView";
import { useWorkspaceState, useWorkspaceStore, useWorkspaceCredentialsReady, WorkspaceSaveStatus } from "../../hooks/useWorkspace";

type LlmSettingsUpdater = LlmSettings | null | ((previous: LlmSettings | null) => LlmSettings | null);
interface NewsRagPanelProps {
  llmSettings?: LlmSettings | null;
  onLlmSettingsChange?: (value: LlmSettingsUpdater) => void;
  watchlist?: WatchlistItem[];
  initialCode?: string;
  initialCodeRequestId?: number;
  code?: string;
  onCodeChange?: (code: string) => void;
  onGoToScreen?: () => void;
  embedded?: boolean;
  active?: boolean;
  onHeaderToolsChange?: (tools: {
    refreshing: boolean;
    vectorReady: boolean;
    onRefresh: () => void;
    onOpenKnowledge: () => void;
    onOpenInbox: () => void;
  }) => void;
}
interface ThreadDetail { answers?: ResearchAnswer[]; }
interface QuestionFailure { title: string; message: string; threadId?: string; }
const EMPTY_WATCHLIST: WatchlistItem[] = [];
const MAX_PDF_FILE_BYTES = 25 * 1024 * 1024;
const MAX_RESEARCH_PACK_BYTES = 64 * 1024 * 1024;

export function NewsRagPanel(props: NewsRagPanelProps) {
  const workspace = useWorkspaceStore();
  const credentialsReady = useWorkspaceCredentialsReady();
  const mobile = useMediaQuery("(max-width: 768px)");
  const nativeMobile = isMobileTauriRuntime();
  const active = props.active !== false;
  const questionInputId = useId();
  const watchlist = props.watchlist || EMPTY_WATCHLIST;
  const controlled = props.onCodeChange !== undefined;
  const [uncontrolledCode, setUncontrolledCode] = useWorkspaceState("news.stock", normalizeStockCode(props.initialCode || watchlist[0]?.code || ""));
  const code = controlled ? normalizeStockCode(props.code || "") : uncontrolledCode;
  const codeRef = useRef(code);
  const [overview, setOverview] = useState<ResearchOverview | null>(null);
  const [messages, setMessages] = useState<ResearchMessage[]>([]);
  const overviewRef = useRef<ResearchOverview | null>(null);
  const messagesRef = useRef<ResearchMessage[]>([]);
  const [threads, setThreads] = useState<ResearchThread[]>([]);
  const [threadId, setThreadId] = useWorkspaceState("news.thread", "");
  const threadIdRef = useRef("");
  const threadLoadRequestRef = useRef(0);
  const deletedThreadIdsRef = useRef(new Set<string>());
  const workspaceGenerationRef = useRef(0);
  const [answers, setAnswers] = useState<ResearchAnswer[]>([]);
  const [question, setQuestion] = useWorkspaceState("news.question", "");
  const [citationStack, setCitationStack] = useState<ResearchCitation[]>([]);
  const [citationPointer, setCitationPointer] = useState(-1);
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState("");
  const [questionError, setQuestionError] = useState<QuestionFailure | null>(null);
  const [inboxOpen, setInboxOpen] = useState(false);
  const [questionsOpen, setQuestionsOpen] = useState(false);
  const [dataStatusOpen, setDataStatusOpen] = useState(false);
  const [modelOpenRequest, setModelOpenRequest] = useState(0);
  const modelReturnFocusRef = useRef<HTMLElement | null>(null);
  const [search, setSearch] = useState(code);
  const questionSurfaceRef = useRef<HTMLDivElement>(null);
  const questionPositionRef = useRef({ scroll: 0, start: 0, end: 0 });
  const citationOriginRef = useRef<HTMLElement | null>(null);
  const evidenceSurfaceRef = useRef<HTMLDivElement>(null);
  const [knowledgeOpen, setKnowledgeOpen] = useState(false);
  const [indexStatus, setIndexStatus] = useState<ResearchIndexStatus | null>(null);
  const [managementBusy, setManagementBusy] = useState(false);
  const [managementResult, setManagementResult] = useState<unknown>(null);
  const [deletingThreadId, setDeletingThreadId] = useState<string | null>(null);
  const [deleteCandidateId, setDeleteCandidateId] = useState<string | null>(null);
  const [readNotice, setReadNotice] = useState("");
  const [readNoticePulse, setReadNoticePulse] = useState(false);
  const [highlightAnswerId, setHighlightAnswerId] = useState<string | null>(null);
  const composer = useMobileComposer(question);
  const questionInputRef = composer.textareaRef;
  const answersRef = useRef<HTMLElement | null>(null);
  const eventRefs = useRef(new Map<string, HTMLButtonElement>());
  const deleteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { selectedId, toggle: toggleEvent, close: closeEvent } = useEventSelection();
  const citation = evidenceOpen && citationPointer >= 0 ? citationStack[citationPointer] || null : null;
  const activeLlmConfig = useMemo(() => buildLlmConfig(props.llmSettings), [props.llmSettings]);

  useEffect(() => { threadIdRef.current = threadId; }, [threadId]);
  const selectCode = useCallback((nextCode: string) => {
    const normalized = normalizeStockCode(nextCode);
    if (normalized === codeRef.current) return;
    codeRef.current = normalized;
    workspaceGenerationRef.current += 1;
    setAsking(false);
    setRefreshing(false);
    setOverview(null);
    overviewRef.current = null;
    setMessages([]);
    messagesRef.current = [];
    setThreadId("");
    threadIdRef.current = "";
    setAnswers([]);
    closeEvent();
    setCitationStack([]);
    setCitationPointer(-1);
    setEvidenceOpen(false);
    setError("");
    setQuestionError(null);
    setLoading(true);
    if (controlled) props.onCodeChange?.(normalized);
    else setUncontrolledCode(normalized);
  }, [closeEvent, controlled, props.onCodeChange, setThreadId, setUncontrolledCode]);
  useEffect(() => { if (codeRef.current !== code) selectCode(code); setSearch(code); }, [code, selectCode]);
  useEffect(() => {
    if (controlled) return;
    const next = normalizeStockCode(props.initialCode);
    if (next) selectCode(next);
  }, [controlled, props.initialCode, props.initialCodeRequestId, selectCode]);

  const loadThread = useCallback(async (
    id: string,
    generation = workspaceGenerationRef.current,
  ) => {
    if (!id) {
      if (generation === workspaceGenerationRef.current && !threadIdRef.current) setAnswers([]);
      return;
    }
    if (generation !== workspaceGenerationRef.current || threadIdRef.current !== id) return;
    const request = ++threadLoadRequestRef.current;
    const ownsRequest = () => generation === workspaceGenerationRef.current
      && threadIdRef.current === id && threadLoadRequestRef.current === request;
    setQuestionError((current) => current?.threadId === id ? null : current);
    try {
      const detail = await postJson<ThreadDetail>("/api/research/threads/detail", { thread_id: id });
      if (!ownsRequest()) return;
      setAnswers(detail.answers || []);
      setQuestionError((current) => current?.threadId === id ? null : current);
    } catch (nextError) {
      if (ownsRequest()) {
        setQuestionError((current) => current && current.threadId !== id ? current : {
          title: "会话加载失败", message: (nextError as Error).message, threadId: id,
        });
      }
    }
  }, []);

  const loadWorkspace = useCallback(async (quiet = false) => {
    const generation = ++workspaceGenerationRef.current;
    if (!quiet) setLoading(true);
    setError("");
    try {
      await workspace.initialize();
      if (generation !== workspaceGenerationRef.current) return;
      const messageQuery = code
        ? `?stock_code=${encodeURIComponent(code)}&limit=120`
        : "?limit=120";
      const [nextOverview, messageResult, threadResult] = await Promise.all([
        getJson<ResearchOverview>("/api/research/overview"),
        getJson<{ items?: ResearchMessage[] }>(`/api/research/messages${messageQuery}`),
        getJson<{ items?: ResearchThread[] }>("/api/research/threads"),
      ]);
      if (generation !== workspaceGenerationRef.current) return;
      const nextThreads = (threadResult.items || []).filter(
        (thread) => !deletedThreadIdsRef.current.has(thread.id),
      );
      overviewRef.current = nextOverview;
      messagesRef.current = messageResult.items || [];
      setOverview(nextOverview);
      setMessages(messagesRef.current);
      setThreads(nextThreads);
      const preferred = nextThreads.find((item) =>
        item.id === workspace.get("news.thread", threadIdRef.current) && (item.stock_code || "") === code)
        || nextThreads.find((item) => item.stock_code === code)
        || (!code ? nextThreads.find((item) => !item.stock_code) : undefined);
      if (preferred) {
        threadIdRef.current = preferred.id;
        setThreadId(preferred.id);
        await loadThread(preferred.id, generation);
      } else {
        threadIdRef.current = "";
        setThreadId("");
        setAnswers([]);
      }
    } catch (nextError) {
      if (generation === workspaceGenerationRef.current) {
        setError((nextError as Error).message);
      }
    } finally {
      if (!quiet && generation === workspaceGenerationRef.current) setLoading(false);
    }
  }, [code, loadThread, setThreadId, workspace]);

  useEffect(() => {
    void loadWorkspace();
    return () => { workspaceGenerationRef.current += 1; };
  }, [loadWorkspace]);

  const refresh = useCallback(async (background = false) => {
    const generation = workspaceGenerationRef.current;
    if (!code || !navigator.onLine || document.visibilityState !== "visible") {
      if (!background) await loadWorkspace();
      return;
    }
    if (!background) setRefreshing(true);
    try {
      await postJson("/api/research/refresh", buildNewsRagRequest(code, 30, undefined, watchlist));
      if (generation !== workspaceGenerationRef.current) return;
      await loadWorkspace(true);
    } catch (nextError) {
      if (!background && generation === workspaceGenerationRef.current) {
        setError((nextError as Error).message);
      }
    } finally {
      if (!background && generation === workspaceGenerationRef.current) setRefreshing(false);
    }
  }, [code, loadWorkspace, watchlist]);

  const visibleMessages = useMemo(
    () => code ? messages.filter((message) => message.stock_code === code) : messages,
    [code, messages],
  );
  const todayMessages = useMemo(
    () => visibleMessages.filter((message) => isToday(message.published_at)),
    [visibleMessages],
  );
  const grouped = useMemo(() => ({
    positive: todayMessages.filter((item) => ["positive", "bullish", "利好"].includes(item.sentiment)),
    negative: todayMessages.filter((item) => ["negative", "bearish", "利空"].includes(item.sentiment)),
    uncertain: todayMessages.filter((item) =>
      !["positive", "bullish", "利好", "negative", "bearish", "利空"].includes(item.sentiment)),
  }), [todayMessages]);
  const eventGroups = useMemo(() => groupMessages(visibleMessages), [visibleMessages]);
  const stock = watchlist.find((item) => normalizeStockCode(item.code) === code);
  const questionPlaceholder = code
    ? `询问 ${code} 的公告、财务或消息…`
    : "询问当前知识库…";
  const composerPlaceholder = mobile ? "询问公告、财务或消息…" : `${questionPlaceholder} · Ctrl+Enter 发送`;
  const summary = useMemo(() => {
    if (!todayMessages.length) return "今日暂无新增消息，可阅读已有资料。";
    const direction = grouped.positive.length > grouped.negative.length ? "利好线索较多"
      : grouped.negative.length > grouped.positive.length ? "风险线索较多" : "多空线索接近";
    return `${direction}，仍需结合公告和财务资料核查。`;
  }, [code, grouped, todayMessages.length]);

  const markReadBatch = useCallback(async (ids: string[], notice = false) => {
    const uniqueIds = [...new Set(ids)].filter(Boolean);
    if (!uniqueIds.length) return;
    try {
      await postJson("/api/research/mark-read", { message_ids: uniqueIds });
      const result = applyMarkRead(messagesRef.current, overviewRef.current, uniqueIds);
      messagesRef.current = result.messages;
      overviewRef.current = result.overview;
      setMessages(result.messages);
      setOverview(result.overview);
      if (notice) {
        setReadNotice("已全部标为已读");
        setReadNoticePulse(true);
        if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
        noticeTimerRef.current = setTimeout(() => {
          setReadNotice("");
          setReadNoticePulse(false);
        }, 2000);
      }
    } catch (nextError) {
      setError((nextError as Error).message);
    }
  }, []);

  const markReadStock = useCallback(async (stockCode: string) => {
    const normalizedStockCode = normalizeStockCode(stockCode);
    const ids = messagesRef.current.filter((message) => normalizeStockCode(message.stock_code) === normalizedStockCode && message.unread)
      .map((message) => message.id);
    const unreadCount = overviewRef.current?.unread_by_stock?.[normalizedStockCode] || 0;
    if (!ids.length && unreadCount <= 0) return;
    try {
      await postJson("/api/research/mark-read", { stock_code: normalizedStockCode });
      const nextMessages = messagesRef.current.map((message) =>
        ids.includes(message.id) ? { ...message, unread: false } : message,
      );
      const currentOverview = overviewRef.current;
      const nextOverview = currentOverview ? (() => {
        const unreadByStock = { ...(currentOverview.unread_by_stock || {}) };
        const changed = Math.max(unreadCount, ids.length);
        unreadByStock[normalizedStockCode] = 0;
        return {
          ...currentOverview,
          unread_count: Math.max(0, currentOverview.unread_count - changed),
          unread_by_stock: unreadByStock,
        };
      })() : currentOverview;
      messagesRef.current = nextMessages;
      overviewRef.current = nextOverview;
      setMessages(nextMessages);
      setOverview(nextOverview);
    } catch (nextError) {
      setError((nextError as Error).message);
    }
  }, []);

  const rememberQuestions = useCallback(() => {
    const input = questionInputRef.current;
    if (!input || !questionSurfaceRef.current) return;
    questionPositionRef.current = { scroll: questionSurfaceRef.current?.scrollTop || 0, start: input?.selectionStart || 0, end: input?.selectionEnd || 0 };
  }, [questionInputRef]);
  const closeQuestions = useCallback(() => { rememberQuestions(); setQuestionsOpen(false); }, [rememberQuestions]);
  const pushCitationSelection = useCallback((next: ResearchCitation) => {
    setInboxOpen(false);
    if (!citation) {
      citationOriginRef.current = document.activeElement as HTMLElement | null;
      if (questionsOpen) rememberQuestions();
    }
    const result = pushCitation(citationStack, citationPointer, next);
    setCitationStack(result.stack);
    setCitationPointer(result.pointer);
    setEvidenceOpen(true);
  }, [citation, citationPointer, citationStack, questionsOpen, rememberQuestions]);

  const closeCitation = useCallback(() => {
    setEvidenceOpen(false);
    queueMicrotask(() => citationOriginRef.current?.focus?.());
  }, []);
  useEffect(() => {
    if (!active) return;
    if (citation) evidenceSurfaceRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    else if (questionsOpen) {
      const position = questionPositionRef.current;
      if (questionSurfaceRef.current) questionSurfaceRef.current.scrollTop = position.scroll;
      questionInputRef.current?.setSelectionRange?.(position.start, position.end);
      const surface = questionSurfaceRef.current;
      const input = questionInputRef.current;
      return () => {
        if (surface && input) questionPositionRef.current = { scroll: surface.scrollTop || 0, start: input.selectionStart || 0, end: input.selectionEnd || 0 };
      };
    }
  }, [active, citation, questionsOpen, questionInputRef]);

  const selectEvent = useCallback((message: ResearchMessage) => {
    const wasSelected = selectedId === message.id;
    toggleEvent(message.id);
    if (wasSelected) return;
    if (message.unread) void markReadBatch([message.id]);
  }, [markReadBatch, selectedId, toggleEvent]);

  useEffect(() => {
    if (!selectedId) return;
    const element = eventRefs.current.get(selectedId);
    if (!element || typeof element.scrollIntoView !== "function") return;
    const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    element.scrollIntoView({ block: "nearest", behavior: reducedMotion ? "auto" : "smooth" });
  }, [selectedId]);

  useEffect(() => {
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (citation || questionsOpen || inboxOpen || knowledgeOpen || event.defaultPrevented) return;
      if (selectedId) closeEvent();
    };
    window.addEventListener?.("keydown", handleEscape);
    return () => {
      window.removeEventListener?.("keydown", handleEscape);
    };
  }, [citation, questionsOpen, inboxOpen, knowledgeOpen, closeEvent, selectedId]);

  useEffect(() => {
    return () => {
      if (deleteTimerRef.current) clearTimeout(deleteTimerRef.current);
      if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
    };
  }, []);

  const createThread = useCallback(async (
    generation = workspaceGenerationRef.current,
    requestedCode = code,
  ): Promise<string | null> => {
    const thread = await postJson<ResearchThread>("/api/research/threads/create", {
      title: requestedCode ? `${requestedCode} 研究` : "综合研究",
      stock_code: requestedCode || null,
    });
    if (generation !== workspaceGenerationRef.current) return null;
    threadIdRef.current = thread.id;
    threadLoadRequestRef.current += 1;
    setQuestionError(null);
    setThreads((current) => [thread, ...current]);
    setThreadId(thread.id);
    setAnswers([]);
    closeEvent();
    setInboxOpen(false);
    queueMicrotask(() => questionInputRef.current?.focus());
    return thread.id;
  }, [code, closeEvent]);

  const createQuestionThread = useCallback(async () => {
    const generation = workspaceGenerationRef.current;
    setQuestionError(null);
    try { await createThread(generation); }
    catch (nextError) {
      if (generation === workspaceGenerationRef.current) setQuestionError({ title: "新建会话失败", message: (nextError as Error).message });
    }
  }, [createThread]);

  const deleteThread = useCallback(async (thread: ResearchThread) => {
    if (asking || deletingThreadId) return;
    if (deleteCandidateId !== thread.id) {
      setDeleteCandidateId(thread.id);
      if (deleteTimerRef.current) clearTimeout(deleteTimerRef.current);
      deleteTimerRef.current = setTimeout(() => setDeleteCandidateId(null), 3000);
      return;
    }
    setDeleteCandidateId(null);
    const generation = workspaceGenerationRef.current;
    setDeletingThreadId(thread.id);
    setQuestionError(null);
    try {
      await postJson("/api/research/threads/delete", { thread_id: thread.id });
      deletedThreadIdsRef.current.add(thread.id);
      setThreads((current) => current.filter((item) => item.id !== thread.id));
      if (threadIdRef.current === thread.id) {
        threadIdRef.current = "";
        setThreadId("");
        setAnswers([]);
      }
      if (generation === workspaceGenerationRef.current) await loadWorkspace(true);
    } catch (nextError) {
      if (generation === workspaceGenerationRef.current) setQuestionError({ title: "删除会话失败", message: (nextError as Error).message });
    } finally {
      setDeletingThreadId(null);
    }
  }, [asking, deleteCandidateId, deletingThreadId, loadWorkspace]);

  const ask = useCallback(async () => {
    const text = question.trim();
    if (!text || asking || deletingThreadId || !credentialsReady) return;
    const generation = workspaceGenerationRef.current;
    const requestedCode = code;
    setAsking(true);
    setQuestionError(null);
    let failureTitle = "新建会话失败";
    try {
      const selectedThread = threads.find((thread) => thread.id === threadId);
      const activeThread = selectedThread && (selectedThread.stock_code || "") === requestedCode
        ? selectedThread.id
        : await createThread(generation, requestedCode);
      if (!activeThread || generation !== workspaceGenerationRef.current) return;
      failureTitle = "提问失败";
      const result = await postJson<ResearchQueryResult>("/api/research/query", {
        query: text, stock_code: requestedCode || null, thread_id: activeThread, top_k: 8,
        llm: activeLlmConfig,
      });
      if (generation !== workspaceGenerationRef.current) return;
      const answer: ResearchAnswer = { ...result, question: text, citations: result.citations || [] };
      const answerKey = answer.id || `${answer.question}-${answers.length}`;
      setAnswers((current) => [...current, answer]);
      setQuestion(current => current === question ? "" : current);
      void workspace.flush();
      setHighlightAnswerId(answerKey);
      if (answer.citations[0]) pushCitationSelection(answer.citations[0]);
      if (mobile) setInboxOpen(false);
      failureTitle = "会话列表更新失败";
      const nextThreads = await getJson<{ items?: ResearchThread[] }>("/api/research/threads");
      if (generation === workspaceGenerationRef.current) {
        setThreads((nextThreads.items || []).filter(
          (thread) => !deletedThreadIdsRef.current.has(thread.id),
        ));
      }
    } catch (nextError) {
      if (generation === workspaceGenerationRef.current) setQuestionError({ title: failureTitle, message: (nextError as Error).message });
    } finally {
      if (generation === workspaceGenerationRef.current) setAsking(false);
    }
  }, [activeLlmConfig, answers.length, asking, code, createThread, deletingThreadId, mobile, pushCitationSelection, question, threadId, threads, credentialsReady, setQuestion, workspace]);

  useEffect(() => {
    if (!highlightAnswerId) return;
    const element = answersRef.current;
    if (element && typeof element.scrollIntoView === "function") {
      const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
      element.scrollIntoView({ block: "end", behavior: reducedMotion ? "auto" : "smooth" });
    }
    const timer = setTimeout(() => setHighlightAnswerId(null), 600);
    return () => clearTimeout(timer);
  }, [answers.length, highlightAnswerId]);

  const management = useCallback(async (action: () => Promise<unknown>) => {
    setManagementBusy(true);
    setManagementResult(null);
    try {
      setManagementResult(await action());
      setIndexStatus(await getJson<ResearchIndexStatus>("/api/research/index-status"));
      await loadWorkspace(true);
    } catch (nextError) {
      setManagementResult({ error: (nextError as Error).message });
    } finally {
      setManagementBusy(false);
    }
  }, [loadWorkspace]);

  const openKnowledge = useCallback(() => {
    setKnowledgeOpen(true);
    void getJson<ResearchIndexStatus>("/api/research/index-status")
      .then(setIndexStatus)
      .catch((nextError) => setManagementResult({ error: (nextError as Error).message }));
  }, []);
  const vectorReady = overview?.retrieval?.vector?.ready === true;
  const onHeaderRefresh = useCallback(() => { void refresh(); }, [refresh]);
  const onHeaderInbox = useCallback(() => { closeCitation(); setInboxOpen(true); }, [closeCitation]);
  useEffect(() => {
    if (!props.embedded || !props.onHeaderToolsChange) return;
    props.onHeaderToolsChange({
      refreshing,
      vectorReady,
      onRefresh: onHeaderRefresh,
      onOpenKnowledge: openKnowledge,
      onOpenInbox: onHeaderInbox,
    });
  }, [onHeaderInbox, onHeaderRefresh, openKnowledge, props.embedded, props.onHeaderToolsChange, refreshing, vectorReady]);

  const unreadVisibleIds = visibleMessages.filter((message) => message.unread).map((message) => message.id);
  const lastUpdated = formatResearchUpdatedAt(overview);

  return <section className="research-workspace" aria-label="研究消息中心">
    {loading && !overview && <p className="research-loading-status" role="status">正在打开研究消息中心，可继续编辑草稿…</p>}
    {!props.embedded && <header className="research-topbar">
      <form className="sentiment-toolbar" onSubmit={(event) => { event.preventDefault(); const next = normalizeStockCode(search); if (/^\d{6}\.(SH|SZ|BJ)$/.test(next)) selectCode(next); else setError("请输入六位股票代码，或从搜索结果中选择股票。"); }}>
        <StockCodeInput id={`${questionInputId}-stock`} value={search} onChange={setSearch} onCommit={selectCode} placeholder="代码或名称" inputAriaLabel="股票代码或名称" />
        <button type="submit" className="sentiment-icon-action" aria-label="查看股票消息"><Search size={16} /></button>
      </form>
      <div className="research-actions">
        <button type="button" className="research-mobile-stock-button" aria-label="打开股票列表" onClick={() => setInboxOpen(true)}><Menu size={16} /><span>股票</span></button>
        <button type="button" onClick={() => void refresh()} disabled={refreshing}><RefreshCw size={15} /><span>{refreshing ? "更新中" : "更新"}</span></button>
        <ResearchMore onKnowledge={openKnowledge} onModel={() => { modelReturnFocusRef.current = document.activeElement as HTMLElement | null; setModelOpenRequest((value) => value + 1); }} onStatus={() => setDataStatusOpen(true)} modelDisabled={!credentialsReady} />
        <div className="research-model-dialog-host"><LlmSettingsPanel settings={props.llmSettings || null}
          onChange={credentialsReady ? props.onLlmSettingsChange || (() => undefined) : () => undefined} presentation="dialog" statusMode="inline" openRequest={modelOpenRequest} returnFocusRef={modelReturnFocusRef} /></div>
      </div>
    </header>}

    {error && <div className="research-error">
      <PanelFeedback kind="error" title="研究中心暂时不可用" description={error} />
    </div>}

    <div className="research-columns">
      <ResearchSurfaceSheet active={mobile} open={active && inboxOpen} close={()=>setInboxOpen(false)} label="自选股收件箱">
      <InboxPanel code={code} setCode={(next) => { selectCode(next); setInboxOpen(false); }}
        watchlist={watchlist} unreadByStock={overview?.unread_by_stock || {}}
        unread={overview?.unread_count || 0} open={inboxOpen} close={() => setInboxOpen(false)} />
      </ResearchSurfaceSheet>

      <main className="research-stream">
        <div className="research-stream-body">
        <div className="research-reading-heading">
          <h1>{stock?.name || (code ? "股票消息" : "全部消息")}{code && <small>{code}</small>}</h1>
          <button type="button" className="research-ghost-button" aria-label="打开研究问答" aria-expanded={questionsOpen}
            onClick={() => { setInboxOpen(false); setQuestionsOpen(true); }}><MessageSquareText size={16} aria-hidden="true" />研究问答</button>
        </div>
        {visibleMessages.length > 0 && <details className="research-daily-brief">
          <summary className="research-brief-summary"><span>{summary}</span><strong>今日新增 {todayMessages.length} 条</strong><ChevronRight className="research-brief-chevron" size={16} aria-hidden="true" /></summary>
          <div className="research-brief-expanded"><ResearchBriefCounts positive={grouped.positive.length} negative={grouped.negative.length} uncertain={grouped.uncertain.length} />
            <p>{lastUpdated ? `上次更新 ${lastUpdated}` : "尚未记录更新时间"} · 每 15 分钟自动更新</p>
          </div>
        </details>}

        <section className="research-event-section">
          <div className="research-section-heading"><div><span>消息</span></div><div className="research-heading-actions">
              {readNotice && <small className={readNoticePulse ? "is-flashing" : ""}>{readNotice}</small>}
              {(unreadVisibleIds.length > 0 || (code && (overview?.unread_by_stock?.[code] || 0) > 0)) && <button type="button" className="research-ghost-button"
                onClick={() => code ? void markReadStock(code) : void markReadBatch(unreadVisibleIds, true)}>全部已读</button>}
              <span>{visibleMessages.length} 条</span>
            </div>
          </div>
          {loading ? null : !visibleMessages.length ? <ResearchEmptyState
            refreshing={refreshing}
            onRefresh={() => void refresh()}
            code={code}
            needsWatchlist={!code && watchlist.length === 0}
            onPickStock={() => mobile ? setInboxOpen(true) : selectCode(watchlist[0]?.code || "")}
            onGoToScreen={props.onGoToScreen}
          /> : <div className="research-event-list">
            {eventGroups.map((group) => <EventGroup key={group.key}
              label={group.label} tone={group.key} messages={group.messages}
              selectedId={selectedId} selectEvent={selectEvent}
              setEventRef={(id, element) => {
                if (element) eventRefs.current.set(id, element);
                else eventRefs.current.delete(id);
              }} pushCitation={pushCitationSelection} />)}
          </div>}
        </section>

        <p className="research-risk-boundary">仅供研究，不构成投资建议。</p>
        </div>
      </main>

      <Sheet open={active && (questionsOpen || Boolean(citation))} onClose={citation ? closeCitation : closeQuestions}
        label={citation ? "引用证据检查器" : "研究问答"} className="research-surface-sheet" backdropClassName="research-surface-backdrop">
        {questionsOpen && <div className="research-question-surface" hidden={Boolean(citation)} ref={questionSurfaceRef}>
          <div className="research-detail-heading"><h2>研究问答</h2><button type="button" className="research-icon-button" aria-label="关闭研究问答" onClick={closeQuestions}><X size={18} /></button></div>
          <ResearchThreads threads={threads.filter((item) => (item.stock_code || "") === code)} threadId={threadId} asking={asking} deletingThreadId={deletingThreadId} deleteCandidateId={deleteCandidateId}
            createThread={() => void createQuestionThread()} deleteThread={(thread) => void deleteThread(thread)}
            setThread={(thread) => { const generation = ++workspaceGenerationRef.current; setQuestionError(null); setAsking(false); setThreadId(thread.id); threadIdRef.current = thread.id; setAnswers([]); void workspace.flush(); void loadThread(thread.id, generation); }} />
          {questionError && <PanelFeedback kind="error" title={questionError.title} description={questionError.message}
            action={questionError.threadId ? <button type="button" className="research-ghost-button" onClick={() => void loadThread(questionError.threadId!, workspaceGenerationRef.current)}>重试加载会话</button> : undefined} />}
          <Answers answers={answers} pushCitation={pushCitationSelection} highlightedId={highlightAnswerId} sectionRef={answersRef} />
        <form className="research-composer" onSubmit={(event) => { event.preventDefault(); void ask(); }}>
          <div><label className="research-composer-label" htmlFor={questionInputId}>
            <span>研究问题</span>{activeLlmConfig && <small>模型回答会强制引用证据</small>}
          </label>
            <div className="research-composer-row">
              <textarea ref={questionInputRef} id={questionInputId} value={question}
                aria-label="研究问题"
                onChange={(event) => setQuestion(event.target.value)}
                onFocus={composer.onFocus} onBlur={composer.onBlur}
                onCompositionStart={composer.onCompositionStart} onCompositionEnd={composer.onCompositionEnd}
                onKeyDown={(event) => {
                  if (!composer.isComposing(event) && event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
                    event.preventDefault();
                    void ask();
                  }
                }}
                placeholder={composerPlaceholder} rows={2} />
              <button className="research-composer-send" type="submit"
                aria-label="提交问题；仅供研究，不构成投资建议。"
                title="提交问题；仅供研究，不构成投资建议。"
                disabled={!credentialsReady || !question.trim() || asking || Boolean(deletingThreadId)}>
                {asking || deletingThreadId
                  ? <RefreshCw size={18} className="is-spinning" /> : <Send size={18} />}
              </button>
            </div>
          </div>
          <WorkspaceSaveStatus />
          <div className="research-composer-meta">
            <p className="research-risk-boundary">仅供研究，不构成投资建议。</p>
            {!activeLlmConfig && <p className="research-composer-setup">未配置模型，将只用已导入资料回答</p>}
          </div>
        </form>

        </div>}
        {citation && <div className="research-evidence-surface" ref={evidenceSurfaceRef}>
          <button type="button" className="research-ghost-button research-evidence-back" aria-label={questionsOpen ? "返回研究问答" : "返回消息"} onClick={closeCitation}><ChevronLeft size={16} />{questionsOpen ? "返回研究问答" : "返回消息"}</button>
          <EvidencePanel citation={citation} citationIndex={citationPointer}
            citationCount={citationStack.length} onPrevious={() => setCitationPointer((value) => Math.max(0, value - 1))}
            onNext={() => setCitationPointer((value) => Math.min(citationStack.length - 1, value + 1))} close={closeCitation} />
        </div>}
      </Sheet>
    </div>

    <Sheet open={active && dataStatusOpen} onClose={() => setDataStatusOpen(false)} label="研究数据状态" className="research-surface-sheet" backdropClassName="research-surface-backdrop">
      <div className="research-detail-heading"><h2>数据状态</h2><button type="button" className="research-icon-button" aria-label="关闭数据状态" onClick={() => setDataStatusOpen(false)}><X size={18} /></button></div>
      <p>{vectorReady ? "关键词 + 语义检索" : "关键词检索"}</p><p>{lastUpdated ? `上次更新 ${lastUpdated}` : "尚未记录更新时间"}</p>
    </Sheet>
    {knowledgeOpen && <ResearchPortal><KnowledgeDrawer code={code} mobile={nativeMobile}
      status={indexStatus} management={management} busy={managementBusy}
      result={managementResult} close={() => setKnowledgeOpen(false)} /></ResearchPortal>}
  </section>;
}
function ResearchPortal({ children }: { children: ReactNode }) {
  return document.body ? createPortal(children, document.body) : children;
}
function ResearchSurfaceSheet({active,open,close,label,children}: {active:boolean;open:boolean;close:()=>void;label:string;children:ReactNode}) {
  if (!active) return children;
  return <Sheet open={open} onClose={close} label={label} className="research-surface-sheet" backdropClassName="research-surface-backdrop">{children}</Sheet>;
}
function EventGroup(props: {
  label: string;
  tone: string;
  messages: ResearchMessage[];
  selectedId: string | null;
  selectEvent: (message: ResearchMessage) => void;
  setEventRef: (id: string, element: HTMLButtonElement | null) => void;
  pushCitation: (citation: ResearchCitation) => void;
}) {
  if (!props.messages.length) return null;
  return <section className={`research-event-group ${props.tone}`}>
    <header><span>{props.label}</span><b>{props.messages.length}</b></header>
    {props.messages.map((message) => {
      const selected = props.selectedId === message.id;
      return <div className={`research-event-shell${selected ? " selected expanded" : ""}`} key={message.id}>
        <button type="button" ref={(element) => props.setEventRef(message.id, element)}
          aria-expanded={selected}
          className={`research-event ${sentimentClass(message.sentiment)}${message.unread ? " unread" : ""}${selected ? " selected" : ""}`}
          onClick={() => props.selectEvent(message)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && selected) {
              event.preventDefault();
              props.selectEvent(message);
            }
          }}>
          <span className={`research-dot research-event-dot-desktop ${message.unread ? "unread" : "neutral"}`} aria-label={message.unread ? "未读" : undefined} />
          <span className="research-event-content">
            <strong>{message.title}</strong>
            <span className="research-event-summary">{message.summary}</span>
            <span className="research-event-meta">
              <span className="research-source-name">{message.source_name}</span>
              <time>{formatDateTime(message.published_at)}</time>
              {message.unread && <span className="research-unread-label">未读</span>}
            </span>
          </span>
        </button>
        {selected && <div className="research-event-detail">
          <p>{message.summary}</p>
          <div className="research-event-detail-meta">
            <span className="research-badge">{sourceTierLabel(message.source_tier)}</span>
            {message.scope_type && <span className="research-badge">{scopeTypeLabel(message.scope_type)}</span>}
            {message.scope_tags?.map((tag) => <span className="research-badge" key={tag}>{tag}</span>)}
          </div>
          {message.citations?.length ? <div className="research-event-detail-evidence">
            <span>已关联 {message.citations.length} 条证据</span>
            <div className="research-event-citation-links">
              {message.citations.map((item) => <button type="button"
                key={item.citation_id} onClick={() => props.pushCitation(item)}>
                查看 {item.citation_id}
              </button>)}
            </div>
          </div> : null}
        </div>}
      </div>;
    })}
  </section>;
}

function ResearchEmptyState(props: { refreshing: boolean; onRefresh: () => void; code: string; needsWatchlist?: boolean; onGoToScreen?: () => void; onPickStock: () => void; }) {
  return <div className="research-empty"><BookOpen size={28} /><strong>{props.code ? "当前股票暂无消息" : "暂无消息"}</strong>
    <p>{props.code ? "更新后查看这只股票的最新消息。" : props.needsWatchlist ? "先去选股收藏，或搜索股票代码。" : "选择股票后更新消息。"}</p>
    <div className="research-empty-actions"><button type="button" className="research-empty-primary" disabled={props.refreshing}
      onClick={props.code ? props.onRefresh : props.needsWatchlist ? props.onGoToScreen : props.onPickStock}>
      {props.code && <RefreshCw size={15} />}<span>{props.code ? props.refreshing ? "更新中" : "更新" : props.needsWatchlist ? "去选股收藏" : "选择股票"}</span>
    </button></div>
  </div>;
}

function ResearchBriefCounts(props: {
  positive: number;
  negative: number;
  uncertain: number;
  inline?: boolean;
}) {
  const hasBriefStats = props.positive > 0 || props.negative > 0
    || props.uncertain > 0;
  if (!hasBriefStats) return null;
  return <span className={`research-brief-counts${props.inline ? " research-brief-inline-counts" : ""}`}>
    <span className="research-stat positive"><span>利好</span><strong>{props.positive}</strong></span>
    <span className="research-stat negative"><span>利空</span><strong>{props.negative}</strong></span>
    <span className="research-stat"><span>待核查</span><strong>{props.uncertain}</strong></span>
  </span>;
}

function InboxPanel(props: {
  code: string;
  setCode: (value: string) => void;
  watchlist: WatchlistItem[];
  unreadByStock: Record<string, number>;
  unread: number;
  open: boolean;
  close: () => void;
}) {
  return <aside className={`research-inbox${props.open ? " mobile-open" : ""}`}>
    <span className="research-sheet-handle" aria-hidden="true" />
    <div className="research-pane-heading">
      <div><strong>自选股票</strong></div>
      <button className="research-icon-button research-mobile-close" type="button"
        aria-label="关闭收件箱" title="关闭" onClick={props.close}><X size={17} /></button>
    </div>
    <button type="button" className={`research-stock-row${!props.code ? " active" : ""}`}
      onClick={() => props.setCode("")}>
      <span><strong>全部消息</strong></span>
      {props.unread > 0 && <b>{props.unread}</b>}
    </button>
    {props.watchlist.length === 0 && <p className="research-inbox-empty-hint">暂无自选股票。</p>}
    {props.watchlist.map((item) => {
      const stockCode = normalizeStockCode(item.code);
      const unread = props.unreadByStock[stockCode] || 0;
      return <div className={`research-stock-row-wrap${props.code === stockCode ? " active" : ""}`} key={stockCode}>
      <button type="button"
        className={`research-stock-row${props.code === stockCode ? " active" : ""}`}
        onClick={() => props.setCode(stockCode)}>
        <span><strong>{item.name || stockCode}</strong><small>{stockCode}</small></span>
        {unread > 0 && <b>{unread}</b>}
      </button>
      </div>;
    })}

  </aside>;
}

function ResearchThreads(props: {
  threads: ResearchThread[]; threadId: string; setThread: (thread: ResearchThread) => void; createThread: () => void;
  deleteThread: (thread: ResearchThread) => void; deletingThreadId: string | null; deleteCandidateId: string | null; asking: boolean;
}) {
  return     <div className="research-inbox-section">
      <div className="research-pane-heading compact"><span>研究会话</span>
        <button className="research-icon-button" type="button" aria-label="新建研究会话"
          title="新建会话" disabled={props.asking || Boolean(props.deletingThreadId)} onClick={props.createThread}><Plus size={16} /></button>
      </div>
      {props.threads.slice(0, 20).map((thread) => <div key={thread.id}
        className={`research-thread-row${thread.id === props.threadId ? " active" : ""}`}>
        <button type="button" className="research-thread-main"
          disabled={Boolean(props.deletingThreadId)} onClick={() => props.setThread(thread)}>
          <MessageSquareText size={15} aria-hidden="true" />
          <span><strong>{thread.title}</strong><small>{formatEpoch(thread.updated_at_epoch_ms)}</small></span>
        </button>
        <button type="button" className={`research-thread-delete${props.deleteCandidateId === thread.id ? " confirm" : ""}`}
          aria-label={`删除研究会话：${thread.title || "未命名会话"}`}
          title="删除会话" disabled={props.asking || props.deletingThreadId === thread.id}
          onClick={() => props.deleteThread(thread)}>
          {props.deleteCandidateId === thread.id ? "确认删除？" : <Trash2 size={14} aria-hidden="true" />}
        </button>
      </div>)}
    </div>;
}

function Answers(props: {
  answers: ResearchAnswer[];
  pushCitation: (value: ResearchCitation) => void;
  highlightedId: string | null;
  sectionRef: React.MutableRefObject<HTMLElement | null>;
}) {
  if (!props.answers.length) return null;
  return <section className="research-answers">
    <div className="research-section-heading"><div><span>历史问答</span>
      <small>回答与引用均保存在本机</small></div>
    </div>
    {props.answers.map((answer, index) => <article
      key={answer.id || answer.question + index}
      ref={index === props.answers.length - 1 ? props.sectionRef : undefined}
      className={`research-answer${props.highlightedId === (answer.id || `${answer.question}-${index}`) ? " is-highlighted" : ""}`}>
      <div className="research-question"><span>问</span><p>{answer.question}</p></div>
      <div className="research-answer-body">
        <div className="research-answer-mode">
          {answer.mode === "model" ? "模型综合" : "证据摘录"}
        </div>
        <p><CitationRichText text={answer.answer} citations={answer.citations}
          onCitation={props.pushCitation} /></p>
        <div className="research-citation-chips">
          {answer.citations.map((item) => <button type="button"
            key={item.citation_id + index} onClick={() => props.pushCitation(item)}>
            <b>{item.citation_id}</b><span>{item.title}</span>
          </button>)}
        </div>
        {(answer.model_warning || answer.vector_warning) &&
          <small className="research-fallback-note">
            已自动降级：{answer.model_warning || answer.vector_warning}
          </small>}
      </div>
    </article>)}
  </section>;
}

function EvidencePanel(props: {
  citation: ResearchCitation | null;
  citationIndex: number;
  citationCount: number;
  onPrevious: () => void;
  onNext: () => void;
  close: () => void;
}) {
  return <aside className={`research-evidence${props.citation ? " has-selection" : ""}`}
    aria-label="引用证据检查器">
    <span className="research-sheet-handle" aria-hidden="true" />
    <div className="research-pane-heading">
      <div><span>证据检查器</span><strong>原文可回溯</strong></div>
      {props.citation && <div className="research-citation-history" aria-label="引用历史">
        <button type="button" aria-label="上一条证据" title="上一条证据"
          disabled={props.citationIndex <= 0} onClick={props.onPrevious}><ChevronLeft size={15} /></button>
        <span>{props.citationIndex + 1} / {props.citationCount}</span>
        <button type="button" aria-label="下一条证据" title="下一条证据"
          disabled={props.citationIndex >= props.citationCount - 1} onClick={props.onNext}><ChevronRight size={15} /></button>
      </div>}
      <button className="research-icon-button research-evidence-close" type="button"
        aria-label="关闭证据检查器" title="关闭" onClick={props.close}><X size={17} /></button>
    </div>
    {props.citation ? <EvidenceInspector citation={props.citation} />
      : <div className="research-evidence-empty">
        <span className="research-evidence-empty-icon"><Search size={18} /></span>
        <div><strong>证据会在这里展开</strong>
          <p>完成一次证据问答，再点击回答中的引用编号查看原文。</p></div>
        <ol aria-label="证据检查步骤">
          <li><span>1</span><div><strong>定位引用</strong><p>先点回答中的引用编号。</p></div></li>
          <li><span>2</span><div><strong>核对原文</strong><p>对照页码、来源与摘录。</p></div></li>
          <li><span>3</span><div><strong>回到原文</strong><p>需要时打开公开链接复核。</p></div></li>
        </ol>
      </div>}
  </aside>;
}

export function EvidenceInspector({ citation: item }: { citation: ResearchCitation }) {
  const externalUrl = safeExternalUrl(item.url);
  return <div className="research-evidence-card">
    <div className="research-citation-ledger"><span>{item.citation_id}</span>
      <div><strong>{sourceTierLabel(item.source_tier)}</strong><small>{item.source_name}</small></div>
    </div>
    <h2>{item.title}</h2>
    {item.unavailable && <p role="status" className="research-community-warning">原证据不可用：以下为回答时保存的历史摘录，未绑定到当前资料。</p>}
    <div className="research-evidence-meta">
      <span>{item.published_at ? formatDateTime(item.published_at) : "日期未提供"}</span>
      {item.page_number != null && <span>第 {item.page_number} 页</span>}
    </div>
    <blockquote>{item.excerpt}</blockquote>
    <details className="research-retrieval-details"><summary>检索技术详情</summary><dl>
      <div><dt>融合分数</dt><dd>{formatScore(item.retrieval_score)}</dd></div>
      <div><dt>关键词</dt><dd>{formatScore(item.lexical_score)}</dd></div>
      <div><dt>向量</dt><dd>{item.vector_score == null ? "未使用" : formatScore(item.vector_score)}</dd></div>
    </dl></details>
    {externalUrl && <a href={externalUrl} target="_blank" rel="noreferrer">
      <ExternalLink size={15} />打开原文
    </a>}
    {item.source_tier === "community" && <p className="research-community-warning">
      社区信息只能作为线索，不能单独支撑事实结论。
    </p>}
  </div>;
}

function CitationRichText(props: {
  text: string;
  citations: ResearchCitation[];
  onCitation: (citation: ResearchCitation) => void;
}) {
  return <>{props.text.split(/(\[C\d+\])/g).map((part, index) => {
    const match = /^\[(C\d+)\]$/.exec(part);
    const item = match ? props.citations.find((citation) =>
      citation.citation_id === match[1]) : undefined;
    return item ? <button type="button" className="research-inline-citation"
      key={part + index} onClick={() => props.onCitation(item)}>{part}</button>
      : <span key={part + index}>{part}</span>;
  })}</>;
}
function KnowledgeDrawer(props: {
  code: string;
  mobile: boolean;
  status: ResearchIndexStatus | null;
  management: (action: () => Promise<unknown>) => Promise<void>;
  busy: boolean;
  result: unknown;
  close: () => void;
}) {
  const [url, setUrl] = useState("");
  const [allowRemoteExport, setAllowRemoteExport] = useState(false);
  const inputFile = async (file: File, endpoint: string) => props.management(async () => {
    validateResearchFile(file, endpoint);
    return postJson(endpoint, {
      bytes_base64: await fileToBase64(file),
      stock_codes: props.code ? [props.code] : [],
      ...(endpoint === "/api/research/import-pdf"
        ? { remote_export_allowed: allowRemoteExport }
        : {}),
    });
  });
  const operationNotice = props.result == null ? null : researchOperationNotice(props.result);

  return <Sheet open onClose={props.close} label="知识库管理" className="research-surface-sheet" backdropClassName="research-surface-backdrop">
    <aside className="knowledge-drawer">
      <header><div><span>{props.mobile ? "资料包同步" : "知识库管理"}</span>
        <h2>{props.mobile ? "导入与回滚" : "资料与索引"}</h2></div>
        <button type="button" className="research-icon-button" aria-label="关闭知识库管理"
          title="关闭" onClick={props.close}><X size={18} /></button>
      </header>

      <section className="knowledge-status">
        <div><span>文档</span><strong>{props.status?.document_count || 0}</strong></div>
        <div><span>分块 / FTS</span><strong>
          {props.status?.chunk_count || 0} / {props.status?.fts_count || 0}
        </strong></div>
        <div><span>向量</span><strong>{props.status?.embedding_count || 0}</strong></div>
        <div><span>占用</span><strong>{formatBytes(props.status?.database_bytes)}</strong></div>
      </section>

      {!props.mobile && <section className="knowledge-section">
        <h3>导入资料</h3>
        <label><span>公网 HTTPS 地址</span>
          <input value={url} onChange={(event) => setUrl(event.target.value)}
            placeholder="https://" />
        </label>
        <label className="knowledge-export-consent">
          <input type="checkbox" checked={allowRemoteExport}
            onChange={(event) => setAllowRemoteExport(event.target.checked)} />
          <span>允许把这份资料发送给远程回答模型（默认关闭）</span>
        </label>
        <button type="button" disabled={props.busy || !url.trim()}
          onClick={() => void props.management(() => postJson("/api/research/import-url", {
            url: url.trim(), stock_codes: props.code ? [props.code] : [],
            remote_export_allowed: allowRemoteExport,
          }))}><Download size={15} />导入 URL</button>
        <label className="knowledge-file-button"><FileText size={16} />
          <span>导入文本型 PDF</span>
          <input type="file" accept="application/pdf,.pdf" onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void inputFile(file, "/api/research/import-pdf");
            event.currentTarget.value = "";
          }} />
        </label>
        <small>扫描件需先 OCR；单文件上限 25 MB、500 页。</small>
      </section>}

      {!props.mobile && <section className="knowledge-section">
        <h3>索引维护</h3>
        <div className="knowledge-button-row">
          <button type="button" disabled={props.busy}
            onClick={() => void props.management(() =>
              postJson("/api/research/rebuild-index", {}))}>
            <RefreshCw size={15} />重建 FTS
          </button>
          <button type="button" disabled={props.busy}
            onClick={() => void props.management(() =>
              postJson("/api/research/rebuild-embeddings", {}))}>
            <Search size={15} />生成向量
          </button>
        </div>
        <p>{!props.status ? "正在读取索引状态。"
          : !(props.status.fts_healthy ?? props.status.healthy) ? "FTS 索引存在缺口，建议重建 FTS。"
          : (props.status?.embedding_pending_count || 0) > 0
            ? `FTS 索引完整；还有 ${props.status?.embedding_pending_count} 个分块等待生成向量。`
            : props.status.integrity_healthy === false
              ? "索引存在完整性问题，建议重建索引。"
            : props.status?.hybrid_ready
              ? "FTS 与向量索引完整，可执行混合检索。"
              : "FTS 索引完整；当前平台使用关键词检索。"}</p>
      </section>}

      <section className="knowledge-section">
        <h3>同步与回滚</h3>
        <div className={`knowledge-button-row${props.mobile ? " single" : ""}`}>
          {!props.mobile && <button type="button" disabled={props.busy}
            onClick={() => void props.management(() =>
              postJson("/api/research/pack/export", {}))}>
            <Upload size={15} />导出 v2 包
          </button>}
          <button type="button" disabled={props.busy}
            onClick={() => void props.management(() =>
              postJson("/api/research/pack/rollback", {}))}>
            <RotateCcw size={15} />回滚导入
          </button>
        </div>
        <label className="knowledge-file-button"><Database size={16} />
          <span>导入 SQLite v2 / 旧版包</span>
          <input type="file" accept=".sqlite,.json" onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void inputFile(file, "/api/research/pack/import");
            event.currentTarget.value = "";
          }} />
        </label>
      </section>

      {props.busy && <div className="knowledge-operation">
        <RefreshCw size={15} className="is-spinning" />正在处理…
      </div>}
      {!props.busy && operationNotice &&
        <div className={`knowledge-operation ${operationNotice.tone}`}>
          {operationNotice.text}
        </div>}
      <details className="knowledge-diagnostics"><summary>高级诊断</summary>
        <pre>{JSON.stringify(props.status, null, 2)}</pre>
        {props.result != null && <pre>{JSON.stringify(props.result, null, 2)}</pre>}
      </details>
    </aside>
  </Sheet>;
}

export function researchOperationNotice(result: unknown): {
  tone: "success" | "warning" | "error";
  text: string;
} {
  if (result && typeof result === "object") {
    const record = result as Record<string, unknown>;
    if ("error" in record) {
      return { tone: "error", text: String(record.error) };
    }
    if (record.ocr_required === true) {
      const skippedPages = Array.isArray(record.skipped_pages)
        ? record.skipped_pages.filter((page): page is number => Number.isInteger(page)) : [];
      const extracted = Number(record.extracted_page_count || 0);
      const total = Number(record.page_count || extracted);
      const pages = skippedPages.length ? `第 ${skippedPages.join("、")} 页` : "部分页面";
      return {
        tone: "warning",
        text: `已导入 ${extracted}/${total} 页；${pages}没有可提取文本，需要 OCR 后重新导入。`,
      };
    }
  }
  return { tone: "success", text: "操作已完成，索引状态已刷新。" };
}

export function NewsRagView({ result }: { result: NewsRagResult }) {
  const groups = normalizeNewsGroups(result.sentiment_groups);
  const items = [...groups.positive, ...groups.negative, ...groups.mixed, ...groups.uncertain];
  return <div className="agent-news-evidence">
    <header><strong>消息证据</strong><span>{items.length} 条</span></header>
    {!items.length ? <p>当前没有命中的消息证据。</p>
      : items.slice(0, 12).map((item, index) => <article
        key={(item.title || "消息") + index}>
        <span>{sourceTierLabel(item.source_tier)}</span>
        <div><strong>{item.title || "未命名消息"}</strong><p>{item.summary}</p></div>
      </article>)}
  </div>;
}

function sourceTierLabel(value?: string): string {
  const labels: Record<string, string> = {
    policy_official: "官方政策",
    news_media: "财经媒体",
    filing: "公告", financial_snapshot: "财务快照", news: "新闻",
    research: "研报", research_report: "研报", community: "社区",
  };
  const key = value ? String(value) : "news";
  return labels[key] || "新闻";
}

function scopeTypeLabel(value?: string): string {
  return ({ macro: "宏观", industry: "行业", region: "区域", company: "公司" } as Record<string, string>)[value || ""] || value || "政策";
}

function sentimentClass(value: string): string {
  if (["positive", "bullish", "利好"].includes(value)) return "positive";
  if (["negative", "bearish", "利空"].includes(value)) return "negative";
  return "uncertain";
}

function groupMessages(messages: ResearchMessage[]) {
  const sorted = messages.map((message, index) => ({ message, index, timestamp: publishTimestamp(message.published_at) }))
    .sort((left, right) => {
      if (left.timestamp === null) return right.timestamp === null ? left.index - right.index : 1;
      if (right.timestamp === null) return -1;
      return right.timestamp - left.timestamp || left.index - right.index;
    });
  const groups = new Map<string, { key: string; label: string; messages: ResearchMessage[] }>();
  for (const { message, timestamp } of sorted) {
    const date = timestamp === null ? null : sentimentCivilDate(timestamp);
    const key = date || "unknown";
    if (!groups.has(key)) groups.set(key, { key, label: date || "日期未提供", messages: [] });
    groups.get(key)!.messages.push(message);
  }
  return [...groups.values()];
}
function publishTimestamp(value?: string | null): number | null {
  if (!value) return null;
  // The research cache uses China civil time for timestamps without an offset.
  const normalized = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?$/.test(value) ? `${value.replace(" ", "T")}+08:00` : value;
  const timestamp = Date.parse(normalized);
  return Number.isFinite(timestamp) ? timestamp : null;
}
function isToday(value?: string | null): boolean {
  const timestamp = publishTimestamp(value);
  return timestamp !== null && sentimentCivilDate(timestamp) === sentimentCivilDate(Date.now());
}

function safeExternalUrl(value?: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function formatEpoch(value?: number): string {
  return value ? new Date(value).toLocaleDateString("zh-CN", {
    month: "2-digit", day: "2-digit",
  }) : "刚刚";
}

export function formatResearchUpdatedAt(overview: ResearchOverview | null): string {
  if (!overview) return "";
  const raw = overview.updated_at_epoch_ms || overview.last_refresh_at || overview.last_updated_at;
  if (!raw) return "";
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });
}

function formatScore(value?: number | null): string {
  return Number.isFinite(value) ? Number(value).toFixed(4) : "—";
}

export function validateResearchFile(file: Pick<File, "name" | "size">, endpoint: string): void {
  const pdf = endpoint === "/api/research/import-pdf";
  const limit = pdf ? MAX_PDF_FILE_BYTES : MAX_RESEARCH_PACK_BYTES;
  if (file.size > limit) {
    throw new Error(`${file.name || "文件"} 超过 ${pdf ? "25 MB" : "64 MB"} 上限`);
  }
}

async function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error || new Error("读取文件失败"));
    reader.onload = () => {
      const result = typeof reader.result === "string" ? reader.result : "";
      const separator = result.indexOf(",");
      if (separator < 0) {
        reject(new Error("文件编码失败"));
        return;
      }
      resolve(result.slice(separator + 1));
    };
    reader.readAsDataURL(file);
  });
}
