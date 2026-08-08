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

    expect(normalizeAgentRunSummary({
      run_id: ` ${"r".repeat(256)} `,
      conversation_id: ` ${"c".repeat(256)} `,
      question: "q".repeat(8_100),
      mode: "m".repeat(100),
    })).toMatchObject({
      runId: "r".repeat(256),
      conversationId: "c".repeat(256),
      question: "q".repeat(8_000),
      mode: "m".repeat(64),
    });

    expect(normalizeAgentRunSummary({
      run_id: "run-overlong-conversation",
      conversation_id: "c".repeat(257),
    })).not.toHaveProperty("conversationId");
    expect(normalizeAgentRunSummary({ run_id: "r".repeat(257) })).toBeNull();

    expect(normalizeAgentRunSummary({ question: "missing id" })).toBeNull();
    expect(normalizeAgentRunSummary({ run_id: "   " })).toBeNull();
  });

  it("accepts only nonnegative safe integer timestamp and duration numbers", () => {
    expect(normalizeAgentRunSummary({
      run_id: "run-safe-numbers",
      started_at_epoch_ms: 10,
      completed_at_epoch_ms: Number.MAX_SAFE_INTEGER,
      duration_ms: 0,
    })).toMatchObject({
      startedAtEpochMs: 10,
      completedAtEpochMs: Number.MAX_SAFE_INTEGER,
      durationMs: 0,
    });

    const invalidNumbers: unknown[] = ["25", "   ", true, [], -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1];
    for (const [index, value] of invalidNumbers.entries()) {
      expect(normalizeAgentRunSummary({
        run_id: `run-invalid-number-${index}`,
        started_at_epoch_ms: value,
        completed_at_epoch_ms: value,
        duration_ms: value,
      })).toMatchObject({
        startedAtEpochMs: 0,
        completedAtEpochMs: undefined,
        durationMs: undefined,
      });
    }
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

  it("constructs bounded allowlisted timeline events", () => {
    const detail = normalizeAgentRunDetail({
      run_id: "run-events",
      events: [
        {
          payload: JSON.stringify({
            run_id: "r".repeat(257),
            type: ` ${"t".repeat(80)} `,
            stage: "s".repeat(80),
            label: "l".repeat(2_100),
            percent: 75,
            action: "a".repeat(80),
            message: "m".repeat(2_100),
            response: { reply: "must be dropped" },
            unknown: "must be dropped",
            payload: {
              id: "tool-1",
              tool: "screen",
              label: "Screen candidates",
              status: "ok",
              output_summary: "Found candidates",
              request: { api_key: "secret" },
              unknown: "must be dropped",
            },
          }),
        },
        { type: "   ", message: "blank type" },
        { type: "status", percent: 101 },
        { type: "status", percent: 1.5 },
      ],
    });

    expect(detail?.events).toEqual([
      {
        type: "t".repeat(64),
        stage: "s".repeat(64),
        label: "l".repeat(2_000),
        percent: 75,
        action: "a".repeat(64),
        message: "m".repeat(2_000),
        payload: {
          id: "tool-1",
          tool: "screen",
          label: "Screen candidates",
          status: "ok",
          output_summary: "Found candidates",
        },
      },
      { type: "status" },
      { type: "status" },
    ]);
  });

  it("constructs a fresh bounded allowlisted Agent result", () => {
    const result = normalizeAgentRunDetail({
      run_id: "run-result",
      events: [],
      result: {
        reply: "r".repeat(8_100),
        action: "a".repeat(100),
        unknown: "must be dropped",
        intent: {
          kind: "k".repeat(100),
          query: "q".repeat(2_100),
          symbols: ["000001.SZ", 123, null, "s".repeat(257)],
          window: null,
          depth: "d".repeat(100),
          mode: "m".repeat(100),
          unknown: "must be dropped",
        },
        tool_calls: [
          null,
          "invalid",
          {
            id: "tool-1",
            tool: "screen",
            label: 42,
            status: "ok",
            output_summary: "o".repeat(2_100),
            warnings: ["check", 42, "w".repeat(2_100)],
            unknown: "must be dropped",
          },
        ],
        evidence_summary: [
          null,
          { title: "t".repeat(300), source: 42, level: "primary", summary: "s".repeat(2_100), unknown: "drop" },
        ],
        answer_sections: [
          null,
          {
            title: "Conclusion",
            bullets: ["b".repeat(2_100), 42],
            provenance: "model_inference",
            evidence_basis: "e".repeat(2_100),
            unknown: "drop",
          },
        ],
        model_answer_sections: ["invalid", { title: 42, bullets: ["Verify"] }],
        warnings: [null, 42, "w".repeat(2_100)],
        next_actions: ["n".repeat(2_100), false],
        harness: {
          prompt_version: "p".repeat(300),
          profile_id: "h".repeat(300),
          model_used: true,
          model: 42,
          unknown: "drop",
        },
      },
    })?.result;

    expect(result).toEqual({
      reply: "r".repeat(8_000),
      action: "a".repeat(64),
      intent: {
        kind: "k".repeat(64),
        query: "q".repeat(2_000),
        symbols: ["000001.SZ"],
        depth: "d".repeat(64),
        mode: "m".repeat(64),
      },
      tool_calls: [{
        id: "tool-1",
        tool: "screen",
        status: "ok",
        output_summary: "o".repeat(2_000),
        warnings: ["check", "w".repeat(2_000)],
      }],
      evidence_summary: [{
        title: "t".repeat(256),
        level: "primary",
        summary: "s".repeat(2_000),
      }],
      answer_sections: [{
        title: "Conclusion",
        bullets: ["b".repeat(2_000)],
        provenance: "model_inference",
        evidence_basis: "e".repeat(2_000),
      }],
      model_answer_sections: [{ bullets: ["Verify"] }],
      warnings: ["w".repeat(2_000)],
      next_actions: ["n".repeat(2_000)],
      harness: {
        prompt_version: "p".repeat(256),
        profile_id: "h".repeat(256),
        model_used: true,
      },
    });
  });

  it("recursively sanitizes known domain payloads while preserving ordinary data", () => {
    const result = normalizeAgentRunDetail({
      run_id: "run-domain-data",
      events: [],
      result: {
        tool_calls: [{
          tool: "screen",
          input: {
            code: "000001.SZ",
            request: { api_key: "secret-marker" },
            nested: { API_KEY: "secret-marker", score: 12.5 },
          },
        }],
        criteria: { kept: "criteria", min_roe: 0.15 },
        backtest: { kept: "backtest", total_return: -0.125 },
        news_rag: { kept: "news_rag" },
        observe: { kept: "observe" },
        sector_screen: { kept: "sector_screen" },
        graph_screen: { kept: "graph_screen" },
        trend_screen: { kept: "trend_screen" },
        data: {
          kept: "data",
          enabled: true,
          missing: null,
          long_text: "d".repeat(8_100),
          rows: Array.from({ length: 105 }, (_, index) => ({ index })),
          sensitive: {
            ordinary: "keep me",
            Request: { value: "secret-marker" },
            API_KEY: "secret-marker",
            requestConfig: { value: "secret-marker" },
            llmConfig: { value: "secret-marker" },
            NETWORK_CONFIG: { value: "secret-marker" },
            runtimeConfig: { value: "secret-marker" },
            providerApiKeyConfig: { value: "secret-marker" },
            authorizationHeader: "secret-marker",
            Headers: { value: "secret-marker" },
            proxyUrl: "secret-marker",
            Credentials: "secret-marker",
            clientSecret: "secret-marker",
            accessToken: "secret-marker",
          },
          deep: { a: { b: { c: { d: { e: { f: { too_deep: "secret-marker" } } } } } } },
        },
        unknown_payload: { value: "must be dropped" },
      },
    })?.result;

    expect(result?.tool_calls?.[0].input).toEqual({
      code: "000001.SZ",
      nested: { score: 12.5 },
    });
    for (const key of [
      "criteria",
      "backtest",
      "news_rag",
      "observe",
      "sector_screen",
      "graph_screen",
      "trend_screen",
      "data",
    ] as const) {
      expect(result?.[key]).toMatchObject({ kept: key });
    }
    expect(result?.criteria).toMatchObject({ min_roe: 0.15 });
    expect(result?.backtest).toMatchObject({ total_return: -0.125 });

    const data = result?.data as Record<string, unknown>;
    expect(data).toMatchObject({ enabled: true, missing: null, long_text: "d".repeat(8_000) });
    expect(data.rows).toHaveLength(100);
    expect(data.sensitive).toEqual({ ordinary: "keep me" });
    expect(JSON.stringify(result)).not.toContain("secret-marker");
    expect(result).not.toHaveProperty("unknown_payload");
  });

  it("requests current-conversation and all-run summaries with stable URLs", async () => {
    getJson.mockResolvedValue({ runs: [] });

    await listAgentRuns({ conversationId: "  conversation/1  " });
    await listAgentRuns({});

    expect(getJson).toHaveBeenNthCalledWith(
      1,
      "/api/agent/runs?limit=50&conversation_id=conversation%2F1",
      { signal: undefined },
    );
    expect(getJson).toHaveBeenNthCalledWith(2, "/api/agent/runs?limit=50", { signal: undefined });
  });

  it("does not widen invalid explicit conversation scopes to all runs", async () => {
    await expect(listAgentRuns({ conversationId: "   " })).resolves.toEqual([]);
    await expect(listAgentRuns({ conversationId: "c".repeat(257) })).resolves.toEqual([]);
    await expect(listAgentRuns({ conversationId: undefined })).resolves.toEqual([]);

    expect(getJson).not.toHaveBeenCalled();
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

    getJson.mockResolvedValueOnce({ runs: [] });
    await expect(listAgentRuns({ limit: -1 })).resolves.toEqual([]);
    expect(getJson).toHaveBeenLastCalledWith("/api/agent/runs?limit=1", { signal: undefined });
  });

  it("uses the default list limit for invalid runtime number values", async () => {
    getJson.mockResolvedValue({ runs: [] });
    const invalidLimits: unknown[] = ["25", "   ", true, [], 1.5, NaN, Infinity];

    for (const limit of invalidLimits) await listAgentRuns({ limit: limit as number });

    expect(getJson).toHaveBeenCalledTimes(invalidLimits.length);
    for (const call of getJson.mock.calls) {
      expect(call).toEqual(["/api/agent/runs?limit=50", { signal: undefined }]);
    }
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

  it("encodes dot-segment ids and rejects invalid lookup identities without requests", async () => {
    getJson.mockResolvedValue({ run: null });

    await getAgentRun(".");
    await getAgentRun("..");
    expect(getJson).toHaveBeenNthCalledWith(1, "/api/agent/runs/%2E", { signal: undefined });
    expect(getJson).toHaveBeenNthCalledWith(2, "/api/agent/runs/%2E%2E", { signal: undefined });

    getJson.mockClear();
    await expect(getAgentRun("   ")).resolves.toBeNull();
    await expect(getAgentRun("r".repeat(257))).resolves.toBeNull();
    expect(getJson).not.toHaveBeenCalled();
  });
});
