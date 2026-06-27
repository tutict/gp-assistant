import type { FilterCriteria } from "../components/FilterBar";
import type {
  AgentStreamEvent,
  BacktestResult,
  GraphScreenResult,
  GraphStockSignal,
  LlmClientConfig,
  LlmSettings,
  NewsEvidence,
  NewsRagResult,
  NewsSentimentGroups,
  ObserveResult,
  RagPackHit,
  ScreenCriteria,
  ScreenedStock,
  ScreenResult,
  SectorScreenGroup,
  SectorScreenRequest,
  SectorScreenResult,
  StockItem,
  StockRowView,
  TrendIndicatorResult,
  TrendScreenResult,
  TrendStockSignal,
  WatchlistItem,
} from "../types";
import {
  clampFloat,
  clampInt,
  currentSystemDateCompact,
  currentSystemDateInputValue,
  normalizeDateParam,
  normalizeStockCode,
  parseCodes,
} from "./format";

export interface ScreenRequestOptions {
  limit?: number;
  score_profile?: "quality" | "rotation";
}

export function buildScreenCriteria(criteria: FilterCriteria, overrides: ScreenRequestOptions = {}): ScreenCriteria {
  const payload: ScreenCriteria = {
    include_st: criteria.includeSt,
    require_institution_buy_ratio_gt_sell_ratio: criteria.requireInstitutionBuyRatio,
    min_deducted_net_profit_billion: 0,
    min_deducted_net_profit_growth_rate: 10,
    limit: clampInt(overrides.limit ?? criteria.resultLimit, 1, 200, 10),
    sort_by: criteria.sortBy || "score",
    sort_dir: criteria.sortDir || "desc",
    score_profile: overrides.score_profile || "rotation",
  };
  if (criteria.industry) payload.industry = criteria.industry;
  if (criteria.minRoe) payload.min_roe = Number(criteria.minRoe);
  if (criteria.maxPe) payload.max_pe = Number(criteria.maxPe);
  if (criteria.maxPb) payload.max_pb = Number(criteria.maxPb);
  if (criteria.minMcap) payload.min_market_cap_billion = Number(criteria.minMcap);
  return payload;
}

export function buildSectorScreenRequest(
  criteria: FilterCriteria,
  groupBy: SectorScreenRequest["group_by"],
  perSectorLimit: number,
  maxSectors?: number,
): SectorScreenRequest {
  const per = clampInt(perSectorLimit, 1, 50, 5);
  return {
    criteria: buildScreenCriteria(criteria),
    group_by: groupBy,
    max_sectors: clampInt(maxSectors ?? (groupBy === "board" ? 5 : 12), 1, 50, groupBy === "board" ? 5 : 12),
    per_sector_limit: per,
    min_sector_candidates: groupBy === "board" ? 1 : per,
  };
}

export function buildGraphScreenRequest(
  criteria: FilterCriteria,
  seedCodesRaw: string,
  relationDepth: number,
  relationWeight: number,
): Record<string, unknown> {
  return {
    criteria: buildScreenCriteria(criteria, { limit: 100 }),
    seed_codes: parseStockCodeList(seedCodesRaw).slice(0, 50),
    relation_depth: clampInt(relationDepth, 1, 3, 1),
    relation_weight: clampFloat(relationWeight, 0, 1, 0.4),
    limit: Math.min(clampInt(criteria.resultLimit, 1, 200, 10), 100),
  };
}

export function buildTrendScreenRequest(
  criteria: FilterCriteria,
  startDate: string,
  endDate: string,
): Record<string, unknown> {
  return {
    criteria: buildScreenCriteria(criteria, { limit: 100 }),
    start_date: normalizeDateParam(startDate, "20200101"),
    end_date: normalizeDateParam(endDate, currentSystemDateCompact()),
    limit: Math.min(clampInt(criteria.resultLimit, 1, 200, 10), 100),
  };
}

export function buildTrendAnalyzeRequest(code: string, startDate: string, endDate: string): Record<string, unknown> {
  return {
    code,
    start_date: normalizeDateParam(startDate, "20200101"),
    end_date: normalizeDateParam(endDate, currentSystemDateCompact()),
    series_limit: 180,
  };
}

export function buildBacktestRequest(args: {
  source: "criteria" | "watchlist";
  criteria: FilterCriteria;
  watchlist: WatchlistItem[];
  startDate: string;
  endDate: string;
  topN: number;
  rebalanceFrequency: string;
  transactionCostBps: number;
  benchmark: string;
}): Record<string, unknown> {
  return {
    source: args.source,
    criteria: buildScreenCriteria(args.criteria, { limit: 100, score_profile: "quality" }),
    stock_codes: args.source === "watchlist" ? args.watchlist.map((item) => item.code).filter(Boolean).slice(0, 100) : [],
    start_date: normalizeDateParam(args.startDate, "20200101"),
    end_date: normalizeDateParam(args.endDate, currentSystemDateCompact()),
    top_n: clampInt(args.topN, 1, 100, 10),
    rebalance_frequency: args.rebalanceFrequency || "monthly",
    transaction_cost_bps: clampFloat(args.transactionCostBps, 0, 500, 10),
    benchmark: args.benchmark || "candidate_equal_weight",
  };
}

export function buildNewsRagRequest(code: string, days: number, llm?: LlmClientConfig): Record<string, unknown> {
  const payload: Record<string, unknown> = {
    code,
    seed_codes: code ? [code] : [],
    criteria: {},
    days: clampInt(days, 1, 365, 30),
    max_items: 24,
  };
  if (llm) payload.llm = llm;
  return payload;
}

export function buildRagPackBuildRequest(code: string, days: number, seedCodesRaw = ""): Record<string, unknown> {
  const stockCodes = code ? [code] : parseStockCodeList(seedCodesRaw);
  return {
    pack_version: `local-news-${new Date().toISOString().slice(0, 10)}`,
    days: clampInt(days, 1, 3650, 30),
    stock_codes: stockCodes,
    relation_types: [],
    source_tiers: ["filing", "news", "community"],
    limit: 1000,
    target_chars: 500,
    overlap_chars: 80,
  };
}

export function buildRagPackQueryRequest(query: string, code: string, seedCodesRaw = ""): Record<string, unknown> {
  const stockCodes = code ? [code] : parseStockCodeList(seedCodesRaw);
  return {
    query: query.trim() || (code ? `${code} upstream supply chain evidence` : "upstream supply chain evidence"),
    stock_codes: stockCodes,
    relation_types: [],
    source_tiers: ["filing", "news", "community"],
    top_k: 8,
  };
}

export function buildUpstreamRagBuildRequest(code: string, newsDays: number, manualUrlsRaw: string): Record<string, unknown> {
  return {
    code,
    data_until: currentSystemDateInputValue(),
    filing_days: 1095,
    news_days: clampInt(newsDays, 1, 3650, 180),
    manual_urls: parseManualUrls(manualUrlsRaw),
  };
}

export function buildLlmConfig(settings: LlmSettings | null | undefined): LlmClientConfig | undefined {
  if (!settings) return undefined;
  const config: LlmClientConfig = {};
  if (settings.api_key) config.api_key = settings.api_key;
  if (settings.base_url) config.base_url = settings.base_url.replace(/\/+$/, "");
  if (settings.model) config.model = settings.model;
  if (settings.temperature !== undefined) config.temperature = Number(settings.temperature);
  if (settings.timeout !== undefined) config.timeout_seconds = Number(settings.timeout);
  if (settings.json_mode !== undefined) config.json_mode = Boolean(settings.json_mode);
  return Object.keys(config).length ? config : undefined;
}

export function normalizeScreenRows(result: ScreenResult | GraphScreenResult | TrendScreenResult | TrendIndicatorResult | unknown): StockRowView[] {
  const data = asRecord(result);
  const items = Array.isArray(data.items) ? data.items : [];
  if (data.stock && data.signal) return [trendAnalyzeToRow(data as unknown as TrendIndicatorResult)];
  return items.map((item) => normalizeStockRow(item)).filter((item): item is StockRowView => Boolean(item));
}

export function normalizeSectorGroups(result: SectorScreenResult | ScreenResult | unknown): { title: string; meta: string; rows: StockRowView[] }[] {
  const groups = Array.isArray(asRecord(result).groups) ? (asRecord(result).groups as unknown[]) : [];
  return groups.map((group) => {
    const g = asRecord(group);
    const rows = Array.isArray(g.items) ? g.items.map((item) => normalizeStockRow(item)).filter(Boolean) as StockRowView[] : [];
    return {
      title: String(g.sector || g.title || g.key || "Group"),
      meta: `returned ${g.returned ?? rows.length} / total ${g.total ?? rows.length}`,
      rows,
    };
  });
}

export function normalizeStockRow(item: unknown): StockRowView | null {
  const raw = asRecord(item);
  const stock = (raw.stock && typeof raw.stock === "object" ? raw.stock : raw) as StockItem;
  if (!stock || !stock.code) return null;
  const graph = raw as Partial<GraphStockSignal>;
  const trend = raw as Partial<TrendStockSignal>;
  const screened = raw as Partial<ScreenedStock>;
  const score = firstNumber(raw.final_score, raw.score, graph.final_score, trend.final_score);
  return {
    code: stock.code,
    name: stock.name || stock.code,
    industry: stock.industry,
    price: firstNumber(stock.price),
    change_pct: firstNumber(stock.change_pct, stock.pct),
    pe: firstNumber(stock.pe),
    pb: firstNumber(stock.pb),
    roe: firstNumber(stock.roe),
    market_cap_billion: firstNumber(stock.market_cap_billion, stock.market_cap),
    score,
    scoreLabel: raw.final_score !== undefined ? "final" : "score",
    reasons: Array.isArray(raw.reasons) ? raw.reasons as string[] : [],
    concept: String(screened.concept || ""),
    factorScores: screened.factor_scores || {},
    explanation: (raw.explanation as StockRowView["explanation"]) || null,
    raw: item,
  };
}

export function normalizeNewsGroups(groups: NewsSentimentGroups | undefined): Required<NewsSentimentGroups> {
  const value = groups || {};
  return {
    mode: value.mode || "plain_news",
    positive: Array.isArray(value.positive) ? value.positive : [],
    negative: Array.isArray(value.negative) ? value.negative : [],
    mixed: Array.isArray(value.mixed) ? value.mixed : [],
    uncertain: Array.isArray(value.uncertain) ? value.uncertain : [],
  };
}

export function normalizeRagHit(hit: RagPackHit): { title: string; score: number | null; text: string; source: string; url: string } {
  return {
    title: hit.title || hit.document_id || "Untitled",
    score: typeof hit.score === "number" ? hit.score : null,
    text: hit.text || "",
    source: hit.source || hit.source_tier || "",
    url: hit.url || "",
  };
}

export function parseStockCodeList(raw: string): string[] {
  return parseCodes(raw)
    .map((item) => normalizeStockCode(item))
    .filter(Boolean);
}

export function parseManualUrls(raw: string): string[] {
  return String(raw || "")
    .split(/\r?\n|[,，]/)
    .map((item) => item.trim())
    .filter((item) => /^https?:\/\//i.test(item))
    .slice(0, 12);
}

export interface UpstreamImportDescriptor {
  manifest_url?: string;
  pack_url?: string;
  manifest?: Record<string, unknown>;
  pack_base64?: string;
}

export function parseUpstreamImportDescriptor(rawValue: string): UpstreamImportDescriptor {
  const raw = String(rawValue || "").trim();
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as UpstreamImportDescriptor;
    if (parsed && typeof parsed === "object") {
      const record = parsed as UpstreamImportDescriptor & { manifestUrl?: string; packUrl?: string; packBase64?: string };
      return {
        manifest_url: record.manifest_url || record.manifestUrl,
        pack_url: record.pack_url || record.packUrl,
        manifest: record.manifest,
        pack_base64: record.pack_base64 || record.packBase64,
      };
    }
  } catch {
    // fall through to URL parser
  }
  if (/^https?:\/\//i.test(raw)) return { manifest_url: raw, pack_url: deriveUpstreamPackUrl(raw) };
  return {};
}

export function deriveUpstreamPackUrl(manifestUrl: string, packFile = "rag_pack.sqlite"): string {
  try {
    return new URL(packFile, manifestUrl).toString();
  } catch {
    return "";
  }
}

export async function fetchUpstreamImportPayload(descriptor: UpstreamImportDescriptor): Promise<Record<string, unknown>> {
  if (descriptor.manifest && descriptor.pack_base64) {
    return { manifest: descriptor.manifest, pack_base64: descriptor.pack_base64 };
  }
  if (!descriptor.manifest_url) throw new Error("Missing manifest_url");
  const manifestResponse = await fetch(descriptor.manifest_url, { cache: "no-store" });
  if (!manifestResponse.ok) throw new Error(`Manifest download failed: HTTP ${manifestResponse.status}`);
  const manifest = await manifestResponse.json() as Record<string, unknown>;
  const packUrl = descriptor.pack_url || deriveUpstreamPackUrl(descriptor.manifest_url, upstreamPackFileName(manifest));
  const packResponse = await fetch(packUrl, { cache: "no-store" });
  if (!packResponse.ok) throw new Error(`Pack download failed: HTTP ${packResponse.status}`);
  const buffer = await packResponse.arrayBuffer();
  return { manifest, pack_base64: arrayBufferToBase64(buffer) };
}

export function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function parseSseBlock(block: string): AgentStreamEvent | null {
  const lines = String(block || "").split(/\r?\n/);
  let eventType = "message";
  const dataLines: string[] = [];
  for (const line of lines) {
    if (line.startsWith("event:")) eventType = line.slice(6).trim();
    if (line.startsWith("data:")) dataLines.push(line.slice(5).trimStart());
  }
  if (!dataLines.length) return null;
  try {
    const payload = JSON.parse(dataLines.join("\n")) as AgentStreamEvent;
    if (!payload.type) payload.type = eventType;
    return payload;
  } catch {
    return { type: eventType, message: dataLines.join("\n") };
  }
}

export function normalizeAgentStreamEvent(rawEvent: unknown): AgentStreamEvent | null {
  if (!rawEvent) return null;
  const event = asRecord(rawEvent).payload ?? rawEvent;
  if (typeof event === "string") {
    try {
      return JSON.parse(event) as AgentStreamEvent;
    } catch {
      return null;
    }
  }
  return event && typeof event === "object" ? event as AgentStreamEvent : null;
}

export function actionResultKind(result: unknown): "screen" | "sector" | "graph" | "trend" | "backtest" | "observe" | "news" | "data" | "unknown" {
  const action = String(asRecord(result).action || "");
  if (action === "screen") return "screen";
  if (action === "sector_screen") return "sector";
  if (action === "graph_screen") return "graph";
  if (action === "trend_screen") return "trend";
  if (action === "backtest") return "backtest";
  if (action === "observe_stock") return "observe";
  if (action === "news_rag") return "news";
  if (["data_status", "refresh_data", "prune_cache"].includes(action)) return "data";
  return "unknown";
}

function trendAnalyzeToRow(result: TrendIndicatorResult): StockRowView {
  return {
    ...normalizeStockRow({ stock: result.stock, final_score: result.signal?.quant_score, reasons: result.signal?.reasons || [] })!,
    scoreLabel: "trend",
    raw: result,
  };
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function firstNumber(...values: unknown[]): number | undefined {
  for (const value of values) {
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return undefined;
}

function upstreamPackFileName(manifest: Record<string, unknown>): string {
  const files = manifest.files;
  if (files && typeof files === "object" && !Array.isArray(files)) {
    const pack = (files as Record<string, unknown>).pack;
    if (typeof pack === "string" && pack.trim()) return pack.trim();
  }
  if (typeof files === "string" && files.trim()) return files.trim();
  return "rag_pack.sqlite";
}

export type { NewsEvidence, BacktestResult, ObserveResult };
