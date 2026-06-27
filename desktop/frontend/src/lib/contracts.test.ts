import { describe, expect, it, vi } from "vitest";
import {
  buildBacktestRequest,
  buildGraphScreenRequest,
  buildNewsRagRequest,
  buildScreenCriteria,
  buildSectorScreenRequest,
  buildTrendScreenRequest,
  fetchUpstreamImportPayload,
  normalizeAgentStreamEvent,
  normalizeNewsGroups,
  normalizeScreenGroups,
  normalizeScreenRows,
  parseSseBlock,
  parseUpstreamImportDescriptor,
} from "./contracts";
import type { FilterCriteria } from "../components/FilterBar";

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
});
