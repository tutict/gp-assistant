import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AgentResult, AgentStreamEvent, BacktestResult, LlmSettings, NewsRagResult, ObserveResult, StockRowView, WatchlistItem } from "../../types";
import { getTauriInvoke, getTauriListen, isTauriRuntime } from "../../lib/tauri";
import { actionResultKind, activeLlmProvider, buildLlmConfig, normalizeAgentResult, normalizeAgentStreamEvent, normalizeScreenRows, parseSseBlock } from "../../lib/contracts";
import { useLocalStorage } from "../../hooks/useLocalStorage";
import { StockList } from "../StockList";
import { BacktestResultView } from "./BacktestPanel";
import { LlmSettingsPanel } from "./LlmSettingsPanel";
import { NewsRagView } from "./NewsRagPanel";
import { ObserveResultView } from "./ObservePanel";

interface AgentPanelProps {
  llmSettings: LlmSettings | null;
  onLlmSettingsChange: (settings: LlmSettings | null) => void;
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
}

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
  timestamp: number;
  result?: AgentResult;
  steps?: AgentStep[];
  error?: boolean;
}

interface AgentStep {
  stage: string;
  label: string;
  percent: number;
}

interface AgentConversation {
  id: string;
  title: string;
  mode: AgentMode;
  messages: ChatMessage[];
  createdAt: number;
  updatedAt: number;
}

const AGENT_MODES = [
  { id: "quick", label: "快速模式", hint: "直接执行选股、观察和新闻查询" },
  { id: "expert", label: "专家模式", hint: "要求更完整的证据、风险和下一步动作" },
  { id: "research", label: "研报模式", hint: "聚焦上下游、公告和消息链路" },
] as const;

const AGENT_CAPABILITIES = [
  { key: "stock_snapshot", title: "个股速览", hint: "行情、财务、资金面一屏汇总", example: "看一下贵州茅台财务和资金面" },
  { key: "stock_news", title: "资讯研判", hint: "近期利好利空与来源证据", example: "分析 300750 最近利好利空" },
  { key: "stock_screen", title: "智能选股", hint: "按条件筛选候选股并解释原因", example: "筛选高 ROE 低估值股票" },
  { key: "trend_analysis", title: "趋势分析", hint: "K 线、量价和趋势因子观察", example: "筛选半导体趋势股" },
  { key: "sector_analysis", title: "板块/主题", hint: "板块强弱、主题链路和候选股", example: "半导体板块有什么机会" },
  { key: "watchlist_action", title: "自选股", hint: "只管理本地 watchlist", example: "把宁德时代加入自选" },
  { key: "portfolio_simulation", title: "组合观察", hint: "基于自选股和回测做模拟观察", example: "用自选股做组合观察" },
] as const;

type AgentMode = typeof AGENT_MODES[number]["id"];

const AGENT_HISTORY_KEY = "stock-optimizer-agent-conversations";
const AGENT_ACTIVE_KEY = "stock-optimizer-agent-active-conversation";
const AGENT_RAIL_COLLAPSED_KEY = "stock-optimizer-agent-rail-collapsed";
const MAX_AGENT_CONVERSATIONS = 40;

export function AgentPanel({ llmSettings, onLlmSettingsChange, watchlist, onWatchlistChange }: AgentPanelProps) {
  const [conversations, setConversations, quotaError] = useLocalStorage<AgentConversation[]>(
    AGENT_HISTORY_KEY,
    [createConversation()],
    sanitizeConversations,
  );
  const [activeConversationId, setActiveConversationId] = useLocalStorage<string>(AGENT_ACTIVE_KEY, "");
  const [railCollapsed, setRailCollapsed] = useLocalStorage<boolean>(AGENT_RAIL_COLLAPSED_KEY, false);
  const [input, setInput] = useState("");
  const [conversationSearch, setConversationSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const threadRef = useRef<HTMLDivElement>(null);
  const activeProvider = activeLlmProvider(llmSettings);

  const activeConversation = conversations.find((item) => item.id === activeConversationId) || conversations[0] || null;
  const activeMode = AGENT_MODES.find((item) => item.id === activeConversation?.mode) || AGENT_MODES[0];
  const messages = activeConversation?.messages || [];
  const sortedConversations = useMemo(
    () => [...conversations].sort((a, b) => b.updatedAt - a.updatedAt),
    [conversations],
  );
  const visibleConversations = useMemo(() => {
    const query = conversationSearch.trim().toLowerCase();
    if (!query) return sortedConversations;
    return sortedConversations.filter((conversation) => {
      const messageText = conversation.messages.map((message) => message.content).join(" ");
      return `${conversation.title} ${messageText}`.toLowerCase().includes(query);
    });
  }, [conversationSearch, sortedConversations]);
  const groupedConversations = useMemo(() => {
    const groups = [
      { label: "7 天内", items: [] as AgentConversation[] },
      { label: "30 天内", items: [] as AgentConversation[] },
      { label: "更早", items: [] as AgentConversation[] },
    ];
    const now = Date.now();
    const day = 24 * 60 * 60 * 1000;
    visibleConversations.forEach((conversation) => {
      const age = now - conversation.updatedAt;
      if (age <= 7 * day) {
        groups[0].items.push(conversation);
      } else if (age <= 30 * day) {
        groups[1].items.push(conversation);
      } else {
        groups[2].items.push(conversation);
      }
    });
    return groups.filter((group) => group.items.length);
  }, [visibleConversations]);

  useEffect(() => {
    if (!activeConversation && conversations[0]) {
      setActiveConversationId(conversations[0].id);
    } else if (activeConversation && activeConversation.id !== activeConversationId) {
      setActiveConversationId(activeConversation.id);
    }
  }, [activeConversation, activeConversationId, conversations, setActiveConversationId]);

  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [messages, loading]);

  useEffect(() => {
    if (typeof window !== "undefined" && window.matchMedia?.("(max-width: 640px)").matches) {
      setRailCollapsed(true);
    }
  }, [setRailCollapsed]);

  const updateConversation = useCallback((conversationId: string, updater: (conversation: AgentConversation) => AgentConversation) => {
    setConversations((prev) => prev.map((conversation) => (
      conversation.id === conversationId ? updater(conversation) : conversation
    )));
  }, [setConversations]);

  const startNewChat = useCallback(() => {
    const next = createConversation(activeConversation?.mode || "quick");
    setConversations((prev) => [next, ...prev].slice(0, MAX_AGENT_CONVERSATIONS));
    setActiveConversationId(next.id);
    setInput("");
  }, [activeConversation?.mode, setActiveConversationId, setConversations]);

  const switchConversation = useCallback((conversationId: string) => {
    setActiveConversationId(conversationId);
    setInput("");
    if (typeof window !== "undefined" && window.matchMedia?.("(max-width: 640px)").matches) {
      setRailCollapsed(true);
    }
  }, [setActiveConversationId, setRailCollapsed]);

  const removeConversation = useCallback((conversationId: string) => {
    setConversations((prev) => {
      const next = prev.filter((conversation) => conversation.id !== conversationId);
      if (conversationId === activeConversationId) {
        setActiveConversationId(next[0]?.id || "");
      }
      return next.length ? next : [createConversation()];
    });
  }, [activeConversationId, setActiveConversationId, setConversations]);

  const changeMode = useCallback((mode: AgentMode) => {
    if (!activeConversation) return;
    updateConversation(activeConversation.id, (conversation) => ({
      ...conversation,
      mode,
      updatedAt: Date.now(),
    }));
  }, [activeConversation, updateConversation]);

  const toggleWatchlist = useCallback((item: StockRowView) => {
    const exists = watchlist.some((w) => w.code === item.code);
    if (exists) {
      onWatchlistChange(watchlist.filter((w) => w.code !== item.code));
    } else {
      onWatchlistChange([
        { code: item.code, name: item.name, industry: item.industry, added_at: new Date().toISOString(), source: "agent" },
        ...watchlist,
      ]);
    }
  }, [onWatchlistChange, watchlist]);

  const send = useCallback(async () => {
    const text = input.trim();
    const conversationId = activeConversation?.id;
    const mode = activeConversation?.mode || "quick";
    if (!text || loading || !conversationId) return;

    const runId = crypto.randomUUID?.() || `agent-${Date.now()}`;
    const now = Date.now();
    const userMessage: ChatMessage = { role: "user", content: text, timestamp: now };
    const assistantMessage: ChatMessage = { role: "assistant", content: "准备中...", timestamp: now, steps: [] };
    setInput("");
    setLoading(true);

    updateConversation(conversationId, (conversation) => {
      const nextMessages = [...conversation.messages, userMessage, assistantMessage];
      return {
        ...conversation,
        title: conversation.messages.length ? conversation.title : titleFromMessage(text),
        messages: nextMessages,
        updatedAt: now,
      };
    });

    const patchAssistant = (patch: Partial<ChatMessage>) => {
      updateConversation(conversationId, (conversation) => ({
        ...conversation,
        messages: conversation.messages.map((message, index) => (
          index === conversation.messages.length - 1 ? { ...message, ...patch } : message
        )),
        updatedAt: Date.now(),
      }));
    };

    const applyEvent = (event: AgentStreamEvent) => {
      if (event.run_id && event.run_id !== runId) return;
      if (event.type === "status") {
        const step = {
          stage: event.stage || `stage-${Date.now()}`,
          label: event.label || event.stage || "运行中",
          percent: Number(event.percent || 0),
        };
        updateAssistantProgress(conversationId, updateConversation, step, event.label || event.stage || "运行中...");
      } else if (event.type === "tool_start") {
        const payload = asRecord(event.payload);
        const tool = String(payload.tool || event.action || "tool");
        const step = {
          stage: String(payload.id || `tool-${tool}`),
          label: String(payload.label || payload.tool || tool || "执行工具"),
          percent: 45,
        };
        updateAssistantProgress(conversationId, updateConversation, step, step.label);
      } else if (event.type === "tool_result") {
        const payload = asRecord(event.payload);
        const tool = String(payload.tool || event.action || "tool");
        const step = {
          stage: String(payload.id || `tool-${tool}`),
          label: String(payload.output_summary || payload.status || "工具完成"),
          percent: 78,
        };
        updateAssistantProgress(conversationId, updateConversation, step, step.label);
      } else if (event.type === "evidence") {
        updateAssistantProgress(conversationId, updateConversation, {
          stage: "evidence",
          label: "汇总证据",
          percent: 86,
        }, "正在汇总证据...");
      } else if (event.type === "final") {
        updateAssistantProgress(conversationId, updateConversation, {
          stage: "final",
          label: "整理结果",
          percent: 96,
        }, "正在整理结果...");
      } else if (event.type === "result") {
        const result = normalizeAgentResult(event.response || {});
        patchAssistant({ content: String(result.reply || "已完成。"), result, steps: undefined });
      } else if (event.type === "error") {
        patchAssistant({ content: event.message || "智能体执行失败。", error: true, steps: undefined });
      }
    };

    try {
      await requestAgentStream({
        message: text,
        run_id: runId,
        llm: buildLlmConfig(llmSettings),
        mode,
        context: {
          watchlist: watchlist.slice(0, 50).map((item) => ({ code: item.code, name: item.name, industry: item.industry })),
        },
      }, applyEvent);
    } catch (err) {
      patchAssistant({ content: `错误：${(err as Error).message}`, error: true, steps: undefined });
    } finally {
      setLoading(false);
    }
  }, [activeConversation?.id, activeConversation?.mode, input, llmSettings, loading, updateConversation, watchlist]);

  return (
    <div className={`panel-container agent-panel agent-workspace ${railCollapsed ? "rail-collapsed" : ""}`}>
      <aside className="agent-rail" aria-label="Agent 会话历史">
        <button
          type="button"
          className="agent-rail-toggle"
          onClick={() => setRailCollapsed((collapsed) => !collapsed)}
          aria-label={railCollapsed ? "展开会话历史" : "折叠会话历史"}
          title={railCollapsed ? "展开会话历史" : "折叠会话历史"}
        >
          <span />
          <span />
        </button>

        <label className="agent-rail-search">
          <span aria-hidden="true" />
          <input
            value={conversationSearch}
            onChange={(event) => setConversationSearch(event.target.value)}
            placeholder="搜索对话内容..."
          />
        </label>

        <div className="agent-rail-brand">
          <span className="agent-logo-mark">股</span>
          <div>
            <strong>股选优 Agent</strong>
            <em>{activeProvider?.name || "本地策略助手"}</em>
          </div>
        </div>

        <button type="button" className="agent-new-chat" onClick={startNewChat}>
          <span>+</span><strong className="agent-new-chat-label">新建对话</strong>
        </button>

        <div className="agent-rail-section">
          <div className="agent-rail-section-head">
            <span className="agent-rail-label">对话历史</span>
            <strong>{visibleConversations.length}</strong>
          </div>
          <div className="agent-history-list">
            {groupedConversations.length ? groupedConversations.map((group) => (
              <div key={group.label} className="agent-history-group">
                <span className="agent-history-group-label">{group.label}</span>
                {group.items.map((conversation) => (
                  <article
                    key={conversation.id}
                    className={`agent-history-item ${conversation.id === activeConversation?.id ? "active" : ""}`}
                  >
                    <button type="button" className="agent-history-main" onClick={() => switchConversation(conversation.id)}>
                      <span>{conversation.title || "新对话"}</span>
                      <em>{formatConversationTime(conversation.updatedAt)}</em>
                      <b>{conversation.messages.length ? `${conversation.messages.length} 条` : "未开始"}</b>
                    </button>
                    <button
                      type="button"
                      className="agent-history-remove"
                      aria-label="删除对话"
                      onClick={(event) => {
                        event.stopPropagation();
                        removeConversation(conversation.id);
                      }}
                    >
                      ×
                    </button>
                  </article>
                ))}
              </div>
            )) : (
              <div className="agent-history-empty">没有匹配的对话</div>
            )}
          </div>
        </div>

        <div className="agent-rail-footer agent-rail-model-settings">
          <LlmSettingsPanel settings={llmSettings} onChange={onLlmSettingsChange} />
        </div>
      </aside>

      <section className="agent-chat-stage" aria-label="Agent 对话">
        <button
          type="button"
          className="agent-rail-scrim"
          onClick={() => setRailCollapsed(true)}
          aria-label="关闭对话栏"
        />
        <div className="agent-mobile-actions" aria-label="Agent 移动端操作">
          <button
            type="button"
            className="agent-mobile-menu"
            onClick={() => setRailCollapsed(false)}
            aria-label="展开对话栏"
          >
            <span />
            <span />
          </button>
          <button
            type="button"
            className="agent-mobile-new"
            onClick={() => {
              startNewChat();
              setRailCollapsed(true);
            }}
            aria-label="新建对话"
          >
            <span>+</span>
          </button>
        </div>
        <header className="agent-topbar">
          <div>
            <span>当前对话</span>
            <strong>{activeConversation?.title || "新对话"}</strong>
          </div>
          <div className="agent-topbar-meta">
            <span>{activeMode.label}</span>
            {activeProvider?.model && <em>{activeProvider.name || activeProvider.model} · {activeProvider.model}</em>}
          </div>
        </header>

        <div className={`agent-thread ${messages.length === 0 ? "empty" : ""}`} ref={threadRef}>
          {messages.length === 0 ? (
            <AgentEmptyState
              mode={activeMode.id}
              activeModel={activeProvider?.model}
              onModeChange={changeMode}
            />
          ) : messages.map((msg, i) => (
            <article key={`${msg.timestamp}-${i}`} className={`agent-message ${msg.role} ${msg.error ? "error" : ""}`}>
              <div className="agent-message-meta">
                <span>{msg.role === "user" ? "你" : "Agent"}</span>
                <time>{new Date(msg.timestamp).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</time>
              </div>
              <div className="agent-message-body">
                {msg.steps?.length ? <AgentSteps steps={msg.steps} /> : null}
                <p className="agent-final-reply">{msg.role === "assistant" && !msg.result ? sanitizeLegacyAgentReply(msg.content) : msg.content}</p>
                {msg.result && <AgentResultView result={msg.result} watchlist={watchlist} onToggleWatchlist={toggleWatchlist} />}
              </div>
            </article>
          ))}
        </div>

        <div className="agent-composer-card">
          {quotaError && (
            <div className="agent-quota-warning" role="alert">
              本地存储配额已满，部分对话历史未能保存，刷新后可能丢失。建议删除旧对话后重试。
            </div>
          )}
          <button
            type="button"
            className="agent-composer-plus"
            aria-label="添加上下文（暂未开放）"
            title="添加上下文（暂未开放）"
            disabled
          >
            +
          </button>
          <textarea
            className="agent-input"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder={`给股选优 Agent 发送消息，当前为 ${activeMode.label}`}
            rows={3}
            disabled={loading}
          />
          <div className="agent-composer-tools">
            <span aria-hidden="true">{activeMode.label}</span>
            <button
              type="button"
              className="agent-voice-btn"
              aria-label="语音输入（暂未开放）"
              title="语音输入（暂未开放）"
              tabIndex={-1}
              disabled
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <path d="M12 19v3" />
              </svg>
            </button>
          </div>
          <div className="agent-composer-footer">
            <button type="button" className="send-btn" onClick={send} disabled={loading || !input.trim()} aria-label="发送">
              {loading ? "..." : "→"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

function AgentEmptyState({
  mode,
  activeModel,
  onModeChange,
}: {
  mode: AgentMode;
  activeModel?: string;
  onModeChange: (mode: AgentMode) => void;
}) {
  const activeMode = AGENT_MODES.find((item) => item.id === mode) || AGENT_MODES[0];

  return (
    <div className="agent-empty-state">
      <div className="agent-empty-mark">股</div>
      <h2>使用{activeMode.label}开始对话</h2>
      <p>{activeModel ? `当前模型：${activeModel}` : "配置模型后，可以让 Agent 调用行情、回测和消息工具完成分析。"}</p>

      <div className="agent-mode-tabs" role="tablist" aria-label="对话模式">
        {AGENT_MODES.map((item) => (
          <button
            key={item.id}
            type="button"
            className={item.id === mode ? "active" : ""}
            onClick={() => onModeChange(item.id)}
            title={item.hint}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="agent-capability-grid" aria-label="Agent 能力">
        {AGENT_CAPABILITIES.map((item) => (
          <article key={item.key} className="agent-capability-card">
            <strong>{item.title}</strong>
            <span>{item.hint}</span>
            <em>{item.example}</em>
          </article>
        ))}
      </div>
    </div>
  );
}

function AgentSteps({ steps }: { steps: AgentStep[] }) {
  return (
    <div className="agent-stream-steps">
      {steps.map((step) => <div key={step.stage} className="agent-stream-step"><span>{step.label}</span><strong>{Math.max(0, Math.min(100, step.percent))}%</strong></div>)}
    </div>
  );
}

function AgentResultView({ result, watchlist, onToggleWatchlist }: {
  result: AgentResult;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
}) {
  const kind = actionResultKind(result);
  const nested = agentNestedResult(result, kind);
  const legacyView = (() => {
    if (kind === "backtest") return <BacktestResultView result={nested as unknown as BacktestResult} />;
    if (["screen", "sector", "graph", "trend"].includes(kind)) {
      const rows = normalizeScreenRows(nested) as StockRowView[];
      return rows.length
        ? <StockList items={rows} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} />
        : <GenericAgentResult result={nested || result} />;
    }
    if (kind === "news") return <NewsRagView result={nested as unknown as NewsRagResult} />;
    if (kind === "observe") return <ObserveResultView result={nested as unknown as ObserveResult} />;
    return <GenericAgentResult result={result} />;
  })();

  return (
    <div className="agent-result-stack">
      <AgentStructuredResult result={result} />
      {legacyView}
    </div>
  );
}

function AgentStructuredResult({ result }: { result: AgentResult }) {
  const toolCalls = Array.isArray(result.tool_calls) ? result.tool_calls : [];
  const evidence = Array.isArray(result.evidence_summary) ? result.evidence_summary : [];
  const sections = Array.isArray(result.answer_sections) ? result.answer_sections : [];
  const warnings = Array.isArray(result.warnings) ? result.warnings : [];
  const nextActions = Array.isArray(result.next_actions) ? result.next_actions : [];
  if (!result.intent && !toolCalls.length && !evidence.length && !sections.length && !warnings.length && !nextActions.length) return null;

  return (
    <section className="agent-structured-result">
      {result.intent && (
        <div className="agent-intent-card">
          <span>任务理解</span>
          <strong>{result.intent.kind || result.action || "stock_research"}</strong>
          <em>{[result.intent.mode, result.intent.depth, result.intent.window].filter(Boolean).join(" ? ")}</em>
        </div>
      )}

      {toolCalls.length > 0 && (
        <div className="agent-tool-trace" aria-label="工具调用轨迹">
          {toolCalls.map((call, index) => (
            <article key={call.id || String(call.tool || "tool") + "-" + index} className={["agent-tool-call", call.status || "ok"].join(" ")}>
              <span>{index + 1}</span>
              <div>
                <strong>{call.label || call.tool || "工具调用"}</strong>
                <em>{call.output_summary || call.status || "已完成"}</em>
              </div>
              <b>{call.status || "ok"}</b>
            </article>
          ))}
        </div>
      )}

      {sections.length > 0 && (
        <div className="agent-answer-sections">
          {sections.map((section, index) => (
            <article key={String(section.title || "section") + "-" + index}>
              <strong>{section.title || "结论"}</strong>
              {(section.bullets || []).map((bullet, bulletIndex) => <p key={bulletIndex}>{bullet}</p>)}
            </article>
          ))}
        </div>
      )}

      {evidence.length > 0 && (
        <div className="agent-evidence-grid">
          {evidence.map((item, index) => (
            <article key={String(item.title || "evidence") + "-" + index}>
              <span>{item.level || "evidence"}</span>
              <strong>{item.title || item.source || "证据"}</strong>
              <p>{item.summary || item.source || "暂无证据摘要"}</p>
              {item.source && <em>{item.source}</em>}
            </article>
          ))}
        </div>
      )}

      {warnings.length > 0 && (
        <div className="agent-warning-list">
          {warnings.map((warning, index) => <p key={index}>{warning}</p>)}
        </div>
      )}

      {nextActions.length > 0 && (
        <div className="agent-next-actions">
          {nextActions.map((action, index) => <button key={index} type="button" disabled>{action}</button>)}
        </div>
      )}
    </section>
  );
}

function agentNestedResult(result: AgentResult, kind: string): Record<string, unknown> {
  const data = asRecord(result.data);
  if (Object.keys(data).length) return data;
  if (kind === "backtest") return asRecord(result.backtest);
  if (kind === "news") return asRecord(result.news_rag);
  if (kind === "observe") return asRecord(result.observe);
  if (kind === "sector") return asRecord(result.sector_screen);
  if (kind === "graph") return asRecord(result.graph_screen);
  if (kind === "trend") return asRecord(result.trend_screen);
  return asRecord(result);
}

function GenericAgentResult({ result }: { result: unknown }) {
  return <details className="raw-json"><summary>原始 JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>;
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function mergeStep(steps: AgentStep[], step: AgentStep): AgentStep[] {
  const next = [...steps];
  const index = next.findIndex((item) => item.stage === step.stage);
  if (index >= 0) next[index] = step;
  else next.push(step);
  return next;
}
function updateAssistantProgress(
  conversationId: string,
  updateConversation: (conversationId: string, updater: (conversation: AgentConversation) => AgentConversation) => void,
  step: AgentStep,
  content: string,
) {
  updateConversation(conversationId, (conversation) => ({
    ...conversation,
    messages: conversation.messages.map((message, index) => {
      if (index !== conversation.messages.length - 1) return message;
      return {
        ...message,
        content,
        steps: mergeStep(message.steps || [], step),
      };
    }),
    updatedAt: Date.now(),
  }));
}

function sanitizeLegacyAgentReply(content: string): string {
  const original = String(content || "").trim();
  if (!original) return "";
  const lines = original
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const kept = lines.filter((line) => !isAgentNoiseLine(line));
  return kept.length ? kept.join("\n") : original;
}

function isAgentNoiseLine(line: string): boolean {
  return [
    /^Android short/i,
    /^Android .* returned \d+ items/i,
    /^RAG 只在/i,
    /^未接入模型/,
    /^当前范围没有供应链/,
    /^消息缓存:/,
    /^移动端已使用/,
    /^暂未找到相关股票/,
    /stocktopicer[\\/]+news[\\/]+news-cache\.json/i,
  ].some((pattern) => pattern.test(line));
}

function createConversation(mode: AgentMode = "quick"): AgentConversation {
  const now = Date.now();
  return {
    id: crypto.randomUUID?.() || `agent-conversation-${now}`,
    title: "新对话",
    mode,
    messages: [],
    createdAt: now,
    updatedAt: now,
  };
}

function sanitizeConversations(value: AgentConversation[]): AgentConversation[] {
  if (!Array.isArray(value)) return [createConversation()];
  const sanitized = value
    .filter((item) => item && typeof item === "object")
    .map((item) => {
      const mode = AGENT_MODES.some((modeItem) => modeItem.id === item.mode) ? item.mode : "quick";
      // Persist only a lightweight transcript: drop the heavy `result` payload and the
      // transient `steps` progress list. Screen/backtest/news results can be large and
      // would otherwise blow past the localStorage quota; the reply text in `content`
      // is enough to reconstruct the conversation on reload.
      const messages: ChatMessage[] = Array.isArray(item.messages)
        ? item.messages
            .filter((message) => message?.role === "user" || message?.role === "assistant")
            .map((message) => ({
              role: message.role,
              content: String(message.content || ""),
              timestamp: Number(message.timestamp || Date.now()),
              error: Boolean(message.error),
            }))
        : [];
      return {
        id: String(item.id || crypto.randomUUID?.() || `agent-conversation-${Date.now()}`),
        title: String(item.title || titleFromMessages(messages) || "新对话"),
        mode,
        messages,
        createdAt: Number(item.createdAt || Date.now()),
        updatedAt: Number(item.updatedAt || item.createdAt || Date.now()),
      };
    })
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .slice(0, MAX_AGENT_CONVERSATIONS);
  return sanitized.length ? sanitized : [createConversation()];
}

function titleFromMessages(messages: ChatMessage[]): string {
  return titleFromMessage(messages.find((message) => message.role === "user")?.content || "");
}

function titleFromMessage(message: string): string {
  const compact = message.replace(/\s+/g, " ").trim();
  if (!compact) return "新对话";
  return compact.length > 24 ? `${compact.slice(0, 24)}...` : compact;
}

function formatConversationTime(timestamp: number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay = date.toDateString() === now.toDateString();
  if (sameDay) return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  return date.toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" });
}

async function requestAgentStream(payload: Record<string, unknown>, onEvent: (event: AgentStreamEvent) => void): Promise<void> {
  if (isTauriRuntime()) return requestTauriAgentStream(payload, onEvent);
  return requestDesktopAgentStream(payload, onEvent);
}

async function requestDesktopAgentStream(payload: Record<string, unknown>, onEvent: (event: AgentStreamEvent) => void): Promise<void> {
  const response = await fetch("/api/agent/stream", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
  if (!response.body?.getReader) throw new Error("当前浏览器无法读取流式响应。");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const blocks = buffer.split(/\r?\n\r?\n/);
    buffer = blocks.pop() || "";
    for (const block of blocks) {
      const event = parseSseBlock(block);
      if (event) onEvent(event);
    }
  }
  const trailing = parseSseBlock(buffer);
  if (trailing) onEvent(trailing);
}

async function requestTauriAgentStream(payload: Record<string, unknown>, onEvent: (event: AgentStreamEvent) => void): Promise<void> {
  const invoke = getTauriInvoke();
  const listen = getTauriListen();
  if (!invoke || !listen) throw new Error("Tauri 事件桥不可用。");
  const runId = String(payload.run_id || "");
  let sawResult = false;
  let unlisten: (() => void) | undefined;
  try {
    unlisten = await listen("agent-stream-event", (event) => {
      const normalized = normalizeAgentStreamEvent(event);
      if (!normalized || normalized.run_id !== runId) return;
      if (normalized.type === "result") sawResult = true;
      onEvent(normalized);
    });
    const response = await invoke<AgentResult>("api_agent_stream", {
      payload: {
        message: String(payload.message || ""),
        run_id: runId,
        mode: String(payload.mode || "quick"),
        context: payload.context,
        platform: payload.platform,
        network: payload.network,
      },
    });
    if (!sawResult && response) onEvent({ run_id: runId, type: "result", response: normalizeAgentResult(response) });
  } finally {
    unlisten?.();
  }
}

