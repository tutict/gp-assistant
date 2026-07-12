import { describe, expect, it } from "vitest";
import { buildAgentStreamPayload } from "./agent";
import type { LlmClientConfig, WatchlistItem } from "../types";

describe("buildAgentStreamPayload", () => {
  it("returns null for blank messages so the Agent cannot send empty requests", () => {
    expect(buildAgentStreamPayload({
      message: "   ",
      runId: "run-empty",
      mode: "quick",
      watchlist: [],
    })).toBeNull();
  });

  it("builds a usable Agent stream payload with mode, model config, and watchlist context", () => {
    const llm: LlmClientConfig = {
      base_url: "https://llm.example.test/v1",
      model: "qwen-plus",
      temperature: 0.2,
      timeout_seconds: 30,
      json_mode: true,
    };
    const watchlist: WatchlistItem[] = [
      { code: "001286.SZ", name: "陕西能源", industry: "深市A股", source: "screen" },
      { code: "300498.SZ", name: "温氏股份", industry: "创业板", source: "agent" },
    ];

    const payload = buildAgentStreamPayload({
      message: "  分析自选股里的电力机会  ",
      runId: "run-1",
      mode: "research",
      llm,
      watchlist,
    });

    expect(payload).toMatchObject({
      message: "分析自选股里的电力机会",
      run_id: "run-1",
      mode: "research",
      llm,
      context: {
        watchlist: [
          { code: "001286.SZ", name: "陕西能源", industry: "深市A股" },
          { code: "300498.SZ", name: "温氏股份", industry: "创业板" },
        ],
      },
    });
  });

  it("caps watchlist context at 50 stocks to keep Agent requests bounded", () => {
    const watchlist = Array.from({ length: 55 }, (_, index): WatchlistItem => ({
      code: `${String(index).padStart(6, "0")}.SZ`,
      name: `股票${index}`,
    }));

    const payload = buildAgentStreamPayload({
      message: "分析自选股",
      runId: "run-limit",
      mode: "expert",
      watchlist,
    });

    const context = payload?.context as { watchlist?: unknown[] };
    expect(context.watchlist).toHaveLength(50);
    expect(context.watchlist?.at(-1)).toMatchObject({ code: "000049.SZ", name: "股票49" });
  });
});