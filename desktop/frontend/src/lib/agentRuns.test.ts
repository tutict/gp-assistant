import { beforeEach, describe, expect, it, vi } from "vitest";

const { getJson, postJson } = vi.hoisted(() => ({ getJson: vi.fn(), postJson: vi.fn() }));
vi.mock("./tauri", () => ({ getJson, postJson }));

import {
  deleteAgentConversationRuns,
  getAgentRun,
  getAgentRunMetrics,
  listAgentRuns,
  MAX_AGENT_REPLAY_EVENTS,
  normalizeAgentRunDetail,
  normalizeAgentRunMetrics,
  normalizeAgentRunSummary,
} from "./agentRuns";

describe("agent run ledger client", () => {
  beforeEach(() => {
    getJson.mockReset();
    postJson.mockReset();
  });

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
    expect(normalizeAgentRunSummary({ run_id: "." })).toBeNull();
    expect(normalizeAgentRunSummary({ run_id: ".." })).toBeNull();
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

  });

  it("distinguishes terminal unavailable results from in-progress and failed runs", () => {
    for (const [runId, result] of [
      ["run-string-result", "invalid"],
      ["run-array-result", [{ reply: "invalid" }]],
      ["run-empty-result", {}],
      ["run-normalized-empty-result", { reply: "   ", unknown: "dropped" }],
    ] as const) {
      const detail = normalizeAgentRunDetail({ run_id: runId, status: "completed", events: [], result });
      expect(detail).toMatchObject({ runId, resultUnavailable: true });
      expect(detail?.result).toBeUndefined();
    }

    for (const input of [
      { run_id: "run-completed-null-result", status: "completed", events: [], result: null },
      { run_id: "run-completed-missing-result", status: "completed", events: [] },
    ]) {
      const detail = normalizeAgentRunDetail(input);
      expect(detail).toMatchObject({ resultUnavailable: true });
      expect(detail?.result).toBeUndefined();
    }

    for (const input of [
      { run_id: "run-running-null-result", status: "running", events: [], result: null },
      { run_id: "run-running-missing-result", status: "running", events: [] },
      { run_id: "run-failed-null-result", status: "failed", error: "Model unavailable", events: [], result: null },
      { run_id: "run-failed-missing-result", status: "failed", error: "Model unavailable", events: [] },
    ]) {
      const detail = normalizeAgentRunDetail(input);
      expect(detail?.result).toBeUndefined();
      expect(detail).not.toHaveProperty("resultUnavailable");
    }

    const valid = normalizeAgentRunDetail({
      run_id: "run-valid-result",
      status: "completed",
      events: [],
      result: { reply: "Completed reply" },
    });
    expect(valid?.result).toMatchObject({ reply: "Completed reply" });
    expect(valid).not.toHaveProperty("resultUnavailable");
  });

  it("rejects malformed replay backtests before the specialized renderer receives them", () => {
    const detail = normalizeAgentRunDetail({
      run_id: "run-malformed-backtest",
      status: "completed",
      events: [],
      result: {
        action: "backtest",
        backtest: {
          metrics: { total_return: 0, num_stocks: 1 },
          equity_curve: [],
          symbols: [],
          adaptive_release_gate: { passed: false, checks: {} },
        },
      },
    });

    expect(detail).toMatchObject({
      runId: "run-malformed-backtest",
      resultUnavailable: true,
    });
    expect(detail?.result).toBeUndefined();
  });

  it("retains a complete valid replay backtest", () => {
    const backtest = {
      metrics: { total_return: 0.12, num_stocks: 1 },
      equity_curve: [{ date: "2026-08-08", equity: 1.12 }],
      symbols: ["002432.SZ"],
      adaptive_release_gate: {
        passed: true,
        checks: [{
          key: "cached_run_millis",
          passed: true,
          actual: 720,
          requirement: "same-day cached run <= 2000 ms",
        }],
      },
    };
    const detail = normalizeAgentRunDetail({
      run_id: "run-valid-backtest",
      status: "completed",
      events: [],
      result: { action: "backtest", backtest },
    });

    expect(detail?.result?.backtest).toEqual(backtest);
    expect(detail).not.toHaveProperty("resultUnavailable");
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

  it("preserves direct ledger event types and allowlisted payloads", () => {
    const detail = normalizeAgentRunDetail({
      run_id: "run-direct-events",
      events: [
        {
          type: "tool_start",
          stage: "tools",
          payload: {
            id: "tool-1",
            tool: "screen",
            label: "Screen candidates",
            request: { api_key: "drop" },
          },
        },
        {
          type: "tool_result",
          payload: {
            id: "tool-1",
            status: "ok",
            output_summary: "Found candidates",
            unknown: "drop",
          },
        },
      ],
    });

    expect(detail?.events).toEqual([
      {
        type: "tool_start",
        stage: "tools",
        payload: { id: "tool-1", tool: "screen", label: "Screen candidates" },
      },
      {
        type: "tool_result",
        payload: { id: "tool-1", status: "ok", output_summary: "Found candidates" },
      },
    ]);
  });

  it("caps raw replay event traversal before normalization", () => {
    const capped = normalizeAgentRunDetail({
      run_id: "run-event-cap",
      events: [
        ...Array.from({ length: MAX_AGENT_REPLAY_EVENTS }, (_, index) => ({
          type: "status",
          label: `Event ${index}`,
        })),
        { type: "error", message: "Beyond cap" },
      ],
    });
    expect(capped?.events).toHaveLength(MAX_AGENT_REPLAY_EVENTS);
    expect(capped?.events.at(-1)).toMatchObject({ label: `Event ${MAX_AGENT_REPLAY_EVENTS - 1}` });

    const invalidBeforeCap = normalizeAgentRunDetail({
      run_id: "run-event-traversal",
      events: [
        ...Array.from({ length: MAX_AGENT_REPLAY_EVENTS }, () => ({ type: "   " })),
        { type: "status", label: "Must not be traversed" },
      ],
    });
    expect(invalidBeforeCap?.events).toEqual([]);
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
          long_text: "d".repeat(8_000),
          rows: Array.from({ length: 105 }, (_, index) => ({ index })),
          sensitive: {
            ordinary: "keep me",
            candidate_requested: true,
            benchmark_requested: "CSI 300",
            breadth_requested: 42,
            configuration_status: "ready",
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
            config: { password: "secret-marker" },
            password: "secret-marker",
            refreshToken: "secret-marker",
            bearer_token: "secret-marker",
            cookie: "secret-marker",
            cookies: ["secret-marker"],
            sessionToken: "secret-marker",
            private_key: "secret-marker",
            clientKey: "secret-marker",
          },
          deep: { a: { b: "keep me" } },
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
    expect(data.rows).toHaveLength(105);
    expect(data.sensitive).toEqual({
      ordinary: "keep me",
      candidate_requested: true,
      benchmark_requested: "CSI 300",
      breadth_requested: 42,
      configuration_status: "ready",
    });
    expect(JSON.stringify(result)).not.toContain("secret-marker");
    expect(result).not.toHaveProperty("unknown_payload");
  });

  it("preserves ordinary backtest series instead of silently truncating replay data", () => {
    const equityCurve = Array.from({ length: 252 }, (_, index) => ({
      date: `2026-${String(Math.floor(index / 28) + 1).padStart(2, "0")}-${String((index % 28) + 1).padStart(2, "0")}`,
      equity: 1 + index / 1_000,
    }));
    const detail = normalizeAgentRunDetail({
      run_id: "run-full-backtest",
      status: "completed",
      events: [],
      result: {
        action: "backtest",
        backtest: {
          metrics: { total_return: 0.25, num_stocks: 1 },
          equity_curve: equityCurve,
          symbols: ["000001.SZ"],
        },
      },
    });

    expect((detail?.result?.backtest as Record<string, unknown>).equity_curve).toEqual(equityCurve);
    expect(detail?.resultUnavailable).toBeUndefined();
  });

  it("marks oversized domain collections unavailable instead of rendering partial data", () => {
    const detail = normalizeAgentRunDetail({
      run_id: "run-oversized-domain",
      status: "completed",
      events: [],
      result: {
        reply: "Do not show this as a complete result",
        data: {
          rows: Array.from({ length: 10_001 }, (_, index) => ({ index })),
        },
      },
    });

    expect(detail?.result).toBeUndefined();
    expect(detail?.resultUnavailable).toBe(true);
  });

  it("marks depth, object-key, and string truncation unavailable", () => {
    const tooDeep = { level: { level: { level: { level: { level: { level: { value: "hidden" } } } } } } };
    const tooWide = Object.fromEntries(Array.from({ length: 101 }, (_, index) => [`field_${index}`, index]));
    for (const [runId, data] of [
      ["run-too-deep", tooDeep],
      ["run-too-wide", tooWide],
      ["run-long-domain-text", { analysis: "x".repeat(8_001) }],
    ] as const) {
      const detail = normalizeAgentRunDetail({
        run_id: runId,
        status: "completed",
        events: [],
        result: { reply: "must not look complete", data },
      });
      expect(detail).toMatchObject({ resultUnavailable: true });
      expect(detail?.result).toBeUndefined();
    }
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

  it("normalizes aggregate run metrics and keeps the request boundary typed", async () => {
    const input = {
      schema_version: 1,
      sample_size: 2,
      sample_limit: 200,
      conversation_id: "must not be surfaced",
      status_counts: { completed: 2, failed: 0 },
      profile_counts: {
        hot_money_early_v1: { count: 2, completed: 2, failed: 0, model_used: 1, fallback: 1 },
        "   ": { count: 99, completed: 99, failed: 0, model_used: 0, fallback: 0 },
      },
      model_outcome_counts: { model_success: 1, not_configured: 1 },
      api_format_counts: { openai_chat: 1, none: 1 },
      duration_ms: { count: 2, average_ms: 120, p50_ms: 100, p95_ms: 200, max_ms: 200 },
      question: "must not be surfaced",
    };
    expect(normalizeAgentRunMetrics(input)).toEqual({
      schemaVersion: 1,
      sampleSize: 2,
      sampleLimit: 200,
      statusCounts: { completed: 2, failed: 0 },
      profileCounts: {
        hot_money_early_v1: { count: 2, completed: 2, failed: 0, modelUsed: 1, fallback: 1 },
      },
      modelOutcomeCounts: { model_success: 1, not_configured: 1 },
      apiFormatCounts: { openai_chat: 1, none: 1 },
      durationMs: { count: 2, averageMs: 120, p50Ms: 100, p95Ms: 200, maxMs: 200 },
    });

    getJson.mockResolvedValue(input);
    await expect(getAgentRunMetrics({ conversationId: " conversation-1 ", limit: 999 })).resolves.toMatchObject({
      sampleSize: 2,
    });
    expect(getJson).toHaveBeenCalledWith(
      "/api/agent/metrics?limit=999&conversation_id=conversation-1",
      { signal: undefined },
    );
  });

  it("deletes conversation runs through the typed ledger boundary", async () => {
    postJson.mockResolvedValue({ deleted: 2 });

    await expect(deleteAgentConversationRuns("  conversation/1  ")).resolves.toBe(2);
    expect(postJson).toHaveBeenCalledWith("/api/agent/runs/delete-conversation", {
      conversation_id: "conversation/1",
    }, { timeoutMs: 10_000 });
    await expect(deleteAgentConversationRuns("   ")).rejects.toThrow("conversation_id is required");
    await expect(deleteAgentConversationRuns("中".repeat(100))).rejects.toThrow("conversation_id is required");
  });

  it("does not widen invalid explicit conversation scopes to all runs", async () => {
    await expect(listAgentRuns({ conversationId: "   " })).resolves.toEqual([]);
    await expect(listAgentRuns({ conversationId: "c".repeat(257) })).resolves.toEqual([]);
    await expect(listAgentRuns({ conversationId: "中".repeat(100) })).resolves.toEqual([]);
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

  it("caps raw list traversal at the normalized requested limit", async () => {
    getJson.mockResolvedValue({
      runs: [
        { run_id: "   " },
        { question: "missing id" },
        { run_id: "run-beyond-limit", status: "completed" },
      ],
    });

    await expect(listAgentRuns({ limit: 2 })).resolves.toEqual([]);
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

  it("rejects a detail response whose run id does not match the requested identity", async () => {
    getJson.mockResolvedValueOnce({
      run: {
        run_id: "run-other",
        status: "completed",
        events: [],
        result: { reply: "Wrong run" },
      },
    });

    await expect(getAgentRun("  run-requested  ")).resolves.toBeNull();
    expect(getJson).toHaveBeenLastCalledWith("/api/agent/runs/run-requested", { signal: undefined });
  });

  it("rejects dot-only and invalid lookup identities without requests", async () => {
    await expect(getAgentRun(".")).resolves.toBeNull();
    await expect(getAgentRun("..")).resolves.toBeNull();
    await expect(getAgentRun("   ")).resolves.toBeNull();
    await expect(getAgentRun("r".repeat(257))).resolves.toBeNull();
    expect(getJson).not.toHaveBeenCalled();
  });
});
