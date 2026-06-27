import { useCallback, useEffect, useRef, useState } from "react";
import type { AgentResult, AgentStreamEvent, LlmSettings, StockRowView } from "../../types";
import { getTauriInvoke, isTauriRuntime } from "../../lib/tauri";
import { actionResultKind, buildLlmConfig, normalizeAgentStreamEvent, normalizeScreenRows, parseSseBlock } from "../../lib/contracts";
import { escapeHtml, formatNumber, formatSignedPercent } from "../../lib/format";
import { StockList } from "../StockList";

interface AgentPanelProps {
  llmSettings: LlmSettings | null;
  onLlmSettingsChange: (settings: LlmSettings | null) => void;
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

export function AgentPanel({ llmSettings }: AgentPanelProps) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const threadRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [messages, loading]);

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || loading) return;
    const runId = crypto.randomUUID?.() || `agent-${Date.now()}`;
    const userMessage: ChatMessage = { role: "user", content: text, timestamp: Date.now() };
    const assistantMessage: ChatMessage = { role: "assistant", content: "Preparing...", timestamp: Date.now(), steps: [] };
    setMessages((prev) => [...prev, userMessage, assistantMessage]);
    setInput("");
    setLoading(true);

    const patchAssistant = (patch: Partial<ChatMessage>) => {
      setMessages((prev) => prev.map((message, index) => index === prev.length - 1 ? { ...message, ...patch } : message));
    };

    const applyEvent = (event: AgentStreamEvent) => {
      if (event.run_id && event.run_id !== runId) return;
      if (event.type === "status") {
        const step = {
          stage: event.stage || `stage-${Date.now()}`,
          label: event.label || event.stage || "Running",
          percent: Number(event.percent || 0),
        };
        setMessages((prev) => prev.map((message, index) => {
          if (index !== prev.length - 1) return message;
          return {
            ...message,
            content: event.label || event.stage || "Running...",
            steps: mergeStep(message.steps || [], step),
          };
        }));
      } else if (event.type === "result") {
        const result = event.response || {};
        patchAssistant({ content: result.reply || "Done.", result, steps: undefined });
      } else if (event.type === "error") {
        patchAssistant({ content: event.message || "Agent failed.", error: true });
      }
    };

    try {
      await requestAgentStream({ message: text, run_id: runId, llm: buildLlmConfig(llmSettings) }, applyEvent);
    } catch (err) {
      patchAssistant({ content: `Error: ${(err as Error).message}`, error: true });
    } finally {
      setLoading(false);
    }
  }, [input, llmSettings, loading]);

  return (
    <div className="panel-container agent-panel">
      <div className="agent-header">
        <h2>Agent</h2>
        {llmSettings?.model && <span className="agent-model">{llmSettings.model}</span>}
      </div>

      <div className="agent-thread" ref={threadRef}>
        {messages.length === 0 && <div className="agent-empty"><p>Ask the local stock agent a question.</p></div>}
        {messages.map((msg, i) => (
          <article key={i} className={`agent-message ${msg.role} ${msg.error ? "error" : ""}`}>
            <div className="agent-message-meta"><span>{msg.role === "user" ? "You" : "Assistant"}</span><time>{new Date(msg.timestamp).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</time></div>
            <div className="agent-message-body">
              {msg.steps?.length ? <AgentSteps steps={msg.steps} /> : null}
              <p className="agent-final-reply">{msg.content}</p>
              {msg.result && <AgentResultView result={msg.result} />}
            </div>
          </article>
        ))}
      </div>

      <div className="agent-input-bar">
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
          placeholder="Type a message, Enter to send"
          rows={2}
          disabled={loading}
        />
        <button type="button" className="send-btn" onClick={send} disabled={loading || !input.trim()}>{loading ? "..." : "Send"}</button>
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

function AgentResultView({ result }: { result: AgentResult }) {
  const kind = actionResultKind(result);
  const nested = result.data || {};
  if (kind === "backtest") {
    const metrics = (nested.metrics || {}) as Record<string, number>;
    return (
      <div className="agent-result-card">
        <div className="metric-strip">
          <div className="metric"><span>Total return</span><strong>{metrics.total_return != null ? formatSignedPercent(metrics.total_return * 100) : "--"}</strong></div>
          <div className="metric"><span>Max drawdown</span><strong>{metrics.max_drawdown != null ? formatSignedPercent(metrics.max_drawdown * 100) : "--"}</strong></div>
          <div className="metric"><span>Stocks</span><strong>{formatNumber(metrics.num_stocks)}</strong></div>
        </div>
      </div>
    );
  }
  if (["screen", "sector", "graph", "trend"].includes(kind)) {
    const rows = normalizeScreenRows(nested) as StockRowView[];
    return <StockList items={rows} watchlist={[]} onToggleWatchlist={() => {}} />;
  }
  if (kind === "news") return <pre className="raw-result">{JSON.stringify(nested, null, 2)}</pre>;
  if (kind === "observe") return <pre className="raw-result">{JSON.stringify(nested, null, 2)}</pre>;
  return <details className="raw-json"><summary>Raw JSON</summary><pre>{JSON.stringify(result, null, 2)}</pre></details>;
}

function mergeStep(steps: AgentStep[], step: AgentStep): AgentStep[] {
  const next = [...steps];
  const index = next.findIndex((item) => item.stage === step.stage);
  if (index >= 0) next[index] = step;
  else next.push(step);
  return next;
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
  if (!response.body?.getReader) throw new Error("This browser cannot read SSE streams.");
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
  const listen = (window as unknown as { __TAURI__?: { event?: { listen?: (event: string, handler: (event: unknown) => void) => Promise<() => void> } } }).__TAURI__?.event?.listen;
  if (!invoke || !listen) throw new Error("Tauri event bridge is not available.");
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
      },
    });
    if (!sawResult && response) onEvent({ run_id: runId, type: "result", response });
  } finally {
    unlisten?.();
  }
}
