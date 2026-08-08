import { beforeEach, describe, expect, it, vi } from "vitest";

const { getJson } = vi.hoisted(() => ({ getJson: vi.fn() }));
vi.mock("./tauri", () => ({ getJson }));

import { getAgentRun, listAgentRuns, normalizeAgentRunDetail, normalizeAgentRunSummary } from "./agentRuns";

describe("agent run ledger client", () => {
  beforeEach(() => getJson.mockReset());

  it("normalizes bounded summaries and rejects missing run ids", () => {
    expect(normalizeAgentRunSummary({
      run_id: "run-1",
      conversation_id: "conversation-1",
      question: "Review the market",
      mode: "expert",
      status: "corrupt-status",
      started_at_epoch_ms: 100,
      completed_at_epoch_ms: null,
      duration_ms: "",
      error: "x".repeat(2_100),
      request: { llm: { api_key: "must-not-leak" } },
    })).toEqual({
      runId: "run-1",
      conversationId: "conversation-1",
      question: "Review the market",
      mode: "expert",
      status: "unknown",
      startedAtEpochMs: 100,
      completedAtEpochMs: undefined,
      durationMs: undefined,
      error: "x".repeat(2_000),
    });

    expect(normalizeAgentRunSummary({ question: "missing id" })).toBeNull();
    expect(normalizeAgentRunSummary({ run_id: "   " })).toBeNull();
  });

  it("normalizes detail events and results without exposing the stored request", () => {
    const detail = normalizeAgentRunDetail({
      run_id: "run-2",
      question: "Trace evidence",
      mode: "research",
      status: "completed",
      started_at_epoch_ms: 200,
      events: [
        null,
        "bad",
        {},
        { type: 123, label: "invalid" },
        { payload: '{"type":"result","message":"Complete"}' },
        { type: "status", label: "Validated", percent: 94 },
      ],
      result: { reply: "Complete" },
      request: { llm: { api_key: "must-not-leak" } },
    });

    expect(detail?.events).toEqual([
      { type: "result", message: "Complete" },
      { type: "status", label: "Validated", percent: 94 },
    ]);
    expect(detail?.result).toMatchObject({
      reply: "Complete",
      tool_calls: [],
      evidence_summary: [],
      warnings: [],
    });
    expect(detail).not.toHaveProperty("request");

    expect(normalizeAgentRunDetail({
      run_id: "run-array-result",
      events: [],
      result: [{ reply: "invalid" }],
    })?.result).toBeUndefined();
  });

  it("requests current-conversation and all-run summaries with stable URLs", async () => {
    getJson.mockResolvedValue({ runs: [] });

    await listAgentRuns({ conversationId: "conversation/1" });
    await listAgentRuns({});

    expect(getJson).toHaveBeenNthCalledWith(
      1,
      "/api/agent/runs?limit=50&conversation_id=conversation%2F1",
      { signal: undefined },
    );
    expect(getJson).toHaveBeenNthCalledWith(2, "/api/agent/runs?limit=50", { signal: undefined });
  });

  it("clamps list limits, forwards abort signals, and tolerates malformed lists", async () => {
    const controller = new AbortController();
    getJson.mockResolvedValueOnce({
      runs: [
        { run_id: "run-valid", status: "running", started_at_epoch_ms: 10 },
        { run_id: "   " },
      ],
    });

    await expect(listAgentRuns({ limit: 500, signal: controller.signal })).resolves.toEqual([
      expect.objectContaining({ runId: "run-valid", status: "running" }),
    ]);
    expect(getJson).toHaveBeenLastCalledWith("/api/agent/runs?limit=200", { signal: controller.signal });

    getJson.mockResolvedValueOnce({ runs: "invalid" });
    await expect(listAgentRuns({ limit: 0 })).resolves.toEqual([]);
    expect(getJson).toHaveBeenLastCalledWith("/api/agent/runs?limit=1", { signal: undefined });
  });

  it("encodes detail run ids and returns null for missing ledger records", async () => {
    const controller = new AbortController();
    getJson.mockResolvedValueOnce({
      run: {
        run_id: "run/1",
        status: "completed",
        started_at_epoch_ms: 100,
        events: [],
        result: { reply: "Done" },
      },
    });

    await expect(getAgentRun("run/1", controller.signal)).resolves.toEqual(
      expect.objectContaining({ runId: "run/1", result: expect.objectContaining({ reply: "Done" }) }),
    );
    expect(getJson).toHaveBeenLastCalledWith("/api/agent/runs/run%2F1", { signal: controller.signal });

    getJson.mockResolvedValueOnce({ run: null });
    await expect(getAgentRun("missing")).resolves.toBeNull();

    getJson.mockResolvedValueOnce({});
    await expect(getAgentRun("omitted")).resolves.toBeNull();
  });
});
