import { describe, expect, it, vi } from "vitest";
import {
  actionResultKind,
  buildBacktestRequest,
  buildAdaptiveScreenRequest,
  buildCustomScreenRequest,
  buildNewsRagRequest,
  buildScreenCriteria,
  buildSectorScreenRequest,
  buildTrendScreenRequest,
  isAdaptiveProgressForRun,
  fetchUpstreamImportPayload,
  buildLlmConfig,
  normalizeAgentResult,
  normalizeAgentStreamEvent,
  normalizeLlmSettings,
  normalizeNewsGroups,
  requireBacktestResult,
  normalizeScreenGroups,
  normalizeScreenRows,
  parseSseBlock,
  parseUpstreamImportDescriptor,
  sanitizePersistedLlmSettings,
} from "./contracts";
import type { FilterCriteria } from "../components/FilterBar";
import type { LlmSettings } from "../types";

const criteria: FilterCriteria = {
  includeSt: false,
  requireInstitutionBuyRatio: true,
  minRoe: "15",
  maxPe: "30",
  maxPb: "5",
  minMcap: "50",
  industry: "影视院线",
  marketScope: "沪市A股",
  resultLimit: 10,
  sortBy: "score",
  sortDir: "desc",
  scoreProfile: "balanced",
};

const fullUniverseCriteria: FilterCriteria = {
  ...criteria,
  requireInstitutionBuyRatio: false,
  minRoe: "",
  maxPe: "",
  maxPb: "",
  minMcap: "",
  industry: "",
  marketScope: "",
};

describe("LLM settings persistence", () => {
  it("drops API keys unless remember_key is enabled", () => {
    const sanitized = sanitizePersistedLlmSettings({ api_key: "sk-test", model: "gpt", remember_key: false } as LlmSettings);
    expect(sanitized).toMatchObject({
      active_provider_id: "legacy",
      providers: [{
        id: "legacy",
        name: "gpt",
        provider: "custom",
        model: "gpt",
        api_format: "openai_chat",
        endpoint_mode: "base_url",
        remember_key: false,
      }],
    });
    expect(sanitized?.providers?.[0]).not.toHaveProperty("api_key");
    expect(sanitizePersistedLlmSettings({
      active_provider_id: "a",
      providers: [
        { id: "a", api_key: "sk-drop", model: "gpt", remember_key: false },
        { id: "b", api_key: "sk-keep", model: "deepseek", remember_key: true },
      ],
    })).toMatchObject({
      active_provider_id: "a",
      providers: [
        { id: "a", model: "gpt", provider: "custom", remember_key: false },
        { id: "b", api_key: "sk-keep", model: "deepseek", provider: "custom", remember_key: true },
      ],
    });
  });

  it("starts from a generic compatible provider instead of a fixed vendor", () => {
    expect(normalizeLlmSettings(null)).toEqual({
      active_provider_id: "compatible",
      providers: [{
        id: "compatible",
        name: "通用兼容",
        provider: "openai-compatible",
        base_url: "",
        model: "",
        api_format: "openai_chat",
        endpoint_mode: "base_url",
        custom_user_agent: "",
        temperature: 0.7,
        timeout: 60,
        json_mode: false,
        remember_key: false,
      }],
    });
    expect(buildLlmConfig(null)).toBeUndefined();
  });

  it("normalizes legacy settings and builds config from the active provider", () => {
    expect(normalizeLlmSettings({ base_url: "https://example.test/v1/", model: "old-model" } as LlmSettings)).toMatchObject({
      active_provider_id: "legacy",
      providers: [{ id: "legacy", model: "old-model" }],
    });
    expect(buildLlmConfig({
      active_provider_id: "deepseek",
      providers: [
        { id: "openai", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
        { id: "deepseek", base_url: "https://api.deepseek.com/v1/", model: "deepseek-chat", api_key: "sk-test", timeout: 45 },
      ],
    })).toEqual({
      api_key: "sk-test",
      base_url: "https://api.deepseek.com/v1",
      model: "deepseek-chat",
      api_format: "openai_chat",
      endpoint_mode: "base_url",
      timeout_seconds: 45,
    });
  });

  it("keeps a model configuration when the provider uses the default endpoint", () => {
    expect(buildLlmConfig({
      active_provider_id: "missing-connection",
      providers: [{ id: "missing-connection", model: "gpt-4o-mini" }],
    })).toBeUndefined();

    expect(buildLlmConfig({
      active_provider_id: "default-endpoint",
      providers: [{ id: "default-endpoint", model: "gpt-4o-mini", api_key: "sk-test" }],
    })).toEqual({
      api_key: "sk-test",
      model: "gpt-4o-mini",
      api_format: "openai_chat",
      endpoint_mode: "base_url",
    });
  });
  it("keeps non-OpenAI provider metadata while building a compatible client config", () => {
    const settings: LlmSettings = {
      active_provider_id: "glm",
      providers: [
        {
          id: "glm",
          name: "GLM / 智谱",
          provider: "zhipu",
          base_url: "https://open.bigmodel.cn/api/paas/v4/",
          model: "glm-4-flash",
          api_key: "zhipu-key",
          remember_key: true,
        },
      ],
    };

    expect(sanitizePersistedLlmSettings(settings)).toMatchObject({
      active_provider_id: "glm",
      providers: [{ id: "glm", provider: "zhipu", api_key: "zhipu-key" }],
    });
    expect(buildLlmConfig(settings)).toEqual({
      api_key: "zhipu-key",
      base_url: "https://open.bigmodel.cn/api/paas/v4",
      model: "glm-4-flash",
      api_format: "openai_chat",
      endpoint_mode: "base_url",
    });
  });

  it("preserves protocol, full endpoint mode, and a sanitized custom User-Agent", () => {
    const settings: LlmSettings = {
      active_provider_id: "responses",
      providers: [{
        id: "responses",
        base_url: "https://gateway.example/custom/generate",
        model: "gpt-compatible",
        api_format: "openai_responses",
        endpoint_mode: "full_url",
        custom_user_agent: "  company-client/1.0  ",
      }],
    };
    expect(normalizeLlmSettings(settings).providers?.[0]).toMatchObject({
      api_format: "openai_responses",
      endpoint_mode: "full_url",
      custom_user_agent: "company-client/1.0",
    });
    expect(buildLlmConfig(settings)).toEqual({
      base_url: "https://gateway.example/custom/generate",
      model: "gpt-compatible",
      api_format: "openai_responses",
      endpoint_mode: "full_url",
      custom_user_agent: "company-client/1.0",
    });
  });
});
describe("contract payload builders", () => {
  it("builds the adaptive swing request as a nested deterministic contract", () => {
    expect(buildAdaptiveScreenRequest(criteria, "defensive", "run-fixed")).toMatchObject({
      criteria: { industry: "影视院线", market_scope: "沪市A股", limit: 80, min_roe: 0.15 },
      mode: "defensive",
      horizon: "swing_10_30d",
      primary_limit: 10,
      exploration_limit: 10,
      run_id: "run-fixed",
    });
  });

  it("keeps an unfiltered adaptive request eligible for full-universe release evidence", () => {
    const request = buildAdaptiveScreenRequest(fullUniverseCriteria, "auto", "run-release");

    expect(request.criteria).not.toHaveProperty("min_deducted_net_profit_billion");
    expect(request.criteria).not.toHaveProperty("min_deducted_net_profit_growth_rate");
    expect(request.criteria).not.toHaveProperty("industry");
    expect(request.criteria).not.toHaveProperty("min_roe");
  });

  it("builds screen criteria in backend schema shape", () => {
    expect(buildScreenCriteria(criteria)).toMatchObject({
      include_st: false,
      require_institution_buy_ratio_gt_sell_ratio: true,
      min_roe: 0.15,
      max_pe: 30,
      max_pb: 5,
      min_market_cap_billion: 50,
      limit: 10,
      sort_by: "score",
      sort_dir: "desc",
      score_profile: "balanced",
    });
  });

  it("migrates legacy Anthropic providers only when their protocol is missing", () => {
    expect(normalizeLlmSettings({
      active_provider_id: "legacy-anthropic",
      providers: [{
        id: "legacy-anthropic",
        provider: "anthropic-compatible",
        base_url: "https://gateway.example/anthropic",
        model: "claude-compatible",
      }],
    }).providers?.[0]).toMatchObject({
      provider: "anthropic-compatible",
      api_format: "anthropic_messages",
    });

    expect(normalizeLlmSettings({
      active_provider_id: "explicit-openai",
      providers: [{
        id: "explicit-openai",
        provider: "anthropic-compatible",
        base_url: "https://gateway.example/v1",
        model: "openai-compatible",
        api_format: "openai_chat",
      }],
    }).providers?.[0]).toMatchObject({
      provider: "anthropic-compatible",
      api_format: "openai_chat",
    });
  });

  it("preserves real industry labels and keeps market scopes separate", () => {
    expect(buildScreenCriteria({ ...criteria, industry: "传媒" })).toMatchObject({
      industry: "传媒",
      market_scope: "沪市A股",
    });
    expect(buildScreenCriteria({ ...criteria, industry: "", marketScope: "科创板" })).toMatchObject({
      market_scope: "科创板",
    });
  });

  it("keeps a real industry and an independent market scope", () => {
    expect(buildScreenCriteria({
      ...criteria,
      industry: "影视院线",
      marketScope: "北交所",
    })).toMatchObject({
      industry: "影视院线",
      market_scope: "北交所",
    });
  });

  it("wraps sector and board requests instead of sending raw criteria", () => {
    expect(buildSectorScreenRequest(criteria, "concept", 5, 12)).toMatchObject({
      group_by: "concept",
      per_sector_limit: 5,
      max_sectors: 12,
      min_sector_candidates: 5,
      criteria: { industry: "影视院线", market_scope: "沪市A股" },
    });
    expect(buildSectorScreenRequest(criteria, "concept", 10, 12)).toMatchObject({
      group_by: "concept",
      per_sector_limit: 10,
      max_sectors: 12,
      min_sector_candidates: 5,
    });
    expect(buildSectorScreenRequest(criteria, "board", 3, 5)).toMatchObject({
      group_by: "board",
      per_sector_limit: 3,
      max_sectors: 5,
      min_sector_candidates: 1,
    });
  });

  it("applies deducted-profit defaults only to custom screening", () => {
    const custom = buildCustomScreenRequest(fullUniverseCriteria).criteria as Record<string, unknown>;
    const concept = buildSectorScreenRequest(fullUniverseCriteria, "concept", 10, 12).criteria;
    const board = buildSectorScreenRequest(fullUniverseCriteria, "board", 5, 5).criteria;
    const trend = buildTrendScreenRequest(fullUniverseCriteria, "2026-01-01", "2026-06-01").criteria as Record<string, unknown>;

    expect(custom).toMatchObject({
      min_deducted_net_profit_billion: 0,
      min_deducted_net_profit_growth_rate: 10,
    });
    for (const criteriaPayload of [concept, board, trend]) {
      expect(criteriaPayload).not.toHaveProperty("min_deducted_net_profit_billion");
      expect(criteriaPayload).not.toHaveProperty("min_deducted_net_profit_growth_rate");
    }
  });

  it("builds graph, trend, backtest, and news payloads with backend field names", () => {
    expect(buildCustomScreenRequest(criteria)).toMatchObject({
      seed_codes: [],
      seed_query: "",
      relation_depth: 1,
      relation_weight: 0,
      criteria: { limit: 100 },
    });
    expect(buildTrendScreenRequest(criteria, "2020-01-01", "2026-06-27")).toMatchObject({
      start_date: "20200101",
      end_date: "20260627",
      criteria: { limit: 100 },
    });
    expect(buildTrendScreenRequest(criteria, "2026-06-25", "2026-07-09")).toMatchObject({
      start_date: "20260305",
      end_date: "20260709",
      criteria: { limit: 100 },
    });
    expect(buildBacktestRequest({
      source: "watchlist",
      criteria,
      watchlist: [{ code: "300750.SZ" }],
      startDate: "2020-01-01",
      endDate: "2026-06-27",
      topN: 5,
      rebalanceFrequency: "quarterly",
      transactionCostBps: 20,
      benchmark: "none",
      strategyMode: "walk_forward",
    })).toMatchObject({
      source: "watchlist",
      stock_codes: ["300750.SZ"],
      rebalance_frequency: "quarterly",
      transaction_cost_bps: 20,
      benchmark: "none",
      strategy_mode: "walk_forward",
    });
    expect(buildBacktestRequest({
      source: "criteria",
      criteria,
      watchlist: [],
      startDate: "2020-01-01",
      endDate: "2026-06-27",
      topN: 10,
      rebalanceFrequency: "monthly",
      transactionCostBps: 10,
      benchmark: "candidate_equal_weight",
    })).toMatchObject({ strategy_mode: "candidate_snapshot" });
    expect(buildBacktestRequest({
      source: "criteria",
      criteria,
      watchlist: [],
      startDate: "2020-01-01",
      endDate: "2026-06-27",
      topN: 10,
      rebalanceFrequency: "monthly",
      transactionCostBps: 10,
      benchmark: "candidate_equal_weight",
      adaptiveScreenSpec: buildAdaptiveScreenRequest(criteria, "range", "screen-run"),
    })).toMatchObject({
      strategy_mode: "adaptive_swing_v1:range",
      adaptive_screen_spec: {
        mode: "range",
        horizon: "swing_10_30d",
        primary_limit: 10,
        exploration_limit: 10,
      },
    });
    expect(buildNewsRagRequest("300750.SZ", 30)).toMatchObject({
      code: "300750.SZ",
      seed_codes: ["300750.SZ"],
      days: 30,
      max_items: 24,
    });
  });
});

describe("adaptive progress isolation", () => {
  it("accepts only events for the active run id", () => {
    expect(isAdaptiveProgressForRun({ run_id: "run-current", percent: 40 }, "run-current")).toBe(true);
    expect(isAdaptiveProgressForRun({ run_id: "run-old", percent: 100 }, "run-current")).toBe(false);
    expect(isAdaptiveProgressForRun({ run_id: "run-current" }, null)).toBe(false);
    expect(isAdaptiveProgressForRun(null, "run-current")).toBe(false);
  });
});

describe("response normalizers", () => {
  it("rejects empty backtest responses instead of silently returning to the empty state", () => {
    expect(() => requireBacktestResult(undefined)).toThrow("回测接口未返回有效结果");
    expect(() => requireBacktestResult({ metrics: [], equity_curve: [], symbols: [] })).toThrow("回测接口未返回有效结果");
    expect(() => requireBacktestResult({ metrics: {}, equity_curve: [], symbols: [] })).toThrow("回测接口未返回有效结果");
    expect(requireBacktestResult({
      metrics: { total_return: 0.12, num_stocks: 1 },
      equity_curve: [{ date: "2026-07-17", equity: 1.12 }],
      symbols: ["002432.SZ"],
      volatility_snapshots: [{ symbol: "002432.SZ", name: "九安医疗", date: "2026-07-17" }],
    })).toMatchObject({ symbols: ["002432.SZ"] });
  });

  it("rejects malformed optional backtest structures before rendering", () => {
    const valid = {
      metrics: { total_return: 0.12, num_stocks: 1 },
      equity_curve: [{ date: "2026-07-17", equity: 1.12 }],
      symbols: ["002432.SZ"],
    };

    expect(() => requireBacktestResult({ ...valid, notes: [null] })).toThrow("回测接口未返回有效结果");
    expect(() => requireBacktestResult({
      ...valid,
      walk_forward_folds: [{ selection_date: "2026-07-01", selected_symbols: null }],
    })).toThrow("回测接口未返回有效结果");
    expect(() => requireBacktestResult({
      ...valid,
      volatility_snapshots: [{ symbol: "002432.SZ", date: "2026-07-17", atr: { value: "bad" } }],
    })).toThrow("回测接口未返回有效结果");
    expect(() => requireBacktestResult({
      ...valid,
      volatility_snapshots: [{ symbol: "002432.SZ", name: 2432, date: "2026-07-17" }],
    })).toThrow("回测接口未返回有效结果");
  });

  it("rejects malformed adaptive release extensions before rendering", () => {
    const valid = {
      metrics: { total_return: 0.12, num_stocks: 1 },
      equity_curve: [{ date: "2026-07-17", equity: 1.12 }],
      symbols: ["002432.SZ"],
    };

    expect(() => requireBacktestResult({
      ...valid,
      adaptive_release_gate: { passed: false, checks: {} },
    })).toThrow("回测接口未返回有效结果");
    expect(() => requireBacktestResult({
      ...valid,
      legacy_balanced_backtest: {},
    })).toThrow("回测接口未返回有效结果");

    expect(requireBacktestResult({
      ...valid,
      legacy_balanced_backtest: {
        ...valid,
        metrics: { ...valid.metrics, annualized_return: 0.1 },
      },
      adaptive_release_gate: {
        passed: false,
        checks: [{
          key: "cached_run_millis",
          passed: false,
          actual: null,
          requirement: "same-day cached run <= 2000 ms",
        }],
      },
    })).toMatchObject({
      adaptive_release_gate: {
        passed: false,
        checks: [{ key: "cached_run_millis" }],
      },
    });
  });

  it("normalizes nested screen, graph, and trend stock rows", () => {
    const rows = normalizeScreenRows({
      items: [
        { stock: { code: "300750.SZ", name: "CATL", pe: 20 }, score: 18, reasons: ["quality"] },
        { stock: { code: "600519.SH", name: "Moutai" }, final_score: 88, relation_score: 10, reasons: ["relation"] },
      ],
    });
    expect(rows.map((row) => row.code)).toEqual(["300750.SZ", "600519.SH"]);
    expect(rows[0].score).toBe(18);
    expect(rows[1].score).toBe(88);
  });

  it("normalizes adaptive primary and exploration groups", () => {
    const groups = normalizeScreenGroups({
      groups: [
        {
          key: "primary",
          title: "主榜",
          description: "自适应评分候选。",
          total: 2,
          returned: 1,
          items: [{ stock: { code: "300750.SZ", name: "宁德时代" }, score: 90, reasons: ["新能源"] }],
        },
        {
          key: "exploration",
          title: "探索榜",
          total: 3,
          returned: 1,
          items: [{ stock: { code: "600519.SH", name: "贵州茅台" }, score: 80, reasons: ["质量"] }],
        },
      ],
    });

    expect(groups.map((group) => group.title)).toEqual(["主榜", "探索榜"]);
    expect(groups[0]).toMatchObject({ key: "primary", meta: "返回 1 / 总数 2" });
    expect(groups[0].rows[0]).toMatchObject({ code: "300750.SZ", name: "宁德时代", score: 90 });
    expect(groups[1].rows[0]).toMatchObject({ code: "600519.SH", name: "贵州茅台" });
  });

  it("keeps empty adaptive groups so data-shortage states remain visible", () => {
    const groups = normalizeScreenGroups({
      groups: [
        { key: "primary", title: "主榜", total: 0, returned: 0, items: [] },
        { key: "exploration", title: "探索榜", total: 0, returned: 0, items: [] },
      ],
    });

    expect(groups.map((group) => group.title)).toEqual(["主榜", "探索榜"]);
    expect(groups.every((group) => group.rows.length === 0)).toBe(true);
  });

  it("normalizes news sentiment object groups", () => {
    const groups = normalizeNewsGroups({ positive: [{ title: "good", source: "x" }], negative: [], mixed: [], uncertain: [] });
    expect(groups.mode).toBe("plain_news");
    expect(groups.positive).toHaveLength(1);
  });
});

describe("agent and upstream utilities", () => {
  it("normalizes structured agent responses while preserving legacy actions", () => {
    const normalized = normalizeAgentResult({
      action: "screen",
      reply: "ok",
      intent: { kind: "stock_screen", mode: "expert" },
      tool_calls: [{ id: "t1", tool: "stock_screen", status: "ok" }],
      evidence_summary: [{ title: "Local", level: "primary", summary: "Returned rows" }],
        answer_sections: [{ title: "Conclusion", bullets: ["ok"] }],
        model_answer_sections: [{ title: "Inference", bullets: ["verify"] }],
      warnings: ["For stock research only"],
      next_actions: ["Run trend analysis"],
    });
    expect(actionResultKind(normalized)).toBe("screen");
    expect(normalized.intent?.kind).toBe("stock_screen");
    expect(normalized.tool_calls).toHaveLength(1);
    expect(normalized.evidence_summary).toHaveLength(1);
      expect(normalized.answer_sections).toHaveLength(1);
      expect(normalized.model_answer_sections).toHaveLength(1);

    const legacy = normalizeAgentResult({ action: "trend_screen", data: { items: [] } });
    expect(actionResultKind(legacy)).toBe("trend");
    expect(legacy.tool_calls).toEqual([]);
    expect(legacy.warnings).toEqual([]);
  });

  it("parses SSE blocks and Tauri event payloads", () => {
    expect(parseSseBlock('event: status\ndata: {"stage":"run","label":"Running"}')?.stage).toBe("run");
    expect(normalizeAgentStreamEvent({ payload: '{"type":"result","response":{"reply":"ok"}}' })?.type).toBe("result");
  });

  it("parses upstream import descriptors", () => {
    expect(parseUpstreamImportDescriptor("https://host/manifest.json")).toMatchObject({
      manifest_url: "https://host/manifest.json",
      pack_url: "https://host/rag_pack.sqlite",
    });
    expect(parseUpstreamImportDescriptor('{"manifest":{"valid":true},"pack_base64":"abc"}')).toMatchObject({
      manifest: { valid: true },
      pack_base64: "abc",
    });
    expect(parseUpstreamImportDescriptor('{"manifestUrl":"https://host/m.json","packUrl":"https://host/p.sqlite","packBase64":"abc"}')).toMatchObject({
      manifest_url: "https://host/m.json",
      pack_url: "https://host/p.sqlite",
      pack_base64: "abc",
    });
  });

  it("uses manifest.files.pack when downloading upstream packs", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      calls.push(String(url));
      if (calls.length === 1) {
        return new Response(JSON.stringify({ files: { pack: "custom.sqlite" } }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response(new Uint8Array([1, 2]), { status: 200 });
    }));

    const payload = await fetchUpstreamImportPayload({ manifest_url: "https://host/path/manifest.json" });
    expect(calls).toEqual(["https://host/path/manifest.json", "https://host/path/custom.sqlite"]);
    expect(payload.pack_base64).toBe("AQI=");

    vi.unstubAllGlobals();
  });

  it("rejects unsafe upstream import URLs before fetching private hosts", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(fetchUpstreamImportPayload({ manifest_url: "http://host/manifest.json" })).rejects.toThrow(/HTTPS/);
    await expect(fetchUpstreamImportPayload({ manifest_url: "https://192.168.1.10/manifest.json" })).rejects.toThrow(/local or private/);
    expect(fetchMock).not.toHaveBeenCalled();

    vi.unstubAllGlobals();
  });

  it("keeps upstream pack downloads on the manifest origin", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      calls.push(String(url));
      return new Response(JSON.stringify({ files: { pack: "https://cdn.example/pack.sqlite" } }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }));

    await expect(fetchUpstreamImportPayload({ manifest_url: "https://host/path/manifest.json" })).rejects.toThrow(/same origin/);
    expect(calls).toEqual(["https://host/path/manifest.json"]);

    vi.unstubAllGlobals();
  });

  it("rejects upstream packs that declare an excessive content length", async () => {
    const oversizedPackBytes = 25 * 1024 * 1024 + 1;
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      if (String(url).endsWith("manifest.json")) {
        return new Response(JSON.stringify({ files: { pack: "custom.sqlite" } }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response(null, {
        status: 200,
        headers: { "Content-Length": String(oversizedPackBytes) },
      });
    }));

    await expect(fetchUpstreamImportPayload({ manifest_url: "https://host/path/manifest.json" })).rejects.toThrow(/import limit/);

    vi.unstubAllGlobals();
  });
});
