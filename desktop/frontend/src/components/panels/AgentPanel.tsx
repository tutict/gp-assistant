import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AgentResult, AgentStreamEvent, BacktestResult, LlmSettings, NewsRagResult, ObserveResult, StockRowView, WatchlistItem } from "../../types";
import { getTauriInvoke, getTauriListen, isTauriRuntime } from "../../lib/tauri";
import { actionResultKind, activeLlmProvider, buildLlmConfig, normalizeAgentStreamEvent, normalizeScreenRows, parseSseBlock } from "../../lib/contracts";
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
  { id: "expert", label: "专家模式", hint: "要求更完整的依据、风险和下一步动作" },
  { id: "research", label: "研报模式", hint: "聚焦上下游、公告和消息链路" },
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
  }, [setActiveConversationId]);

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
        updateConversation(conversationId, (conversation) => ({
          ...conversation,
          messages: conversation.messages.map((message, index) => {
            if (index !== conversation.messages.length - 1) return message;
            return {
              ...message,
              content: event.label || event.stage || "运行中...",
              steps: mergeStep(message.steps || [], step),
            };
          }),
          updatedAt: Date.now(),
        }));
      } else if (event.type === "result") {
        const result = event.response || {};
        patchAssistant({ content: result.reply || "已完成。", result, steps: undefined });
      } else if (event.type === "error") {
        patchAssistant({ content: event.message || "智能体执行失败。", error: true, steps: undefined });
      }
    };

    try {
      await requestAgentStream({ message: text, run_id: runId, llm: buildLlmConfig(llmSettings), mode }, applyEvent);
    } catch (err) {
      patchAssistant({ content: `错误：${(err as Error).message}`, error: true, steps: undefined });
    } finally {
      setLoading(false);
    }
  }, [activeConversation?.id, activeConversation?.mode, input, llmSettings, loading, updateConversation]);

  return (
    <div className={`panel-container agent-panel agent-workspace ${railCollapsed ? "rail-collapsed" : ""}`}>
      <aside className="agent-rail" aria-label="Agent 会话历史">
        <button
          type="button"
          className="agent-rail-toggle"
          onClick={() => setRailCollapsed((collapsed) => !collapsed)}
          aria-label={railCollapsed ? "展开对话历史" : "折叠对话历史"}
          title={railCollapsed ? "展开对话历史" : "折叠对话历史"}
        >
          <span />
          <span />
        </button>

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
            <strong>{sortedConversations.length}</strong>
          </div>
          <div className="agent-history-list">
            {sortedConversations.map((conversation) => (
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
        </div>

        <div className="agent-rail-footer">
          <span>当前模型</span>
          <strong>{activeProvider?.model || "未配置模型"}</strong>
        </div>
      </aside>

      <section className="agent-chat-stage" aria-label="Agent 对话">
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
                <p className="agent-final-reply">{msg.content}</p>
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
            placeholder={`给股选优 Agent 发送消息，当前为${activeMode.label}`}
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
              {loading ? "..." : "↑"}
            </button>
          </div>
        </div>

        <LlmSettingsPanel settings={llmSettings} onChange={onLlmSettingsChange} />
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
      },
    });
    if (!sawResult && response) onEvent({ run_id: runId, type: "result", response });
  } finally {
    unlisten?.();
  }
}
