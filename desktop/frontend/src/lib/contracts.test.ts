import { describe, expect, it, vi } from "vitest";
import {
  buildBacktestRequest,
  buildGraphScreenRequest,
  buildNewsRagRequest,
  buildScreenCriteria,
  buildSectorScreenRequest,
  buildTrendScreenRequest,
  fetchUpstreamImportPayload,
  buildLlmConfig,
  normalizeAgentStreamEvent,
  normalizeLlmSettings,
  normalizeNewsGroups,
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
  industry: "bank",
  resultLimit: 10,
  sortBy: "score",
  sortDir: "desc",
};

describe("LLM settings persistence", () => {
  it("drops API keys unless remember_key is enabled", () => {
    expect(sanitizePersistedLlmSettings({ api_key: "sk-test", model: "gpt", remember_key: false } as LlmSettings)).toEqual({
      active_provider_id: "legacy",
      providers: [{ id: "legacy", name: "gpt", provider: "custom", model: "gpt", remember_key: false }],
    });
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
        temperature: 0.7,
        timeout: 60,
        json_mode: false,
        remember_key: false,
      }],
    });
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
      timeout_seconds: 45,
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
    });
  });
});
describe("contract payload builders", () => {
  it("builds screen criteria in backend schema shape", () => {
    expect(buildScreenCriteria(criteria)).toMatchObject({
      include_st: false,
      require_institution_buy_ratio_gt_sell_ratio: true,
      min_roe: 15,
      max_pe: 30,
      max_pb: 5,
      min_market_cap_billion: 50,
      min_deducted_net_profit_billion: 0,
      min_deducted_net_profit_growth_rate: 10,
      limit: 10,
      sort_by: "score",
      sort_dir: "desc",
      score_profile: "rotation",
    });
  });

  it("wraps sector and board requests instead of sending raw criteria", () => {
    expect(buildSectorScreenRequest(criteria, "concept", 5, 12)).toMatchObject({
      group_by: "concept",
      per_sector_limit: 5,
      max_sectors: 12,
      min_sector_candidates: 5,
      criteria: { industry: "bank" },
    });
    expect(buildSectorScreenRequest(criteria, "board", 3, 5)).toMatchObject({
      group_by: "board",
      per_sector_limit: 3,
      max_sectors: 5,
      min_sector_candidates: 1,
    });
  });

  it("builds graph, trend, backtest, and news payloads with backend field names", () => {
    expect(buildGraphScreenRequest(criteria, "300750, 600519.SH", 2, 0.5)).toMatchObject({
      seed_codes: ["300750.SZ", "600519.SH"],
      relation_depth: 2,
      relation_weight: 0.5,
      criteria: { limit: 100 },
    });
    expect(buildTrendScreenRequest(criteria, "2020-01-01", "2026-06-27")).toMatchObject({
      start_date: "20200101",
      end_date: "20260627",
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
    })).toMatchObject({
      source: "watchlist",
      stock_codes: ["300750.SZ"],
      rebalance_frequency: "quarterly",
      transaction_cost_bps: 20,
      benchmark: "none",
    });
    expect(buildNewsRagRequest("300750.SZ", 30)).toMatchObject({
      code: "300750.SZ",
      seed_codes: ["300750.SZ"],
      days: 30,
      max_items: 24,
    });
  });
});

describe("response normalizers", () => {
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

  it("normalizes ordinary screen hot and potential groups", () => {
    const groups = normalizeScreenGroups({
      groups: [
        {
          key: "hot",
          title: "热门股",
          description: "热门方向候选。",
          total: 2,
          returned: 1,
          items: [{ stock: { code: "300750.SZ", name: "宁德时代" }, score: 90, reasons: ["新能源"] }],
        },
        {
          key: "potential",
          title: "潜力股",
          total: 3,
          returned: 1,
          items: [{ stock: { code: "600519.SH", name: "贵州茅台" }, score: 80, reasons: ["质量"] }],
        },
      ],
    });

    expect(groups.map((group) => group.title)).toEqual(["热门股", "潜力股"]);
    expect(groups[0]).toMatchObject({ key: "hot", meta: "返回 1 / 总数 2" });
    expect(groups[0].rows[0]).toMatchObject({ code: "300750.SZ", name: "宁德时代", score: 90 });
    expect(groups[1].rows[0]).toMatchObject({ code: "600519.SH", name: "贵州茅台" });
  });

  it("keeps empty ordinary screen groups so users can see hot and potential sections", () => {
    const groups = normalizeScreenGroups({
      groups: [
        { key: "hot", title: "热门股", total: 0, returned: 0, items: [] },
        { key: "potential", title: "潜力股", total: 0, returned: 0, items: [] },
      ],
    });

    expect(groups.map((group) => group.title)).toEqual(["热门股", "潜力股"]);
    expect(groups.every((group) => group.rows.length === 0)).toBe(true);
  });

  it("normalizes news sentiment object groups", () => {
    const groups = normalizeNewsGroups({ positive: [{ title: "good", source: "x" }], negative: [], mixed: [], uncertain: [] });
    expect(groups.mode).toBe("plain_news");
    expect(groups.positive).toHaveLength(1);
  });
});

describe("agent and upstream utilities", () => {
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
