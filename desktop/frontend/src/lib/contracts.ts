import type { FilterCriteria } from "../components/FilterBar";
import type {
  AgentResult,
  AgentStreamEvent,
  AdaptiveScreenMode,
  AdaptiveScreenRequest,
  BacktestResult,
  GraphScreenResult,
  GraphStockSignal,
  LlmClientConfig,
  LlmSettings,
  NewsEvidence,
  NewsSentimentGroups,
  ObserveResult,
  RagPackHit,
  ScreenCriteria,
  ScreenedStock,
  ScreenResult,
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
  trendScreenStartDateInputValue,
} from "./format";

export interface ScreenRequestOptions {
  limit?: number;
  score_profile?: "balanced" | "quality" | "trend" | "rotation";
}

export interface StockGroupView {
  key: string;
  title: string;
  description: string;
  meta: string;
  rows: StockRowView[];
  total: number;
  returned: number;
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
    score_profile: overrides.score_profile || criteria.scoreProfile || "balanced",
  };
  if (criteria.industry) payload.industry = criteria.industry;
  if (criteria.minRoe) payload.min_roe = Number(criteria.minRoe) / 100;
  if (criteria.maxPe) payload.max_pe = Number(criteria.maxPe);
  if (criteria.maxPb) payload.max_pb = Number(criteria.maxPb);
  if (criteria.minMcap) payload.min_market_cap_billion = Number(criteria.minMcap);
  return payload;
}

export function buildAdaptiveScreenRequest(
  criteria: FilterCriteria,
  mode: AdaptiveScreenMode = "auto",
  runId: string = crypto.randomUUID?.() || "screen-" + Date.now(),
): AdaptiveScreenRequest {
  return {
    criteria: buildScreenCriteria(criteria, { limit: 80 }),
    mode,
    horizon: "swing_10_30d",
    primary_limit: 10,
    exploration_limit: 10,
    run_id: runId,
  };
}

export function isAdaptiveProgressForRun(
  payload: unknown,
  activeRunId: string | null,
): payload is { run_id: string; percent?: number; message?: string } {
  if (!activeRunId || !payload || typeof payload !== "object" || Array.isArray(payload)) return false;
  return (payload as Record<string, unknown>).run_id === activeRunId;
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

export function buildCustomScreenRequest(criteria: FilterCriteria): Record<string, unknown> {
  return {
    criteria: buildScreenCriteria(criteria, { limit: 100 }),
    seed_codes: [],
    seed_query: "",
    relation_depth: 1,
    relation_weight: 0,
    limit: Math.min(clampInt(criteria.resultLimit, 1, 200, 10), 100),
  };
}

export function buildGraphScreenRequest(criteria: FilterCriteria): Record<string, unknown> {
  return buildCustomScreenRequest(criteria);
}


export function buildTrendScreenRequest(
  criteria: FilterCriteria,
  startDate: string,
  endDate: string,
): Record<string, unknown> {
  const normalizedEndDate = normalizeDateParam(endDate, currentSystemDateCompact());
  const normalizedStartDate = normalizeDateParam(
    trendScreenStartDateInputValue(startDate, endDate),
    "20200101",
  );
  return {
    criteria: buildScreenCriteria(criteria, { limit: 100 }),
    start_date: normalizedStartDate,
    end_date: normalizedEndDate,
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
  strategyMode?: string;
  adaptiveScreenSpec?: AdaptiveScreenRequest;
}): Record<string, unknown> {
  const adaptiveScreenSpec = args.adaptiveScreenSpec;
  return {
    source: args.source,
    criteria: adaptiveScreenSpec?.criteria
      || buildScreenCriteria(args.criteria, { limit: 100, score_profile: "quality" }),
    strategy_mode: adaptiveScreenSpec
      ? "adaptive_swing_v1:" + adaptiveScreenSpec.mode
      : args.strategyMode || "candidate_snapshot",
    ...(adaptiveScreenSpec ? { adaptive_screen_spec: adaptiveScreenSpec } : {}),
    stock_codes: args.source === "watchlist" ? args.watchlist.map((item) => item.code).filter(Boolean).slice(0, 100) : [],
    start_date: normalizeDateParam(args.startDate, "20200101"),
    end_date: normalizeDateParam(args.endDate, currentSystemDateCompact()),
    top_n: clampInt(args.topN, 1, 100, 10),
    rebalance_frequency: args.rebalanceFrequency || "monthly",
    transaction_cost_bps: clampFloat(args.transactionCostBps, 0, 500, 10),
    benchmark: args.benchmark || "candidate_equal_weight",
  };
}

function isRecordValue(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string" && item.trim().length > 0);
}

function isOptionalFinite(value: unknown): boolean {
  return value == null || Number.isFinite(value);
}

function isNumericRecord(value: unknown, required: string[], optional: string[] = []): boolean {
  if (!isRecordValue(value)) return false;
  return required.every((key) => Number.isFinite(value[key]))
    && optional.every((key) => isOptionalFinite(value[key]));
}

function isEquityPoint(value: unknown): boolean {
  return isRecordValue(value) && typeof value.date === "string" && Number.isFinite(value.equity);
}

function isWalkForwardFold(value: unknown): boolean {
  if (!isRecordValue(value)) return false;
  return typeof value.selection_date === "string"
    && (value.evaluation_end_date == null || typeof value.evaluation_end_date === "string")
    && isStringArray(value.selected_symbols)
    && Number.isFinite(value.eligible_symbol_count)
    && Number.isFinite(value.evaluated_selection_count)
    && Number.isFinite(value.hit_count)
    && [
      value.precision_at_n,
      value.average_forward_return,
      value.benchmark_forward_return,
      value.average_excess_return,
    ].every(isOptionalFinite);
}

function isVolatilitySnapshot(value: unknown): boolean {
  if (!isRecordValue(value) || typeof value.symbol !== "string" || !value.symbol.trim() || typeof value.date !== "string") {
    return false;
  }
  const unavailableValid = value.unavailable == null || (
    Array.isArray(value.unavailable)
    && value.unavailable.every((item) => isRecordValue(item)
      && typeof item.indicator === "string"
      && typeof item.reason === "string")
  );
  return isOptionalFinite(value.close)
    && (value.atr == null || isNumericRecord(value.atr, ["period", "value", "percent_of_close"]))
    && (value.bollinger_bands == null || isNumericRecord(
      value.bollinger_bands,
      ["period", "multiplier", "upper", "middle", "lower"],
      ["bandwidth_percent", "percent_b"],
    ))
    && (value.donchian_channel == null || isNumericRecord(
      value.donchian_channel,
      ["period", "upper", "middle", "lower"],
      ["width_percent", "position_percent"],
    ))
    && (value.keltner_channel == null || isNumericRecord(
      value.keltner_channel,
      ["ema_period", "atr_period", "multiplier", "upper", "middle", "lower"],
      ["width_percent", "position_percent"],
    ))
    && (value.chaikin_volatility == null || isNumericRecord(
      value.chaikin_volatility,
      ["ema_period", "roc_period", "value"],
    ))
    && (value.rvi == null || isNumericRecord(value.rvi, ["period", "value"]))
    && unavailableValid;
}

export function requireBacktestResult(value: unknown): BacktestResult {
  const result = asRecord(value);
  const metrics = asRecord(result.metrics);
  const equityCurve = Array.isArray(result.equity_curve) ? result.equity_curve : null;
  const symbols = Array.isArray(result.symbols) ? result.symbols : null;
  const optionalMetricKeys = [
    "annualized_return",
    "max_drawdown",
    "benchmark_total_return",
    "benchmark_annualized_return",
    "benchmark_max_drawdown",
    "excess_return",
    "total_transaction_cost",
    "total_turnover",
    "rebalance_count",
    "oos_fold_count",
    "evaluated_selection_count",
    "selection_hit_count",
    "precision_at_n",
  ];
  if (
    Array.isArray(result.metrics)
    || !Number.isFinite(metrics.total_return)
    || !Number.isFinite(metrics.num_stocks)
    || optionalMetricKeys.some((key) => !isOptionalFinite(metrics[key]))
    || (metrics.strategy_mode != null && typeof metrics.strategy_mode !== "string")
    || !equityCurve
    || equityCurve.some((point) => !isEquityPoint(point))
    || !symbols
    || !isStringArray(symbols)
    || (result.benchmark_curve != null && (
      !Array.isArray(result.benchmark_curve) || result.benchmark_curve.some((point) => !isEquityPoint(point))
    ))
    || (result.benchmark_symbols != null && !isStringArray(result.benchmark_symbols))
    || (result.rebalance_dates != null && !isStringArray(result.rebalance_dates))
    || (result.walk_forward_folds != null && (
      !Array.isArray(result.walk_forward_folds) || result.walk_forward_folds.some((fold) => !isWalkForwardFold(fold))
    ))
    || (result.volatility_snapshots != null && (
      !Array.isArray(result.volatility_snapshots)
      || result.volatility_snapshots.some((snapshot) => !isVolatilitySnapshot(snapshot))
    ))
    || (result.volatility_message != null && typeof result.volatility_message !== "string")
    || (result.strategy_mode != null && typeof result.strategy_mode !== "string")
    || (result.notes != null && !isStringArray(result.notes))
  ) {
    throw new Error("回测接口未返回有效结果，请刷新数据后重试。");
  }
  return value as BacktestResult;
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

export function sanitizePersistedLlmSettings(settings: LlmSettings | null | undefined): LlmSettings | null {
  if (!settings) return null;
  const source = normalizeLlmSettings(settings);
  const sanitized: LlmSettings = {
    active_provider_id: source.active_provider_id,
    providers: source.providers?.map((provider) => {
      const sanitizedProvider = { ...provider };
      if (!sanitizedProvider.remember_key) delete sanitizedProvider.api_key;
      return sanitizedProvider;
    }),
  };
  return sanitized.providers?.length ? sanitized : null;
}

export function buildLlmConfig(settings: LlmSettings | null | undefined): LlmClientConfig | undefined {
  const active = activeLlmProvider(settings);
  if (!active?.model?.trim() || (!active.api_key?.trim() && !active.base_url?.trim())) return undefined;
  const config: LlmClientConfig = {};
  if (active.api_key) config.api_key = active.api_key;
  if (active.base_url) config.base_url = active.base_url.replace(/\/+$/, "");
  if (active.model) config.model = active.model;
  if (active.temperature !== undefined) config.temperature = Number(active.temperature);
  if (active.timeout !== undefined) config.timeout_seconds = Number(active.timeout);
  if (active.json_mode !== undefined) config.json_mode = Boolean(active.json_mode);
  return Object.keys(config).length ? config : undefined;
}

export function normalizeLlmSettings(settings: LlmSettings | null | undefined): LlmSettings {
  if (!settings) {
    const provider = defaultLlmProvider();
    return { active_provider_id: provider.id, providers: [provider] };
  }
  if (settings.providers?.length) {
    const providers = settings.providers.map((provider, index) => ({
      ...provider,
      id: provider.id || `provider-${index + 1}`,
      name: provider.name || provider.provider || provider.model || `Provider ${index + 1}`,
      provider: provider.provider || "custom",
    }));
    const activeProviderId = providers.some((provider) => provider.id === settings.active_provider_id)
      ? settings.active_provider_id
      : providers[0]?.id;
    return { active_provider_id: activeProviderId, providers };
  }

  const legacy = settings as LlmSettings & LlmClientConfig & { remember_key?: boolean; timeout?: number };
  return {
    active_provider_id: "legacy",
    providers: [{
      id: "legacy",
      name: legacy.model || "自定义",
      provider: "custom",
      api_key: legacy.api_key,
      base_url: legacy.base_url,
      model: legacy.model,
      temperature: legacy.temperature,
      timeout: legacy.timeout,
      json_mode: legacy.json_mode,
      remember_key: legacy.remember_key,
    }],
  };
}

export function activeLlmProvider(settings: LlmSettings | null | undefined) {
  const normalized = normalizeLlmSettings(settings);
  return normalized.providers?.find((provider) => provider.id === normalized.active_provider_id)
    || normalized.providers?.[0];
}

function defaultLlmProvider() {
  return {
    id: "compatible",
    name: "通用兼容",
    provider: "openai-compatible",
    base_url: "",
    model: "",
    temperature: 0.7,
    timeout: 60,
    json_mode: false,
    remember_key: false,
  };
}

export function normalizeScreenRows(result: ScreenResult | GraphScreenResult | TrendScreenResult | TrendIndicatorResult | unknown): StockRowView[] {
  const data = asRecord(result);
  const items = Array.isArray(data.items) ? data.items : [];
  if (data.stock && data.signal) return [trendAnalyzeToRow(data as unknown as TrendIndicatorResult)];
  return items.map((item) => normalizeStockRow(item)).filter((item): item is StockRowView => Boolean(item));
}

export function normalizeScreenGroups(result: ScreenResult | unknown): StockGroupView[] {
  const groups = Array.isArray(asRecord(result).groups) ? (asRecord(result).groups as unknown[]) : [];
  return groups.map((group) => normalizeStockGroup(group, "screen"));
}

export function normalizeSectorGroups(result: SectorScreenResult | ScreenResult | unknown): StockGroupView[] {
  const groups = Array.isArray(asRecord(result).groups) ? (asRecord(result).groups as unknown[]) : [];
  return groups.map((group) => normalizeStockGroup(group, "sector"));
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
    eps: firstNumber(stock.eps, stock.latest_eps, raw.eps, raw.latest_eps),
    roe: firstNumber(stock.roe),
    market_cap_billion: firstNumber(stock.market_cap_billion, stock.market_cap),
    score,
    scoreLabel: raw.final_score !== undefined ? "final" : raw.balanced_score !== undefined ? "balanced" : "score",
    reasons: Array.isArray(raw.reasons) ? raw.reasons as string[] : [],
    concept: String(screened.concept || ""),
    qualityScore: firstNumber(screened.quality_score),
    trendScore: firstNumber(screened.trend_score),
    riskScore: firstNumber(screened.risk_score),
    balancedScore: firstNumber(screened.balanced_score),
    factorScores: screened.factor_scores || {},
    scoreBreakdown: Array.isArray(screened.score_breakdown) ? screened.score_breakdown : [],
    reasonTags: Array.isArray(screened.reason_tags) ? screened.reason_tags : [],
    riskTags: Array.isArray(screened.risk_tags) ? screened.risk_tags : [],
    suitablePeriods: Array.isArray(screened.suitable_periods) ? screened.suitable_periods : [],
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

const UPSTREAM_IMPORT_TIMEOUT_MS = 15000;
const UPSTREAM_MANIFEST_MAX_BYTES = 256 * 1024;
const UPSTREAM_PACK_MAX_BYTES = 25 * 1024 * 1024;

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
    assertManifestPackSize(descriptor.manifest);
    const packBase64 = normalizePackBase64(descriptor.pack_base64);
    return { manifest: descriptor.manifest, pack_base64: packBase64 };
  }
  if (!descriptor.manifest_url) throw new Error("Missing manifest_url");

  const manifestUrl = validateUpstreamImportUrl(descriptor.manifest_url, "Manifest URL");
  const manifestText = await fetchTextWithLimit(manifestUrl, UPSTREAM_MANIFEST_MAX_BYTES, "Manifest");
  let manifest: Record<string, unknown>;
  try {
    manifest = JSON.parse(manifestText) as Record<string, unknown>;
  } catch {
    throw new Error("Manifest is not valid JSON.");
  }
  assertManifestPackSize(manifest);

  const rawPackUrl = descriptor.pack_url || deriveUpstreamPackUrl(manifestUrl, upstreamPackFileName(manifest));
  const packUrl = validateUpstreamImportUrl(rawPackUrl, "Pack URL", manifestUrl);
  const buffer = await fetchArrayBufferWithLimit(packUrl, UPSTREAM_PACK_MAX_BYTES, "Pack");
  return { manifest, pack_base64: arrayBufferToBase64(buffer) };
}

function validateUpstreamImportUrl(rawUrl: string | undefined, label: string, expectedOrigin?: string): string {
  if (!rawUrl) throw new Error(`${label} is missing.`);
  let url: URL;
  try {
    url = new URL(rawUrl);
  } catch {
    throw new Error(`${label} is not a valid URL.`);
  }
  if (url.protocol !== "https:") throw new Error(`${label} must use HTTPS.`);
  if (isBlockedImportHost(url.hostname)) throw new Error(`${label} cannot target local or private hosts.`);
  if (expectedOrigin && url.origin !== new URL(expectedOrigin).origin) {
    throw new Error("Pack URL must use the same origin as the manifest URL.");
  }
  url.username = "";
  url.password = "";
  return url.toString();
}

function isBlockedImportHost(hostname: string): boolean {
  const host = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  if (!host || host === "localhost" || host.endsWith(".localhost") || host.endsWith(".local")) return true;
  const ipv4 = parseIpv4Address(host);
  if (ipv4) return isPrivateOrReservedIpv4(ipv4);
  if (host.includes(":")) {
    if (host === "::" || host === "::1") return true;
    if (/^f[cd]/.test(host) || /^fe[89ab]/.test(host)) return true;
  }
  return false;
}

function parseIpv4Address(host: string): number[] | null {
  const parts = host.split(".");
  if (parts.length !== 4) return null;
  const bytes = parts.map((part) => Number(part));
  if (bytes.some((byte, index) => !Number.isInteger(byte) || byte < 0 || byte > 255 || String(byte) !== parts[index])) return null;
  return bytes;
}

function isPrivateOrReservedIpv4(bytes: number[]): boolean {
  const [a, b] = bytes;
  if (a === 0 || a === 10 || a === 127) return true;
  if (a === 100 && b >= 64 && b <= 127) return true;
  if (a === 169 && b === 254) return true;
  if (a === 172 && b >= 16 && b <= 31) return true;
  if (a === 192 && (b === 0 || b === 168)) return true;
  if (a === 198 && (b === 18 || b === 19)) return true;
  return a >= 224;
}

function assertManifestPackSize(manifest: Record<string, unknown>): void {
  const rawSize = manifest.file_size;
  if (rawSize === undefined || rawSize === null || rawSize === "") return;
  const size = Number(rawSize);
  if (Number.isFinite(size) && size > UPSTREAM_PACK_MAX_BYTES) {
    throw new Error(`Pack exceeds the ${UPSTREAM_PACK_MAX_BYTES} byte import limit.`);
  }
}

function normalizePackBase64(rawValue: string): string {
  const normalized = String(rawValue || "").replace(/\s/g, "");
  const padding = normalized.endsWith("==") ? 2 : normalized.endsWith("=") ? 1 : 0;
  const estimatedBytes = Math.max(0, Math.floor((normalized.length * 3) / 4) - padding);
  if (estimatedBytes > UPSTREAM_PACK_MAX_BYTES) {
    throw new Error(`Pack exceeds the ${UPSTREAM_PACK_MAX_BYTES} byte import limit.`);
  }
  return normalized;
}

async function fetchTextWithLimit(url: string, maxBytes: number, label: string): Promise<string> {
  const buffer = await fetchArrayBufferWithLimit(url, maxBytes, label);
  return new TextDecoder().decode(buffer);
}

async function fetchArrayBufferWithLimit(url: string, maxBytes: number, label: string): Promise<ArrayBuffer> {
  const controller = new AbortController();
  const timeoutId = globalThis.setTimeout(() => controller.abort(), UPSTREAM_IMPORT_TIMEOUT_MS);
  try {
    const response = await fetch(url, { cache: "no-store", signal: controller.signal });
    if (!response.ok) throw new Error(`${label} download failed: HTTP ${response.status}`);
    assertContentLengthWithinLimit(response, maxBytes, label);
    return await readResponseArrayBufferWithLimit(response, maxBytes, label);
  } catch (error) {
    if (controller.signal.aborted) throw new Error(`${label} download timed out.`);
    throw error;
  } finally {
    globalThis.clearTimeout(timeoutId);
  }
}

function assertContentLengthWithinLimit(response: Response, maxBytes: number, label: string): void {
  const contentLength = response.headers.get("content-length");
  if (!contentLength) return;
  const bytes = Number(contentLength);
  if (Number.isFinite(bytes) && bytes > maxBytes) {
    throw new Error(`${label} exceeds the ${maxBytes} byte import limit.`);
  }
}

async function readResponseArrayBufferWithLimit(response: Response, maxBytes: number, label: string): Promise<ArrayBuffer> {
  const reader = response.body?.getReader();
  if (!reader) {
    const buffer = await response.arrayBuffer();
    if (buffer.byteLength > maxBytes) throw new Error(`${label} exceeds the ${maxBytes} byte import limit.`);
    return buffer;
  }

  const chunks: Uint8Array[] = [];
  let total = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    if (!value) continue;
    total += value.byteLength;
    if (total > maxBytes) {
      await reader.cancel().catch(() => undefined);
      throw new Error(`${label} exceeds the ${maxBytes} byte import limit.`);
    }
    chunks.push(value);
  }

  const output = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return output.buffer;
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

export function normalizeAgentResult(rawResult: unknown): AgentResult {
  const result = asRecord(rawResult) as AgentResult;
  const data = asRecord(result.data);
  const action = String(result.action || data.action || "");
  const normalized: AgentResult = { ...result };
  if (action && !normalized.action) normalized.action = action;
  if (!Array.isArray(normalized.tool_calls)) normalized.tool_calls = [];
  if (!Array.isArray(normalized.evidence_summary)) normalized.evidence_summary = [];
  if (!Array.isArray(normalized.answer_sections)) normalized.answer_sections = [];
  if (!Array.isArray(normalized.model_answer_sections)) normalized.model_answer_sections = [];
  if (!Array.isArray(normalized.warnings)) normalized.warnings = [];
  if (!Array.isArray(normalized.next_actions)) normalized.next_actions = [];
  return normalized;
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

function normalizeStockGroup(group: unknown, mode: "screen" | "sector"): StockGroupView {
  const g = asRecord(group);
  const key = String(g.key || g.sector || "");
  const rows = Array.isArray(g.items) ? g.items.map((item) => normalizeStockRow(item)).filter((item): item is StockRowView => Boolean(item)) : [];
  const total = Number.isFinite(Number(g.total)) ? Number(g.total) : rows.length;
  const returned = Number.isFinite(Number(g.returned)) ? Number(g.returned) : rows.length;
  return {
    key,
    title: String(g.sector || g.title || screenGroupTitle(key) || (mode === "screen" ? "筛选分组" : "分组")),
    description: String(g.description || ""),
    meta: `返回 ${returned} / 总数 ${total}`,
    rows,
    total,
    returned,
  };
}

function screenGroupTitle(key: string): string {
  const titles: Record<string, string> = {
    hot: "热门股",
    potential: "潜力股",
  };
  return titles[key] || "";
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


