import type { LlmClientConfig, WatchlistItem } from "../types";

export type AgentMode = "quick" | "expert" | "research";

export interface AgentStreamPayloadInput {
  message: string;
  runId: string;
  mode: AgentMode;
  llm?: LlmClientConfig;
  watchlist: WatchlistItem[];
}

export function buildAgentStreamPayload(input: AgentStreamPayloadInput): Record<string, unknown> | null {
  const message = input.message.trim();
  if (!message) return null;
  return {
    message,
    run_id: input.runId,
    llm: input.llm,
    mode: input.mode,
    context: {
      watchlist: input.watchlist.slice(0, 50).map((item) => ({
        code: item.code,
        name: item.name,
        industry: item.industry,
      })),
    },
  };
}