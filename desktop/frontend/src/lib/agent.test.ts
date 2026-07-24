import { describe, expect, it } from "vitest";
import { agentHarnessExecutionLabel, agentHarnessLabel, buildAgentStreamPayload, MAX_AGENT_EVIDENCE_ITEMS, MAX_AGENT_MESSAGE_CHARS } from "./agent";
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

  it("sends only the latest bounded conversation history to the harness", () => {
    const history = Array.from({ length: 15 }, (_, index) => ({
      role: index % 2 === 0 ? "user" as const : "assistant" as const,
      content: ` message ${index} `,
    }));

    const payload = buildAgentStreamPayload({
      message: "继续比较",
      runId: "run-history",
      mode: "research",
      watchlist: [],
      history,
    });

    expect(payload?.history).toEqual(history.slice(-12).map((item) => ({
      ...item,
      content: item.content.trim(),
    })));
  });

  it("shares the backend message and evidence bounds", () => {
    expect(MAX_AGENT_MESSAGE_CHARS).toBe(8_000);
    expect(MAX_AGENT_EVIDENCE_ITEMS).toBe(16);
  });
});

describe("agentHarnessLabel", () => {
  it("exposes the active versioned method without impersonating an investor", () => {
    expect(agentHarnessLabel("hot_money_early_v1")).toBe("游资早期研究 v1");
    expect(agentHarnessLabel("value_compounder_v1")).toBe("价值复利研究 v1");
    expect(agentHarnessLabel("deterministic_v1")).toBe("本地快速分析");
    expect(agentHarnessExecutionLabel("deterministic_v1", false)).toBe("确定性工具执行");
    expect(agentHarnessExecutionLabel("hot_money_early_v1", true, "qwen-plus")).toBe("模型综合 · qwen-plus");
    expect(agentHarnessExecutionLabel("value_compounder_v1", false)).toBe("本地工具降级");
  });
});
