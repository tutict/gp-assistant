import { Component, type ReactNode } from "react";
import type { AgentResult, BacktestResult, NewsRagResult, ObserveResult, StockRowView, WatchlistItem } from "../../types";
import { actionResultKind, normalizeScreenRows } from "../../lib/contracts";
import { agentHarnessExecutionLabel, agentHarnessLabel, MAX_AGENT_EVIDENCE_ITEMS } from "../../lib/agent";
import { StockList } from "../StockList";
import { RawJson } from "../RawJson";
import { BacktestResultView } from "./BacktestPanel";
import { NewsRagView } from "./NewsRagPanel";
import { ObserveResultView } from "./ObservePanel";

export const AGENT_RESULT_UNAVAILABLE_TEXT = "结果不可用";

interface ResultRenderBoundaryState {
  failed: boolean;
  result: AgentResult;
}

class ResultRenderBoundary extends Component<{
  children: ReactNode;
  result: AgentResult;
}, ResultRenderBoundaryState> {
  state: ResultRenderBoundaryState = { failed: false, result: this.props.result };

  static getDerivedStateFromError(): Partial<ResultRenderBoundaryState> {
    return { failed: true };
  }

  static getDerivedStateFromProps(
    props: { result: AgentResult },
    state: ResultRenderBoundaryState,
  ): Partial<ResultRenderBoundaryState> | null {
    return props.result === state.result ? null : { failed: false, result: props.result };
  }

  render() {
    return this.state.failed
      ? <p role="status">{AGENT_RESULT_UNAVAILABLE_TEXT}</p>
      : this.props.children;
  }
}

export function AgentResultView({ result, watchlist, onToggleWatchlist }: {
  result: AgentResult;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
}) {
  return (
    <div className="agent-result-stack">
      <ResultRenderBoundary result={result}>
        <AgentStructuredResult result={result} />
      </ResultRenderBoundary>
      <ResultRenderBoundary result={result}>
        <AgentDomainResult
          result={result}
          watchlist={watchlist}
          onToggleWatchlist={onToggleWatchlist}
        />
      </ResultRenderBoundary>
    </div>
  );
}

function AgentDomainResult({ result, watchlist, onToggleWatchlist }: {
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

function AgentStructuredResult({ result }: { result: AgentResult }) {
  const toolCalls = Array.isArray(result.tool_calls) ? result.tool_calls : [];
  const evidence = Array.isArray(result.evidence_summary) ? result.evidence_summary.slice(0, MAX_AGENT_EVIDENCE_ITEMS) : [];
  const sections = Array.isArray(result.answer_sections) ? result.answer_sections : [];
  const modelSections = Array.isArray(result.model_answer_sections) ? result.model_answer_sections : [];
  const warnings = Array.isArray(result.warnings) ? result.warnings : [];
  const nextActions = Array.isArray(result.next_actions) ? result.next_actions : [];
  const harness = result.harness;
  if (!harness && !result.intent && !toolCalls.length && !evidence.length && !sections.length && !modelSections.length && !warnings.length && !nextActions.length) return null;

  return (
    <section className="agent-structured-result">
      {harness && (
        <div className="agent-harness-meta" aria-label="本次回答方法与模型状态">
          <span>方法</span>
          <strong>{agentHarnessLabel(harness.profile_id)}</strong>
          <em>{agentHarnessExecutionLabel(harness.profile_id, harness.model_used, harness.model)}</em>
        </div>
      )}
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

      {modelSections.length > 0 && (
        <div className="agent-model-answer-block">
          <div className="agent-model-answer-head">
            <strong>模型推断</strong>
            <span>按 [E#] 邻近引用本地证据，仍需核验原始数据</span>
          </div>
          <div className="agent-answer-sections agent-model-answer-sections">
            {modelSections.map((section, index) => (
              <article key={String(section.title || "model-section") + "-" + index}>
                <strong>{section.title || "研究推断"}</strong>
                {(section.bullets || []).map((bullet, bulletIndex) => <p key={bulletIndex}>{bullet}</p>)}
              </article>
            ))}
          </div>
        </div>
      )}

      {evidence.length > 0 && (
        <div className="agent-evidence-grid">
          {evidence.map((item, index) => (
            <article key={String(item.title || "evidence") + "-" + index}>
              <span>{`E${index + 1} · ${item.level || "evidence"}`}</span>
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
  return <RawJson result={result} />;
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}
