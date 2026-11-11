import type { AgentEvidenceItem, AgentResearchContext, AgentResult, LlmClientConfig, WatchlistItem } from "../types";

export type AgentMode = "quick" | "expert" | "research";

export const MAX_AGENT_MESSAGE_CHARS = 8_000;
export const MAX_AGENT_EVIDENCE_ITEMS = 16;

export interface AgentHistoryMessage {
  role: "user" | "assistant";
  content: string;
  result?: AgentResult;
  runId?: string;
  error?: boolean;
}

export function boundedAgentRunId(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const id = value.trim();
  return id && id !== "." && id !== ".." && new TextEncoder().encode(id).byteLength <= 256 ? id : undefined;
}

function boundedEvidenceValue(value: unknown, depth = 0): unknown {
  if (typeof value === "string") return value.slice(0, 600);
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "boolean" || value === null) return value;
  if (depth >= 4) return null;
  if (Array.isArray(value)) return value.slice(0, 16).map((item) => boundedEvidenceValue(item, depth + 1));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).slice(0, 48)
    .map(([key, item]) => [key, boundedEvidenceValue(item, depth + 1)]));
  return null;
}

function previousResearchContext(history: AgentHistoryMessage[], market: "cn" | "hk"): AgentResearchContext | null {
  if (market !== "hk") return null;
  const previous = [...history].reverse().find((item) => item.role === "assistant" && !item.error && item.result?.research_context);
  const context = previous?.result?.research_context;
  if (!context || context.market !== "hk" || !/^\d{5}\.HK$/.test(context.stock_code)
    || !Number.isFinite(context.snapshot_at_epoch_ms) || context.snapshot_at_epoch_ms <= 0) return null;
  const evidence = (Array.isArray(context.evidence) ? context.evidence : []).slice(0, MAX_AGENT_EVIDENCE_ITEMS)
    .filter((item) => item && typeof item.document_id === "string" && item.document_id.length <= 256
      && item.stock_codes?.includes(context.stock_code))
    .map((item, index): AgentEvidenceItem => ({
      id: item.id && /^E[1-9]\d*$/.test(item.id) && item.id.length <= 64 ? item.id : `E${index + 1}`, document_id: item.document_id, chunk_id: item.chunk_id?.slice(0, 256), citation_id: item.citation_id?.slice(0, 64),
      title: item.title?.slice(0, 160), source: item.source?.slice(0, 200), level: item.level?.slice(0, 64), source_tier: item.source_tier?.slice(0, 64),
      summary: item.summary?.slice(0, 1600), excerpt: item.excerpt?.slice(0, 1600), summary_kind: item.summary_kind?.slice(0, 64),
      url: item.url && item.url.length <= 2048 ? item.url : null, published_at: item.published_at?.slice(0, 128),
      report_period: item.report_period?.slice(0, 64), period_end: item.period_end?.slice(0, 128),
      currency: item.currency?.slice(0, 32), unit: item.unit?.slice(0, 64),
      verification_status: item.verification_status?.slice(0, 64), audit_status: item.audit_status?.slice(0, 64),
      stock_codes: [context.stock_code], remote_export_allowed: item.remote_export_allowed === true,
      metadata: boundedEvidenceValue(item.metadata || {}) as Record<string, unknown>,
    }));
  const sourceRunId = boundedAgentRunId(previous?.runId) || boundedAgentRunId(context.source_run_id);
  return { market: "hk", ...(sourceRunId ? { source_run_id: sourceRunId } : {}), stock_code: context.stock_code, snapshot_at_epoch_ms: context.snapshot_at_epoch_ms, evidence,
    data_gaps: (Array.isArray(context.data_gaps) ? context.data_gaps : []).filter((gap) => typeof gap === "string").slice(0, 16).map((gap) => gap.slice(0, 400)) };
}

export interface AgentStreamPayloadInput {
  message: string;
  runId: string;
  conversationId?: string;
  mode: AgentMode;
  llm?: LlmClientConfig;
  watchlist: WatchlistItem[];
  history?: AgentHistoryMessage[];
  market?: "cn" | "hk";
}

export function agentHarnessLabel(profileId?: string | null): string {
  if (profileId === "hot_money_early_v1") return "游资早期研究 v1";
  if (profileId === "value_compounder_v1") return "价值复利研究 v1";
  if (profileId === "quick_agent_v1") return "快速模型分析";
  return "本地快速分析";
}

export function agentHarnessExecutionLabel(
  profileId?: string | null,
  modelUsed?: boolean,
  model?: string | null,
): string {
  if (profileId === "deterministic_v1") return "确定性工具执行";
  if (!modelUsed) return "本地工具降级";
  return `模型综合${model ? ` · ${model}` : ""}`;
}

export function buildAgentStreamPayload(input: AgentStreamPayloadInput): Record<string, unknown> | null {
  const message = input.message.trim();
  if (!message) return null;
  const researchContext = previousResearchContext(input.history || [], input.market || "cn");
  return {
    message,
    run_id: input.runId,
    ...(input.market ? { market: input.market } : {}),
    ...(input.conversationId?.trim() ? { conversation_id: input.conversationId.trim() } : {}),
    llm: input.llm,
    mode: input.mode,
    ...(researchContext ? { research_context: researchContext } : {}),
    history: (input.history || [])
      .slice(-12)
      .map((item) => ({ role: item.role, content: item.content.trim().slice(0, 2_000) }))
      .filter((item) => item.content),
    context: {
      ...(researchContext ? { stock_code: researchContext.stock_code } : {}),
      watchlist: input.watchlist.slice(0, 50).map((item) => ({
        code: item.code,
        name: item.name,
        industry: item.industry,
      })),
    },
  };
}
