import { Component, useId, useState, type ReactNode } from "react";
import type { AgentEvidenceItem, AgentResult, BacktestResult, HkObserveResult, NewsRagResult, ObserveResult, StockRowView, WatchlistItem } from "../../types";
import { actionResultKind, normalizeScreenRows } from "../../lib/contracts";
import { agentHarnessExecutionLabel, agentHarnessLabel, MAX_AGENT_EVIDENCE_ITEMS } from "../../lib/agent";
import { StockList } from "../StockList";
import { RawJson } from "../RawJson";
import { BacktestResultView } from "./BacktestPanel";
import { NewsRagView } from "./NewsRagPanel";
import { ObserveResultView } from "./ObservePanel";
import { toHkSharedObserveResult } from "./hkObserveAdapter";
import { openExternalUrl } from "../../lib/tauri";

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

export function AgentResultView({ result, watchlist, onToggleWatchlist, reply = result.reply, replyClassName = "agent-final-reply" }: {
  result: AgentResult;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  reply?: string;
  replyClassName?: string;
}) {
  const showDomain = hasDomainView(result);
  const citationPrefix = `agent-evidence-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  return (
    <div className="agent-result-stack">
      {reply && <p className={replyClassName}><CitedText text={reply} result={result} citationPrefix={citationPrefix} /></p>}
      {result.research_context && (
        <div className="agent-research-context" role="status">
          <strong>{result.research_context.stock_code} · {result.research_context.reused ? "沿用研究快照" : "本次研究快照"}</strong>
          <span>取证时间：{new Date(result.research_context.snapshot_at_epoch_ms).toLocaleString("zh-CN", { hour12: false })}</span>
          {result.research_context.reused && <span>本轮沿用已有证据；询问最新资料可重新读取缓存。</span>}
        </div>
      )}
      <ResultRenderBoundary result={result}>
        <AgentAnswerSections result={result} citationPrefix={citationPrefix} />
      </ResultRenderBoundary>
      {showDomain && (
        <ResultRenderBoundary result={result}>
          <AgentDomainResult
            result={result}
            watchlist={watchlist}
            onToggleWatchlist={onToggleWatchlist}
          />
        </ResultRenderBoundary>
      )}
      <ResultRenderBoundary result={result}>
        <AgentEvidenceAndNotes result={result} showRaw={!showDomain} citationPrefix={citationPrefix} />
      </ResultRenderBoundary>
    </div>
  );
}

function hasDomainView(result: AgentResult) {
  const kind = actionResultKind(result);
  if (kind === "backtest" || kind === "news" || kind === "observe") return true;
  if (["screen", "sector", "graph", "trend"].includes(kind)) {
    return (normalizeScreenRows(agentNestedResult(result, kind)) as StockRowView[]).length > 0;
  }
  return false;
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
      : null;
  }
  if (kind === "news") return <NewsRagView result={nested as unknown as NewsRagResult} />;
  if (kind === "observe") {
    const observe = nested.market === "hk" && nested.security && Array.isArray(nested.history)
      ? toHkSharedObserveResult(nested as unknown as HkObserveResult)
      : nested as unknown as ObserveResult;
    return <ObserveResultView result={observe} />;
  }
  return null;
}

const INTENT_LABELS: Record<string, string> = {
  trend_analysis: "趋势分析",
  sector_analysis: "板块分析",
  portfolio_simulation: "组合回测",
  stock_screen: "条件选股",
  stock_snapshot: "个股观察",
  stock_news: "消息研究",
  watchlist_action: "自选操作",
  clarify: "需要补充问题",
  stock_research: "股票研究",
};
const INTENT_META_LABELS: Record<string, string> = {
  quick: "快速",
  expert: "专家",
  research: "研报",
  recent: "近期",
  today: "今日",
};

function intentText(kind: string | undefined, action: string | undefined) {
  return INTENT_LABELS[kind || ""] || INTENT_LABELS[action || ""] || "股票研究";
}
function intentMeta(values: Array<string | null | undefined>) {
  return values.filter(Boolean).map((value) => INTENT_META_LABELS[value || ""] || "").filter(Boolean).join(" · ");
}

function evidenceId(item: AgentEvidenceItem, index: number) {
  return item.id && /^E[1-9]\d*$/.test(item.id) ? item.id : `E${index + 1}`;
}

function CitedText({ text, result, citationPrefix }: { text: string; result: AgentResult; citationPrefix: string }) {
  const evidence = Array.isArray(result.evidence_summary) ? result.evidence_summary.slice(0, MAX_AGENT_EVIDENCE_ITEMS) : [];
  const ids = new Set(evidence.map(evidenceId));
  return <>{String(text).split(/(\[E\d+\])/g).map((part, index) => {
    const id = /^\[(E\d+)\]$/.exec(part)?.[1];
    return id && ids.has(id)
      ? <a key={index} className="agent-inline-citation" href={`#${citationPrefix}-${id}`} aria-label={`查看证据 ${id}`} onClick={event => {
        // The app owns hash navigation; citations only move focus within this answer.
        event.preventDefault();
        const target = event.currentTarget.ownerDocument.getElementById(`${citationPrefix}-${id}`);
        target?.scrollIntoView({ block: "nearest", inline: "nearest" });
        target?.focus({ preventScroll: true });
      }}>{part}</a>
      : part;
  })}</>;
}

const EVIDENCE_STATUS_LABELS: Record<string, string> = {
  candidate: "候选数据，尚未核验", candidate_only: "候选数据，尚未核验", unverified: "尚未核验", verified: "已核验",
  official: "官方披露", official_disclosure: "官方披露", official_disclosure_unaudited: "官方披露，未经审计",
  metadata_only: "仅有披露线索，尚未核验",
};
const EVIDENCE_SOURCE_LABELS: Record<string, string> = {
  primary: "一手来源", official: "官方披露", official_filing: "官方公告", filing: "官方公告",
  financial_snapshot: "财务快照", candidate: "候选数据", media: "媒体报道", news: "新闻报道", user_import: "用户资料",
};
const EVIDENCE_UNIT_LABELS: Record<string, string> = { million: "百万元", billion: "十亿元", percent: "%", per_share: "每股" };

function AgentAnswerSections({ result, citationPrefix }: { result: AgentResult; citationPrefix: string }) {
  const sections = Array.isArray(result.answer_sections) ? result.answer_sections : [];
  const modelSections = Array.isArray(result.model_answer_sections) ? result.model_answer_sections : [];
  if (!sections.length && !modelSections.length) return null;
  return <>
    {sections.length > 0 && (
      <div className="agent-answer-sections">
        {sections.map((section, index) => (
          <article key={String(section.title || "section") + "-" + index}>
            <strong>{section.title || "结论"}</strong>
            {(section.bullets || []).map((bullet, bulletIndex) => <p key={bulletIndex}><CitedText text={bullet} result={result} citationPrefix={citationPrefix} /></p>)}
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
              {(section.bullets || []).map((bullet, bulletIndex) => <p key={bulletIndex}><CitedText text={bullet} result={result} citationPrefix={citationPrefix} /></p>)}
            </article>
          ))}
        </div>
      </div>
    )}
  </>;
}

function EvidenceSourceCard({ item, id, citationPrefix }: { item: AgentEvidenceItem; id: string; citationPrefix: string }) {
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState("");
  let url = "";
  try {
    const parsed = new URL(item.url || "");
    if (["http:", "https:"].includes(parsed.protocol)) url = parsed.href;
  } catch { /* An absent or malformed source remains visible without an open action. */ }
  const openSource = async () => {
    if (!url || opening) return;
    setOpening(true);
    setError("");
    try { await openExternalUrl(url); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "原文暂时无法打开"); }
    finally { setOpening(false); }
  };
  const status = item.verification_status ? EVIDENCE_STATUS_LABELS[item.verification_status] || item.verification_status : "";
  const level = item.source_tier || item.level;
  return <article id={`${citationPrefix}-${id}`} tabIndex={-1}>
    <span>{`${id}${level && level !== "evidence" ? ` · ${EVIDENCE_SOURCE_LABELS[level] || level}` : ""}`}</span>
    <strong>{item.title || item.source || "证据"}</strong>
    <p>{item.summary || item.source || "暂无证据摘要"}</p>
    {item.source && <em>{item.source}</em>}
    {(item.published_at || item.document_id) && <em>披露日期：{item.published_at || "未提供"}</em>}
    {(item.report_period || item.currency || item.unit) && <em>{[item.report_period, item.currency, item.unit ? EVIDENCE_UNIT_LABELS[item.unit] || item.unit : ""].filter(Boolean).join(" · ")}</em>}
    {(status || item.audit_status || item.summary_kind === "article_summary") && <em>{[status, item.audit_status === "unaudited" ? "未经审计" : item.audit_status, item.summary_kind === "article_summary" ? "正文摘要" : ""].filter(Boolean).join(" · ")}</em>}
    {url && <button className="agent-source-open" type="button" disabled={opening} onClick={openSource}>{opening ? "正在打开…" : "查看原文"}</button>}
    {error && <p role="status">{error}</p>}
  </article>;
}

function AgentEvidenceAndNotes({ result, showRaw, citationPrefix }: { result: AgentResult; showRaw: boolean; citationPrefix: string }) {
  const toolCalls = Array.isArray(result.tool_calls) ? result.tool_calls : [];
  const allEvidence = Array.isArray(result.evidence_summary) ? result.evidence_summary : [];
  const evidence = allEvidence.slice(0, MAX_AGENT_EVIDENCE_ITEMS);
  const warnings = Array.isArray(result.warnings) ? result.warnings : [];
  const nextActions = Array.isArray(result.next_actions) ? result.next_actions : [];
  const harness = result.harness;
  const meta = result.intent ? intentMeta([result.intent.mode, result.intent.depth, result.intent.window]) : "";
  const hasProcess = Boolean(harness || result.intent || toolCalls.length || showRaw);
  if (!evidence.length && !warnings.length && !nextActions.length && !hasProcess) return null;
  return (
    <section className="agent-structured-result">
      {evidence.length > 0 && (
        <div className="agent-evidence-grid">
          {evidence.map((item, index) => (
            <EvidenceSourceCard key={String(item.document_id || item.title || "evidence") + "-" + index} item={item} id={evidenceId(item, index)} citationPrefix={citationPrefix} />
          ))}
        </div>
      )}
      {allEvidence.length > evidence.length && <p className="agent-evidence-limit-note">仅显示前 {MAX_AGENT_EVIDENCE_ITEMS} 条证据，更早的引用可能没有对应卡片。</p>}
      {warnings.length > 0 && (
        <div className="agent-warning-list">
          {warnings.map((warning, index) => <p key={index}>{warning}</p>)}
        </div>
      )}
      {nextActions.length > 0 && <p className="agent-next-actions">可以继续：{nextActions.join("、")}</p>}
      {hasProcess && (
        <details className="agent-process">
          <summary>本次过程</summary>
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
              <strong>{intentText(result.intent.kind, result.action)}</strong>
              {meta && <em>{meta}</em>}
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
          {showRaw && <GenericAgentResult result={result} />}
        </details>
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
  return <RawJson result={result} enabled />;
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}
