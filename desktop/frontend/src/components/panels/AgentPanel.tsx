import { FileSearch, History, LoaderCircle, Menu, PanelLeftClose, PanelLeftOpen, Plus, Search, Send, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AgentResult, AgentStreamEvent, LlmSettings, StockRowView, WatchlistItem } from "../../types";
import { buildTauriAgentPayload, getTauriInvoke, getTauriListen, isTauriRuntime } from "../../lib/tauri";
import { activeLlmProvider, buildLlmConfig, normalizeAgentResult, normalizeAgentStreamEvent, parseSseBlock } from "../../lib/contracts";
import { buildAgentStreamPayload, MAX_AGENT_MESSAGE_CHARS } from "../../lib/agent";
import { useLocalStorage } from "../../hooks/useLocalStorage";
import { AgentResultView } from "./AgentResultView";
import { AgentRunDrawer } from "./AgentRunDrawer";
import { LlmSettingsPanel } from "./LlmSettingsPanel";
import { IconButton } from "../ui/IconButton";

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
  runId?: string;
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
  { id: "expert", label: "专家模式", hint: "游资早期框架：环境、主线、情绪周期与失效条件" },
  { id: "research", label: "研报模式", hint: "价值复利框架：企业质量、资本配置与估值" },
] as const;

type AgentMode = typeof AGENT_MODES[number]["id"];

const AGENT_HISTORY_KEY = "stock-optimizer-agent-conversations";
const AGENT_ACTIVE_KEY = "stock-optimizer-agent-active-conversation";
const AGENT_RAIL_COLLAPSED_KEY = "stock-optimizer-agent-rail-collapsed";
const AGENT_MOBILE_DRAWER_QUERY = "(max-width: 768px)";
const MAX_AGENT_CONVERSATIONS = 40;
const MAX_AGENT_RUN_ID_CHARS = 256;

export function AgentPanel({ llmSettings, onLlmSettingsChange, watchlist, onWatchlistChange }: AgentPanelProps) {
  const [conversations, setConversations, quotaError] = useLocalStorage<AgentConversation[]>(
    AGENT_HISTORY_KEY,
    [createConversation()],
    sanitizeAgentConversations,
  );
  const [activeConversationId, setActiveConversationId] = useLocalStorage<string>(AGENT_ACTIVE_KEY, "");
  const [railCollapsed, setRailCollapsed] = useLocalStorage<boolean>(AGENT_RAIL_COLLAPSED_KEY, false);
  const [input, setInput] = useState("");
  const [conversationSearch, setConversationSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [replayOpen, setReplayOpen] = useState(false);
  const [replayRunId, setReplayRunId] = useState<string>();
  const [finishedRunId, setFinishedRunId] = useState<string>();
  const threadRef = useRef<HTMLDivElement>(null);
  const replayTriggerRef = useRef<HTMLElement | null>(null);
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
    setReplayOpen(false);
    setReplayRunId(undefined);
  }, [activeConversationId]);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;

    const mobileDrawerQuery = window.matchMedia(AGENT_MOBILE_DRAWER_QUERY);
    const syncRailForViewport = () => {
      if (mobileDrawerQuery.matches) {
        setRailCollapsed(true);
      }
    };

    syncRailForViewport();
    mobileDrawerQuery.addEventListener?.("change", syncRailForViewport);
    mobileDrawerQuery.addListener?.(syncRailForViewport);

    return () => {
      mobileDrawerQuery.removeEventListener?.("change", syncRailForViewport);
      mobileDrawerQuery.removeListener?.(syncRailForViewport);
    };
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
    if (typeof window !== "undefined" && window.matchMedia?.(AGENT_MOBILE_DRAWER_QUERY).matches) {
      setRailCollapsed(true);
    }
  }, [activeConversation?.mode, setActiveConversationId, setConversations, setRailCollapsed]);

  const switchConversation = useCallback((conversationId: string) => {
    setActiveConversationId(conversationId);
    setInput("");
    if (typeof window !== "undefined" && window.matchMedia?.(AGENT_MOBILE_DRAWER_QUERY).matches) {
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

  const openRunHistory = useCallback((trigger: HTMLElement) => {
    replayTriggerRef.current = trigger;
    setReplayRunId(undefined);
    setReplayOpen(true);
  }, []);

  const openRunReplay = useCallback((runId: string, trigger: HTMLElement) => {
    replayTriggerRef.current = trigger;
    setReplayRunId(runId);
    setReplayOpen(true);
  }, []);

  const closeRunReplay = useCallback(() => setReplayOpen(false), []);

  const send = useCallback(async () => {
    const text = input.trim();
    const conversationId = activeConversation?.id;
    const mode = activeConversation?.mode || "quick";
    if (!text || loading || !conversationId) return;

    const runId = crypto.randomUUID?.() || `agent-${Date.now()}`;
    const now = Date.now();
    const userMessage: ChatMessage = { role: "user", content: text, timestamp: now };
    const assistantMessage: ChatMessage = { role: "assistant", content: "准备中...", timestamp: now, runId, steps: [] };
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
        setFinishedRunId(runId);
      } else if (event.type === "error") {
        patchAssistant({ content: event.message || "智能体执行失败。", error: true, steps: undefined });
        setFinishedRunId(runId);
      }
    };

    try {
      const payload = buildAgentStreamPayload({
        message: text,
        runId,
        conversationId,
        llm: buildLlmConfig(llmSettings),
        mode,
        watchlist,
        history: messages.map((message) => ({ role: message.role, content: message.content })),
      });
      if (!payload) return;
      await requestAgentStream(payload, applyEvent);
    } catch (err) {
      patchAssistant({ content: `错误：${(err as Error).message}`, error: true, steps: undefined });
      setFinishedRunId(runId);
    } finally {
      setLoading(false);
    }
  }, [activeConversation?.id, activeConversation?.mode, input, llmSettings, loading, messages, updateConversation, watchlist]);

  return (
    <div className={`panel-container agent-panel agent-workspace ${railCollapsed ? "rail-collapsed" : ""}`}>
      <aside className="agent-rail" aria-label="Agent 会话历史">
        <IconButton
          className="agent-rail-toggle"
          onClick={() => setRailCollapsed((collapsed) => !collapsed)}
          label={railCollapsed ? "展开会话历史" : "折叠会话历史"}
          icon={railCollapsed ? <PanelLeftOpen size={18} aria-hidden="true" /> : <PanelLeftClose size={18} aria-hidden="true" />}
        />

        <label className="agent-rail-search">
          <Search size={15} aria-hidden="true" />
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
          <Plus size={16} aria-hidden="true" /><strong className="agent-new-chat-label">新建对话</strong>
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
                      <Trash2 size={14} aria-hidden="true" />
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
          <LlmSettingsPanel settings={llmSettings} onChange={onLlmSettingsChange} presentation="dialog" />
        </div>
      </aside>

      <section className="agent-chat-stage" aria-label={`Agent 对话：${activeConversation?.title || "新对话"}，${activeMode.label}${activeProvider?.model ? `，模型 ${activeProvider.model}` : ""}`}>
        <button
          type="button"
          className="agent-rail-scrim"
          onClick={() => setRailCollapsed(true)}
          aria-label="关闭对话栏"
        />
        <div className="agent-mobile-actions" aria-label="Agent 移动端操作">
          <IconButton
            className="agent-mobile-menu"
            onClick={() => setRailCollapsed(false)}
            label="展开对话栏"
            icon={<Menu size={18} aria-hidden="true" />}
          />
          <IconButton
            className="agent-mobile-new"
            onClick={() => {
              startNewChat();
              setRailCollapsed(true);
            }}
            label="新建对话"
            icon={<Plus size={18} aria-hidden="true" />}
          />
          <IconButton
            className="agent-mobile-history"
            onClick={(event) => openRunHistory(event.currentTarget)}
            label="运行历史"
            title="运行历史"
            icon={<History size={18} aria-hidden="true" />}
          />
        </div>
        <div className="agent-thread-toolbar">
          <strong>{activeConversation?.title || "新对话"}</strong>
          <IconButton
            className="agent-thread-history"
            onClick={(event) => openRunHistory(event.currentTarget)}
            label="运行历史"
            title="运行历史"
            icon={<History size={18} aria-hidden="true" />}
          />
        </div>
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
                {msg.role === "assistant" && msg.runId && (
                  <IconButton
                    className="agent-message-replay"
                    onClick={(event) => openRunReplay(msg.runId!, event.currentTarget)}
                    label="查看本次运行复盘"
                    title="查看本次运行复盘"
                    icon={<FileSearch size={15} aria-hidden="true" />}
                  />
                )}
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
<textarea
            className="agent-input"
            value={input}
            maxLength={MAX_AGENT_MESSAGE_CHARS}
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
            <span>{activeMode.label}</span>
          </div>
          <div className="agent-composer-footer">
            <button type="button" className="send-btn" onClick={send} disabled={loading || !input.trim()} aria-label="发送">
              {loading ? <LoaderCircle className="spin" size={18} aria-hidden="true" /> : <Send size={18} aria-hidden="true" />}
            </button>
          </div>
        </div>
        <AgentRunDrawer
          open={replayOpen}
          activeConversationId={activeConversation?.id}
          initialRunId={replayRunId}
          finishedRunId={finishedRunId}
          returnFocusElement={replayTriggerRef.current}
          watchlist={watchlist}
          onToggleWatchlist={toggleWatchlist}
          onClose={closeRunReplay}
        />
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
  return (
    <div className="agent-empty-state">
      <h2>开始对话</h2>

      <div className="agent-mode-tabs" role="group" aria-label="对话模式">
        {AGENT_MODES.map((item) => (
          <button
            key={item.id}
            type="button"
            className={item.id === mode ? "active" : ""}
            aria-pressed={item.id === mode}
            aria-label={`${item.label}：${item.hint}`}
            onClick={() => onModeChange(item.id)}
            title={item.hint}
          >
            {item.label}
          </button>
        ))}
      </div>

      {!activeModel && <p role="status">请先配置模型</p>}
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

export function sanitizeAgentConversations(value: AgentConversation[]): AgentConversation[] {
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
              ...(typeof message.runId === "string" && message.runId.trim() && message.runId.trim().length <= MAX_AGENT_RUN_ID_CHARS
                ? { runId: message.runId.trim() }
                : {}),
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
      payload: buildTauriAgentPayload({ ...payload, run_id: runId }),
    });
    if (!sawResult && response) onEvent({ run_id: runId, type: "result", response: normalizeAgentResult(response) });
  } finally {
    unlisten?.();
  }
}
