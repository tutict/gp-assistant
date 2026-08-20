import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  BookOpen, Database, Download, ExternalLink, FileText, Inbox, Menu,
  MessageSquareText, Plus, RefreshCw, RotateCcw, Search, Send, Upload, X,
  Trash2,
} from "lucide-react";
import type {
  LlmSettings, NewsRagResult, ResearchAnswer, ResearchCitation,
  ResearchIndexStatus, ResearchMessage, ResearchOverview, ResearchQueryResult,
  ResearchThread, WatchlistItem,
} from "../../types";
import { buildLlmConfig, buildNewsRagRequest, normalizeNewsGroups } from "../../lib/contracts";
import { formatBytes, formatDateTime, normalizeStockCode } from "../../lib/format";
import { getJson, isMobileTauriRuntime, postJson } from "../../lib/tauri";
import { LlmSettingsPanel } from "./LlmSettingsPanel";
import { PanelFeedback } from "../ui/PanelFeedback";

type LlmSettingsUpdater = LlmSettings | null | ((previous: LlmSettings | null) => LlmSettings | null);
interface NewsRagPanelProps {
  llmSettings?: LlmSettings | null;
  onLlmSettingsChange?: (value: LlmSettingsUpdater) => void;
  watchlist?: WatchlistItem[];
  initialCode?: string;
  initialCodeRequestId?: number;
}
interface ThreadDetail { answers?: ResearchAnswer[]; }
const MAX_PDF_FILE_BYTES = 25 * 1024 * 1024;
const MAX_RESEARCH_PACK_BYTES = 64 * 1024 * 1024;

export function NewsRagPanel(props: NewsRagPanelProps) {
  const mobile = isMobileTauriRuntime();
  const questionInputId = useId();
  const watchlist = props.watchlist || [];
  const [code, setCode] = useState(() => normalizeStockCode(props.initialCode || watchlist[0]?.code || ""));
  const codeRef = useRef(code);
  const [overview, setOverview] = useState<ResearchOverview | null>(null);
  const [messages, setMessages] = useState<ResearchMessage[]>([]);
  const [threads, setThreads] = useState<ResearchThread[]>([]);
  const [threadId, setThreadId] = useState("");
  const threadIdRef = useRef("");
  const deletedThreadIdsRef = useRef(new Set<string>());
  const workspaceGenerationRef = useRef(0);
  const [answers, setAnswers] = useState<ResearchAnswer[]>([]);
  const [question, setQuestion] = useState("");
  const [citation, setCitation] = useState<ResearchCitation | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState("");
  const [inboxOpen, setInboxOpen] = useState(false);
  const [knowledgeOpen, setKnowledgeOpen] = useState(false);
  const [indexStatus, setIndexStatus] = useState<ResearchIndexStatus | null>(null);
  const [managementBusy, setManagementBusy] = useState(false);
  const [managementResult, setManagementResult] = useState<unknown>(null);
  const [deletingThreadId, setDeletingThreadId] = useState<string | null>(null);

  useEffect(() => { threadIdRef.current = threadId; }, [threadId]);
  const selectCode = useCallback((nextCode: string) => {
    const normalized = normalizeStockCode(nextCode);
    if (normalized === codeRef.current) return;
    codeRef.current = normalized;
    workspaceGenerationRef.current += 1;
    setAsking(false);
    setRefreshing(false);
    setCode(normalized);
  }, []);
  useEffect(() => {
    const next = normalizeStockCode(props.initialCode);
    if (next) selectCode(next);
  }, [props.initialCode, props.initialCodeRequestId, selectCode]);

  const loadThread = useCallback(async (
    id: string,
    generation = workspaceGenerationRef.current,
  ) => {
    if (!id) {
      if (generation === workspaceGenerationRef.current) setAnswers([]);
      return;
    }
    const detail = await postJson<ThreadDetail>("/api/research/threads/detail", { thread_id: id });
    if (generation !== workspaceGenerationRef.current) return;
    setAnswers(detail.answers || []);
  }, []);

  const loadWorkspace = useCallback(async (quiet = false) => {
    const generation = ++workspaceGenerationRef.current;
    if (!quiet) setLoading(true);
    setError("");
    try {
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
      setOverview(nextOverview);
      setMessages(messageResult.items || []);
      setThreads(nextThreads);
      const preferred = nextThreads.find((item) =>
        item.id === threadIdRef.current && (item.stock_code || "") === code)
        || nextThreads.find((item) => item.stock_code === code)
        || (!code ? nextThreads.find((item) => !item.stock_code) : undefined);
      if (preferred) {
        setThreadId(preferred.id);
        await loadThread(preferred.id, generation);
      } else {
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
  }, [code, loadThread]);

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
      await postJson("/api/research/refresh", buildNewsRagRequest(code, 30));
      if (generation !== workspaceGenerationRef.current) return;
      await loadWorkspace(true);
    } catch (nextError) {
      if (!background && generation === workspaceGenerationRef.current) {
        setError((nextError as Error).message);
      }
    } finally {
      if (!background && generation === workspaceGenerationRef.current) setRefreshing(false);
    }
  }, [code, loadWorkspace]);

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
  const summary = useMemo(() => {
    if (!todayMessages.length) {
      return code ? `${code} 暂无新增证据。可立即更新，或导入公告、研报后再核查。`
        : "当前知识库暂无新增证据。请选择自选股或导入资料。";
    }
    const direction = grouped.positive.length > grouped.negative.length ? "利好线索较多"
      : grouped.negative.length > grouped.positive.length ? "风险线索较多" : "多空线索接近";
    return `今日共整理 ${todayMessages.length} 条事件，${direction}；其中 ${grouped.uncertain.length} 条仍需公告或财务数据交叉核验。`;
  }, [code, grouped, todayMessages.length]);

  const markRead = useCallback(async (message: ResearchMessage) => {
    if (!message.unread) return;
    try {
      await postJson("/api/research/mark-read", { message_ids: [message.id] });
      setMessages((current) => current.map((item) =>
        item.id === message.id ? { ...item, unread: false } : item));
      setOverview((current) => {
        if (!current) return current;
        const unreadByStock = { ...(current.unread_by_stock || {}) };
        if (message.stock_code) {
          unreadByStock[message.stock_code] = Math.max(
            0,
            (unreadByStock[message.stock_code] || 0) - 1,
          );
        }
        return {
          ...current,
          unread_count: Math.max(0, current.unread_count - 1),
          unread_by_stock: unreadByStock,
        };
      });
    } catch (nextError) {
      setError((nextError as Error).message);
    }
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
    setThreads((current) => [thread, ...current]);
    setThreadId(thread.id);
    setAnswers([]);
    return thread.id;
  }, [code]);

  const deleteThread = useCallback(async (thread: ResearchThread) => {
    if (asking || deletingThreadId) return;
    if (!window.confirm(`确认删除研究会话“${thread.title || "未命名会话"}”？历史问答将一并删除。`)) {
      return;
    }
    const generation = workspaceGenerationRef.current;
    setDeletingThreadId(thread.id);
    setError("");
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
      if (generation === workspaceGenerationRef.current) setError((nextError as Error).message);
    } finally {
      setDeletingThreadId(null);
    }
  }, [asking, deletingThreadId, loadWorkspace]);

  const ask = useCallback(async () => {
    const text = question.trim();
    if (!text || asking || deletingThreadId) return;
    const generation = workspaceGenerationRef.current;
    const requestedCode = code;
    setAsking(true);
    setError("");
    try {
      const selectedThread = threads.find((thread) => thread.id === threadId);
      const activeThread = selectedThread && (selectedThread.stock_code || "") === requestedCode
        ? selectedThread.id
        : await createThread(generation, requestedCode);
      if (!activeThread || generation !== workspaceGenerationRef.current) return;
      const result = await postJson<ResearchQueryResult>("/api/research/query", {
        query: text, stock_code: requestedCode || null, thread_id: activeThread, top_k: 8,
        llm: buildLlmConfig(props.llmSettings),
      });
      if (generation !== workspaceGenerationRef.current) return;
      const answer: ResearchAnswer = { ...result, question: text, citations: result.citations || [] };
      setAnswers((current) => [...current, answer]);
      setQuestion("");
      setCitation(answer.citations[0] || null);
      if (mobile) setInboxOpen(false);
      const nextThreads = await getJson<{ items?: ResearchThread[] }>("/api/research/threads");
      if (generation === workspaceGenerationRef.current) {
        setThreads((nextThreads.items || []).filter(
          (thread) => !deletedThreadIdsRef.current.has(thread.id),
        ));
      }
    } catch (nextError) {
      if (generation === workspaceGenerationRef.current) setError((nextError as Error).message);
    } finally {
      if (generation === workspaceGenerationRef.current) setAsking(false);
    }
  }, [asking, code, createThread, deletingThreadId, mobile, props.llmSettings, question, threadId, threads]);

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

  if (loading && !overview) {
    return <div className="research-loading">
      <PanelFeedback kind="loading" description="正在打开研究消息中心…" />
    </div>;
  }
  const vectorReady = overview?.retrieval?.vector?.ready === true;

  return <section className="research-workspace" aria-label="研究消息中心">
    <header className="research-topbar">
      <div className="research-context">
        <button type="button" className="research-icon-button research-mobile-inbox-button"
          aria-label="打开自选股收件箱" title="自选股收件箱" onClick={() => setInboxOpen(true)}>
          <Menu size={18} />
        </button>
        <div><span className="research-eyebrow">研究消息中心</span><h1>
          {stock?.name || code || "全部自选股"}{code && <small>{code}</small>}
        </h1></div>
        <span className={`research-mode ${vectorReady ? "ready" : "lexical"}`}><Search size={13} />
          {vectorReady ? "混合检索" : "BM25 证据模式"}
        </span>
      </div>
      <div className="research-actions">
        <span>自选股前台每 15 分钟更新</span>
        <button type="button" onClick={() => void refresh()} disabled={refreshing}>
          <RefreshCw size={15} className={refreshing ? "is-spinning" : ""} />
          <span>{refreshing ? "更新中" : "立即更新"}</span>
        </button>
        <button type="button" onClick={openKnowledge}
          aria-label={mobile ? "资料包同步" : "知识库管理"}>
          <Database size={15} /><span>{mobile ? "资料包同步" : "知识库管理"}</span>
        </button>
      </div>
    </header>

    {error && <div className="research-error">
      <PanelFeedback kind="error" title="研究中心暂时不可用" description={error} />
    </div>}

    <div className="research-columns">
      <InboxPanel code={code} setCode={(next) => { selectCode(next); setInboxOpen(false); }}
        watchlist={watchlist} unreadByStock={overview?.unread_by_stock || {}}
        threads={threads} threadId={threadId}
        setThread={(thread) => {
          selectCode(thread.stock_code || "");
          setThreadId(thread.id);
          void loadThread(thread.id);
          setInboxOpen(false);
        }}
        unread={overview?.unread_count || 0} open={inboxOpen} close={() => setInboxOpen(false)}
        createThread={() => void createThread()}
        deleteThread={(thread) => void deleteThread(thread)}
        deletingThreadId={deletingThreadId}
        asking={asking} />

      <main className="research-stream">
        <div className="research-stream-body">
        <section className="research-daily-brief">
          <div className="research-brief-rule"><span>今日摘要</span>
            <time>{new Date().toLocaleDateString("zh-CN")}</time>
          </div>
          <p>{summary}</p>
          <div className="research-brief-counts">
            <span className="research-brief-stat positive">
              <span>利好</span><strong>{grouped.positive.length}</strong>
            </span>
            <span className="research-brief-stat negative">
              <span>利空</span><strong>{grouped.negative.length}</strong>
            </span>
            <span className="research-brief-stat">
              <span>待核查</span><strong>{grouped.uncertain.length}</strong>
            </span>
            <span className="research-brief-stat">
              <span>文档</span><strong>{overview?.document_count || 0}</strong>
            </span>
          </div>
        </section>

        <section className="research-event-section">
          <div className="research-section-heading"><div><span>事件流</span>
            <small>按来源等级与时间整理</small></div><span>{visibleMessages.length} 条</span>
          </div>
          {!visibleMessages.length ? <ResearchEmptyState
            refreshing={refreshing}
            onRefresh={() => void refresh()}
            onKnowledge={openKnowledge}
          /> : <div className="research-event-list">
            {eventGroups.map((group) => <EventGroup key={group.key}
              label={group.label} tone={group.key} messages={group.messages}
              markRead={markRead} />)}
          </div>}
        </section>

        <Answers answers={answers} setCitation={setCitation} />
        </div>
        <form className="research-composer" onSubmit={(event) => { event.preventDefault(); void ask(); }}>
          <div><label htmlFor={questionInputId}><span>研究问题</span><small>
            {buildLlmConfig(props.llmSettings)
              ? "模型回答会强制引用证据" : "未配置模型，返回证据摘录"}
          </small></label>
            <textarea id={questionInputId} value={question}
              onChange={(event) => setQuestion(event.target.value)}
              placeholder={questionPlaceholder} rows={2} />
            <p className="research-risk-boundary">仅供研究，不构成投资建议。</p>
          </div>
          <button type="submit" aria-label="提交问题" title="提交问题"
            disabled={!question.trim() || asking || Boolean(deletingThreadId)}>
            {asking || deletingThreadId ? <RefreshCw size={18} className="is-spinning" /> : <Send size={18} />}
          </button>
        </form>
      </main>

      <EvidencePanel citation={citation} close={() => setCitation(null)} />
    </div>

    {inboxOpen && <button type="button" className="research-mobile-overlay"
      aria-label="关闭自选股收件箱" onClick={() => setInboxOpen(false)} />}
    {knowledgeOpen && <KnowledgeDrawer panelProps={props} code={code} mobile={mobile}
      status={indexStatus} management={management} busy={managementBusy}
      result={managementResult} close={() => setKnowledgeOpen(false)} />}
  </section>;
}
function EventGroup(props: {
  label: string;
  tone: string;
  messages: ResearchMessage[];
  markRead: (message: ResearchMessage) => Promise<void>;
}) {
  if (!props.messages.length) return null;
  return <section className={`research-event-group ${props.tone}`}>
    <header><span>{props.label}</span><b>{props.messages.length}</b></header>
    {props.messages.map((message) => <button type="button" key={message.id}
      className={`research-event ${sentimentClass(message.sentiment)}${message.unread ? " unread" : ""}`}
      onClick={() => void props.markRead(message)}>
      <span className="research-event-mark" />
      <span className="research-event-content">
        <span className="research-event-meta">
          <span className={`research-source-tier ${message.source_tier}`}>
            {sourceTierLabel(message.source_tier)}
          </span>
          <time>{formatDateTime(message.published_at)}</time>
          {message.unread && <b>未读</b>}
        </span>
        <strong>{message.title}</strong>
        <span className="research-event-summary">{message.summary}</span>
      </span>
    </button>)}
  </section>;
}

function ResearchEmptyState(props: {
  refreshing: boolean;
  onRefresh: () => void;
  onKnowledge: () => void;
}) {
  return <div className="research-empty">
    <BookOpen size={28} />
    <strong>还没有可研究的消息</strong>
    <p>选择一只自选股并立即更新，或从知识库管理导入公告与研报。</p>
    <div className="research-empty-actions">
      <button type="button" className="research-empty-primary"
        disabled={props.refreshing} onClick={props.onRefresh}>
        <RefreshCw size={15} className={props.refreshing ? "is-spinning" : ""} />
        <span>{props.refreshing ? "更新中" : "立即更新"}</span>
      </button>
      <button type="button" className="research-empty-ghost" onClick={props.onKnowledge}>
        <Database size={15} />
        <span>知识库管理</span>
      </button>
    </div>
  </div>;
}


function InboxPanel(props: {
  code: string;
  setCode: (value: string) => void;
  watchlist: WatchlistItem[];
  unreadByStock: Record<string, number>;
  threads: ResearchThread[];
  threadId: string;
  setThread: (thread: ResearchThread) => void;
  unread: number;
  open: boolean;
  close: () => void;
  createThread: () => void;
  deleteThread: (thread: ResearchThread) => void;
  deletingThreadId: string | null;
  asking: boolean;
}) {
  return <aside className={`research-inbox${props.open ? " mobile-open" : ""}`}>
    <div className="research-pane-heading">
      <div><span>自选股收件箱</span><strong>{props.unread} 条未读</strong></div>
      <button className="research-icon-button research-mobile-close" type="button"
        aria-label="关闭收件箱" title="关闭" onClick={props.close}><X size={17} /></button>
    </div>
    <button type="button" className={`research-stock-row${!props.code ? " active" : ""}`}
      onClick={() => props.setCode("")}>
      <span className="research-stock-monogram"><Inbox size={16} /></span>
      <span><strong>全部消息</strong><small>跨股票研究流</small></span>
      {props.unread > 0 && <b>{props.unread}</b>}
    </button>
    {props.watchlist.map((item) => {
      const stockCode = normalizeStockCode(item.code);
      const unread = props.unreadByStock[stockCode] || 0;
      return <button type="button" key={stockCode}
        className={`research-stock-row${props.code === stockCode ? " active" : ""}`}
        onClick={() => props.setCode(stockCode)}>
        <span className="research-stock-monogram">{(item.name || stockCode).slice(0, 1)}</span>
        <span><strong>{item.name || stockCode}</strong><small>{stockCode}</small></span>
        {unread > 0 && <b>{unread}</b>}
      </button>;
    })}
    <div className="research-inbox-section">
      <div className="research-pane-heading compact"><span>研究会话</span>
        <button className="research-icon-button" type="button" aria-label="新建研究会话"
          title="新建会话" onClick={props.createThread}><Plus size={16} /></button>
      </div>
      {props.threads.slice(0, 20).map((thread) => <div key={thread.id}
        className={`research-thread-row${thread.id === props.threadId ? " active" : ""}`}>
        <button type="button" className="research-thread-main"
          onClick={() => props.setThread(thread)}>
          <MessageSquareText size={15} aria-hidden="true" />
          <span><strong>{thread.title}</strong><small>{formatEpoch(thread.updated_at_epoch_ms)}</small></span>
        </button>
        <button type="button" className="research-thread-delete"
          aria-label={`删除研究会话：${thread.title || "未命名会话"}`}
          title="删除会话" disabled={props.asking || props.deletingThreadId === thread.id}
          onClick={() => props.deleteThread(thread)}>
          <Trash2 size={14} aria-hidden="true" />
        </button>
      </div>)}
    </div>
  </aside>;
}

function Answers(props: {
  answers: ResearchAnswer[];
  setCitation: (value: ResearchCitation) => void;
}) {
  if (!props.answers.length) return null;
  return <section className="research-answers">
    <div className="research-section-heading"><div><span>历史问答</span>
      <small>回答与引用均保存在本机</small></div>
    </div>
    {props.answers.map((answer, index) => <article
      key={answer.id || answer.question + index} className="research-answer">
      <div className="research-question"><span>问</span><p>{answer.question}</p></div>
      <div className="research-answer-body">
        <div className="research-answer-mode">
          {answer.mode === "model" ? "模型综合" : "证据摘录"}
        </div>
        <p><CitationRichText text={answer.answer} citations={answer.citations}
          onCitation={props.setCitation} /></p>
        <div className="research-citation-chips">
          {answer.citations.map((item) => <button type="button"
            key={item.citation_id + index} onClick={() => props.setCitation(item)}>
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

function EvidencePanel(props: { citation: ResearchCitation | null; close: () => void }) {
  return <aside className={`research-evidence${props.citation ? " has-selection" : ""}`}
    aria-label="引用证据检查器">
    <div className="research-pane-heading">
      <div><span>证据检查器</span><strong>原文可回溯</strong></div>
      <button className="research-icon-button research-mobile-close" type="button"
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

function EvidenceInspector({ citation: item }: { citation: ResearchCitation }) {
  const externalUrl = safeExternalUrl(item.url);
  return <div className="research-evidence-card">
    <div className="research-citation-ledger"><span>{item.citation_id}</span>
      <div><strong>{sourceTierLabel(item.source_tier)}</strong><small>{item.source_name}</small></div>
    </div>
    <h2>{item.title}</h2>
    <div className="research-evidence-meta">
      <span>{item.published_at ? formatDateTime(item.published_at) : "日期未提供"}</span>
      {item.page_number != null && <span>第 {item.page_number} 页</span>}
    </div>
    <blockquote>{item.excerpt}</blockquote>
    <dl>
      <div><dt>融合分数</dt><dd>{formatScore(item.retrieval_score)}</dd></div>
      <div><dt>BM25</dt><dd>{formatScore(item.lexical_score)}</dd></div>
      <div><dt>向量</dt><dd>{item.vector_score == null ? "未使用" : formatScore(item.vector_score)}</dd></div>
    </dl>
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
  panelProps: NewsRagPanelProps;
  code: string;
  mobile: boolean;
  status: ResearchIndexStatus | null;
  management: (action: () => Promise<unknown>) => Promise<void>;
  busy: boolean;
  result: unknown;
  close: () => void;
}) {
  const [url, setUrl] = useState("");
  const inputFile = async (file: File, endpoint: string) => props.management(async () => {
    validateResearchFile(file, endpoint);
    return postJson(endpoint, {
      bytes_base64: await fileToBase64(file),
      stock_codes: props.code ? [props.code] : [],
    });
  });
  const operationNotice = props.result == null ? null : researchOperationNotice(props.result);

  return <div className="knowledge-drawer-layer">
    <button type="button" className="knowledge-drawer-overlay"
       aria-label="关闭知识库管理" onClick={props.close} />
    <aside className="knowledge-drawer">
      <header><div><span>{props.mobile ? "资料包同步" : "知识库管理"}</span>
        <h2>{props.mobile ? "导入与回滚" : "资料、索引与模型"}</h2></div>
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
        <button type="button" disabled={props.busy || !url.trim()}
          onClick={() => void props.management(() => postJson("/api/research/import-url", {
            url: url.trim(), stock_codes: props.code ? [props.code] : [],
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
              : "FTS 索引完整；当前平台使用 BM25 检索。"}</p>
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

      {props.panelProps.onLlmSettingsChange && <section className="knowledge-model-settings">
        <h3>回答模型</h3>
        <LlmSettingsPanel settings={props.panelProps.llmSettings || null}
          onChange={props.panelProps.onLlmSettingsChange} />
      </section>}

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
  </div>;
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
    filing: "公告", financial_snapshot: "财务快照", news: "新闻",
    research: "研报", research_report: "研报", community: "社区",
  };
  const key = value ? String(value) : "news";
  return labels[key] || "新闻";
}

function sentimentClass(value: string): string {
  if (["positive", "bullish", "利好"].includes(value)) return "positive";
  if (["negative", "bearish", "利空"].includes(value)) return "negative";
  return "uncertain";
}

function groupMessages(messages: ResearchMessage[]) {
  return [
    { key: "positive", label: "利好线索", messages: messages.filter((item) => ["positive", "bullish", "利好"].includes(item.sentiment)) },
    { key: "negative", label: "利空与风险", messages: messages.filter((item) => ["negative", "bearish", "利空"].includes(item.sentiment)) },
    { key: "uncertain", label: "待核查", messages: messages.filter((item) => !["positive", "bullish", "利好", "negative", "bearish", "利空"].includes(item.sentiment)) },
  ];
}

function isToday(value?: string | null): boolean {
  if (!value) return false;
  const date = new Date(value);
  const today = new Date();
  return !Number.isNaN(date.getTime())
    && date.getFullYear() === today.getFullYear()
    && date.getMonth() === today.getMonth()
    && date.getDate() === today.getDate();
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
