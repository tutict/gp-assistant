import type { LlmClientConfig, WatchlistItem } from "../types";

export type AgentMode = "quick" | "expert" | "research";

export interface AgentHistoryMessage {
  role: "user" | "assistant";
  content: string;
}

export interface AgentStreamPayloadInput {
  message: string;
  runId: string;
  mode: AgentMode;
  llm?: LlmClientConfig;
  watchlist: WatchlistItem[];
  history?: AgentHistoryMessage[];
}

export function agentHarnessLabel(profileId?: string | null): string {
  if (profileId === "hot_money_early_v1") return "游资早期研究 v1";
  if (profileId === "value_compounder_v1") return "价值复利研究 v1";
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
  return {
    message,
    run_id: input.runId,
    llm: input.llm,
    mode: input.mode,
    history: (input.history || [])
      .slice(-12)
      .map((item) => ({ role: item.role, content: item.content.trim().slice(0, 2_000) }))
      .filter((item) => item.content),
    context: {
      watchlist: input.watchlist.slice(0, 50).map((item) => ({
        code: item.code,
        name: item.name,
        industry: item.industry,
      })),
    },
  };
}
