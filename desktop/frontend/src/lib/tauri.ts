import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import {
  clampInt,
  currentSystemDateCompact,
  expectedMarketQuoteDateCompact,
  localTradeDateCompact,
  normalizeDateParam,
  normalizeStockCode,
  parseLooseNumber,
  relationStatusLabel,
  relationTypeLabel,
  stockCodeDigits,
  uniqueCompactStrings,
  uniqueNotes,
} from "./format";

declare global {
  interface Window {
    __TAURI__?: {
      core?: { invoke: <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T> };
      event?: { listen: (event: string, handler: (event: unknown) => void) => Promise<() => void> };
    };
    __TAURI_INTERNALS__?: unknown;
    __TAURI_IPC__?: unknown;
  }
}

type InvokeFn = <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>;
type ListenFn = (event: string, handler: (event: unknown) => void) => Promise<() => void>;
type TauriRouteHandler = (ctx: { invoke: InvokeFn; parsed: URL; path: string; payload?: unknown }) => Promise<unknown>;

export interface RequestOptions {
  timeoutMs?: number;
  signal?: AbortSignal | null;
  headers?: Record<string, string>;
}

export interface MarketRefreshLogEntry {
  time: string;
  message: string;
  tone: string;
}

export interface MarketRefreshOptions {
  mode?: string;
  max_bytes?: number;
  daily_days?: number;
  minute_days?: number;
  batch_start?: number;
  batch_count?: number;
  max_candidates?: number;
  max_failed_batches?: number;
  max_refresh_loops?: number;
  validate_after_write?: boolean;
  onLog?: (entry: MarketRefreshLogEntry) => void;
}

const STOCK_SEARCH_LIMIT = 5;
const MOBILE_OBSERVE_PREFETCH_TIMEOUT_MS = 5000;
const MOBILE_OBSERVE_INVOKE_TIMEOUT_MS = 55000;
const MOBILE_NEWS_RAG_TIMEOUT_MS = 45000;
const MOBILE_MARKET_REFRESH_INVOKE_TIMEOUT_MS = 60000;
const MOBILE_TENCENT_MAX_CANDIDATES = 15000;
const MOBILE_TENCENT_BATCHES_PER_STEP = 12;
const OBSERVE_FULL_HISTORY_LIMIT = 10000;
const OBSERVE_HISTORY_LIMIT = OBSERVE_FULL_HISTORY_LIMIT;
const MOBILE_FINANCIAL_SNAPSHOT_URL = new URL("mobile-financial-snapshot.json", window.location.href).toString();
const MOBILE_DEDUCTED_FINANCIAL_FIELDS = [
  "deducted_net_profit_billion",
  "deducted_net_profit_margin",
  "deducted_net_profit_growth_rate",
];

let mobileFinancialSnapshotPromise: Promise<Record<string, unknown> | null> | null = null;

function hasTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  return Boolean(window.__TAURI_INTERNALS__ || window.__TAURI_IPC__ || window.__TAURI__?.core?.invoke);
}

export function isTauriRuntime(): boolean {
  return hasTauriRuntime();
}

export function isMobileTauriRuntime(): boolean {
  return isTauriRuntime() && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent || "");
}

export function getTauriInvoke(): InvokeFn | undefined {
  if (!hasTauriRuntime()) return undefined;
  return window.__TAURI__?.core?.invoke || (tauriInvoke as InvokeFn);
}

export function getTauriListen(): ListenFn | undefined {
  if (!hasTauriRuntime()) return undefined;
  return window.__TAURI__?.event?.listen || (tauriListen as ListenFn);
}

export const TAURI_GET_ROUTES: Record<string, TauriRouteHandler> = {
  "/health": async ({ invoke }) => invoke("api_health"),
  "/api/strategies": async ({ invoke }) => invoke("api_strategies"),
  "/api/data-sources": async ({ invoke }) => invoke("api_data_sources"),
  "/api/data-sources/status": async ({ invoke }) => invoke("api_market_status"),
  "/api/rag-pack/status": async ({ invoke }) => invoke("api_rag_pack_status"),
  "/api/upstream-rag/mobile/list": async ({ invoke }) => invoke("core_upstream_rag_list"),
  "/api/upstream-rag/status": async ({ invoke }) => invoke("api_upstream_rag_status"),
  "/api/upstream-rag/mobile/detail": async ({ invoke, parsed }) => invoke("core_upstream_rag_detail", {
    payload: {
      stock_code: parsed.searchParams.get("stock_code") || "",
      pack_version: parsed.searchParams.get("pack_version") || "",
    },
  }),
  "/api/stock-search": async ({ invoke, parsed }) => invoke("api_stock_search", {
    payload: { q: parsed.searchParams.get("q") || "", limit: parsed.searchParams.get("limit") || STOCK_SEARCH_LIMIT },
  }),
};

export const TAURI_GET_PREFIX_ROUTES: { prefix: string; handler: TauriRouteHandler }[] = [
  {
    prefix: "/api/stocks/",
    handler: async ({ invoke, path }) => invoke("api_stock_get", { payload: { code: decodeURIComponent(path.slice("/api/stocks/".length)) } }),
  },
  {
    prefix: "/api/minutes/",
    handler: async ({ invoke, path, parsed }) => invoke("api_minutes", {
      payload: {
        code: decodeURIComponent(path.slice("/api/minutes/".length)),
        start: parsed.searchParams.get("start") || "",
        end: parsed.searchParams.get("end") || "",
        period: parsed.searchParams.get("period") || "1",
        limit: parsed.searchParams.get("limit") || 500,
      },
    }),
  },
  {
    prefix: "/api/order-book/",
    handler: async ({ invoke, path }) => invoke("api_order_book", { payload: { code: decodeURIComponent(path.slice("/api/order-book/".length)) } }),
  },
  {
    prefix: "/api/observe/",
    handler: async ({ invoke, path, parsed }) => {
      const code = normalizeStockCode(decodeURIComponent(path.slice("/api/observe/".length)));
      const payload: Record<string, unknown> = withAndroidNetworkOptions({
        code,
        start_date: normalizeDateParam(parsed.searchParams.get("start_date"), "19900101"),
        end_date: normalizeDateParam(parsed.searchParams.get("end_date"), currentSystemDateCompact()),
        series_limit: clampInt(parsed.searchParams.get("series_limit"), 20, OBSERVE_FULL_HISTORY_LIMIT, OBSERVE_FULL_HISTORY_LIMIT),
        include_order_book: parsed.searchParams.get("include_order_book") === "true",
        include_chip_distribution: parsed.searchParams.get("include_chip_distribution") !== "false",
      });
      if (isMobileTauriRuntime()) {
        payload.mobile_fast_observe = true;
        const financialSnapshot = await loadMobileFinancialSnapshotForCode(code).catch(() => null);
        if (financialSnapshot) payload.financial_snapshot = financialSnapshot;
        const history = await fetchObserveDailyHistoryForTauri(payload, MOBILE_OBSERVE_PREFETCH_TIMEOUT_MS).catch(() => null);
        if (Array.isArray(history) && history.length) payload.history = history;
      }
      const observeInvoke = invoke("api_observe", { payload });
      return isMobileTauriRuntime()
        ? withTimeout(observeInvoke, MOBILE_OBSERVE_INVOKE_TIMEOUT_MS, `移动端观察超过 ${Math.round(MOBILE_OBSERVE_INVOKE_TIMEOUT_MS / 1000)} 秒未返回，已中止等待。`)
        : observeInvoke;
    },
  },
];

export const TAURI_POST_ROUTES: Record<string, TauriRouteHandler> = {
  "/api/screen": async ({ invoke, payload }) => invoke("api_screen", { payload }),
  "/api/sector-screen": async ({ invoke, payload }) => invoke("api_sector_screen", { payload }),
  "/api/graph-screen": async ({ invoke, payload }) => invoke("api_graph_screen", { payload }),
  "/api/trend": async ({ invoke, payload }) => invoke("api_trend_analyze", { payload }),
  "/api/trend-screen": async ({ invoke, payload }) => invoke("api_trend_screen", { payload }),
  "/api/backtest": async ({ invoke, payload }) => invoke("api_backtest", { payload }),
  "/api/news-rag": async ({ invoke, payload }) => isMobileTauriRuntime()
    ? withTimeout(analyzeMobileStockNews(invoke, asRecord(payload)), MOBILE_NEWS_RAG_TIMEOUT_MS, `移动端消息分析超过 ${Math.round(MOBILE_NEWS_RAG_TIMEOUT_MS / 1000)} 秒未返回，已中止等待。`)
    : invoke("api_news_rag", { payload }),
  "/api/rag-pack/build": async ({ invoke, payload }) => invoke("api_rag_pack_build", { payload }),
  "/api/rag-pack/build-from-news-cache": async ({ invoke, payload }) => invoke("api_rag_pack_build_from_news_cache", { payload }),
  "/api/rag-pack/query": async ({ invoke, payload }) => invoke("api_rag_pack_query", { payload }),
  "/api/upstream-rag/build": async ({ invoke, payload }) => invoke("api_upstream_rag_build", { payload }),
  "/api/upstream-rag/transfer/start": async ({ invoke, payload }) => invoke("api_upstream_rag_transfer_start", { payload }),
  "/api/data-sources/auto-refresh-universe": async ({ invoke, payload }) => tauriAutoRefreshUniverse(invoke, asRecord(payload)),
  "/api/data-sources/refresh-universe": async ({ invoke, payload }) => refreshTauriMarketData(invoke, asRecord(payload)),
  "/api/data-sources/prune-cache": async ({ invoke }) => invoke("api_market_clear_cache"),
  "/api/upstream-rag/mobile/import": async ({ invoke, payload }) => invoke("core_upstream_rag_import", { payload }),
  "/api/upstream-rag/mobile/detail": async ({ invoke, payload }) => invoke("core_upstream_rag_detail", { payload }),
  "/api/upstream-rag/mobile/rollback": async ({ invoke, payload }) => invoke("core_upstream_rag_rollback", { payload }),
  "/api/agent": async ({ invoke, payload }) => invokeAgent(invoke, payload),
  "/api/agent/stream": async ({ invoke, payload }) => invokeAgent(invoke, payload),
};
function invokeAgent(invoke: InvokeFn, payload: unknown): Promise<unknown> {
  const request = asRecord(payload);
  return invoke("api_agent_stream", {
    payload: {
      message: String(request.message || ""),
      run_id: String(request.run_id || `react-agent-${Date.now()}`),
      mode: String(request.mode || "quick"),
    },
  });
}

function tauriRouteHandler(method: string, path: string): TauriRouteHandler | null {
  if (method === "GET") return TAURI_GET_ROUTES[path] || TAURI_GET_PREFIX_ROUTES.find((route) => path.startsWith(route.prefix))?.handler || null;
  if (method === "POST") return TAURI_POST_ROUTES[path] || null;
  return null;
}

async function requestTauriJson(method: string, url: string, payload?: unknown): Promise<{ handled: boolean; data?: unknown }> {
  const invoke = getTauriInvoke();
  if (!invoke) return { handled: false };
  const parsed = new URL(url, window.location.href);
  const normalizedMethod = String(method || "GET").toUpperCase();
  const handler = tauriRouteHandler(normalizedMethod, parsed.pathname);
  if (handler) return { handled: true, data: await handler({ invoke, parsed, path: parsed.pathname, payload }) };
  if (normalizedMethod === "GET" || normalizedMethod === "POST") throw new Error(`移动端暂不支持该接口：${parsed.pathname}`);
  return { handled: false };
}

function defaultCachePolicy(): Record<string, unknown> {
  return { mode: "light", max_bytes: 209715200, daily_days: 500, minute_days: 3 };
}

function buildAndroidNetworkOptions(): Record<string, unknown> {
  return {
    proxy_mode: localStorage.getItem("stock-optimizer-proxy-mode") || "system",
    proxy_url: localStorage.getItem("stock-optimizer-proxy-url") || "",
    android_short_sources: isMobileTauriRuntime(),
  };
}

function withAndroidNetworkOptions<T extends Record<string, unknown>>(payload: T): T & Record<string, unknown> {
  if (!isTauriRuntime()) return payload;
  return { ...payload, ...buildAndroidNetworkOptions() };
}

async function tauriAutoRefreshUniverse(invoke: InvokeFn, payload: Record<string, unknown>): Promise<Record<string, unknown>> {
  const status = await invoke<Record<string, unknown>>("api_market_status");
  const empty = asRecord(status.policy).mode === "empty" || Number(status.universe_count || 0) <= 0;
  const stale = isMarketStatusStale(status);
  if (empty || stale) {
    refreshTauriMarketData(invoke, { ...defaultCachePolicy(), ...payload }).catch((error) => console.warn("background market refresh failed", error));
  }
  return {
    source: "tencent",
    checked_at: new Date().toISOString(),
    trading_day: isLikelyTradingDay(),
    after_close: !shouldUsePreviousCloseForMobileRefresh(),
    due: empty || stale,
    refreshed: false,
    background_refresh: empty || stale,
    initial_refresh: empty,
    status,
    notes: empty
      ? ["首次安装正在后台联网生成股票池，页面可以先操作；生成完成后会自动更新状态。"]
      : stale
        ? ["检测到行情日期过期，正在后台刷新股票池。"]
        : ["已读取 Tauri/Rust 本地股票池缓存。"],
  };
}

export async function refreshTauriMarketData(invoke: InvokeFn, options: MarketRefreshOptions = {}): Promise<Record<string, unknown>> {
  const logs = createMarketRefreshLogger(options.onLog);
  const unlisten = await listenMarketRefreshLogs(logs).catch((error) => {
    logs(`无法监听 Rust 刷新日志：${(error as Error).message}`, "warn");
    return undefined;
  });
  try {
    logs("开始通过 Tauri/Rust 刷新股票池。", "info");
    const financialSnapshot = await loadMobileFinancialSnapshot().catch(() => null);
    const baseOptions = sanitizeMarketRefreshOptions(options);
    const maxLoops = clampInt(Number(baseOptions.max_refresh_loops || 512), 1, 2048, 512);
    let batchStart = clampInt(baseOptions.batch_start, 0, 100000, 0);
    const batchCount = clampInt(baseOptions.batch_count, 1, 1000, MOBILE_TENCENT_BATCHES_PER_STEP);
    let aggregate: Record<string, unknown> | null = null;
    let lastData: Record<string, unknown> | null = null;
    let financialSnapshotSent = false;

    for (let loop = 0; loop < maxLoops; loop += 1) {
      logs(`请求行情刷新批次窗口：从第 ${batchStart + 1} 批开始，每轮 ${batchCount} 批。`, "info");
      const payload = withAndroidNetworkOptions({
        ...defaultCachePolicy(),
        ...baseOptions,
        batch_start: batchStart,
        batch_count: batchCount,
        financial_snapshot: !financialSnapshotSent ? financialSnapshotPayload(financialSnapshot) : null,
        scan_candidates: true,
        use_previous_close: shouldUsePreviousCloseForMobileRefresh(),
      });
      if (payload.financial_snapshot) financialSnapshotSent = true;
      const refreshPromise = invoke<Record<string, unknown>>("api_market_refresh", { payload });
      const data = await withTimeout(
        refreshPromise,
        MOBILE_MARKET_REFRESH_INVOKE_TIMEOUT_MS,
        `Tauri/Rust 行情批次 ${batchStart + 1} 超过 ${Math.round(MOBILE_MARKET_REFRESH_INVOKE_TIMEOUT_MS / 1000)} 秒未返回，已中止等待。`,
      );
      lastData = data;
      aggregate = mergeMarketRefreshResult(aggregate, data);
      logMarketRefreshBatchResult(logs, data, aggregate);

      if (data.done || data.stopped_early || baseOptions.mode === "single") break;
      const nextStart = Number(data.next_batch_start);
      if (!Number.isFinite(nextStart) || nextStart <= batchStart) {
        aggregate = { ...(aggregate || {}), stopped_early: true, stop_reason: "stalled_batches", done: true };
        logs("批次游标没有前进，已停止刷新以避免重复请求。", "error");
        break;
      }
      batchStart = nextStart;
    }

    if (!lastData) throw new Error("Tauri/Rust 行情刷新没有返回任何批次结果。");
    if (aggregate && !aggregate.done && !aggregate.stopped_early) {
      aggregate = { ...aggregate, stopped_early: true, stop_reason: "loop_guard", done: true };
      logs("刷新循环达到安全上限，已停止并保留当前已落盘缓存。", "warn");
    }

    const result: Record<string, unknown> = { ...lastData, refresh_result: aggregate || lastData };
    const notes = mobileRefreshNotes(result, "已通过 Tauri/Rust 联网更新股票池。");
    logs(notes.join(" "), aggregate?.stopped_early ? "warn" : "success");

    let validation: Record<string, unknown> | null = null;
    if (options.validate_after_write !== false) {
      validation = await validateTauriMarketCache(invoke, logs).catch((error) => {
        logs(`刷新后校验失败：${(error as Error).message}`, "error");
        return { ok: false, error: (error as Error).message };
      });
    }

    return {
      ...result,
      validation,
      notes: uniqueNotes([...(Array.isArray(result.notes) ? result.notes as string[] : []), ...notes, ...validationNotes(validation)]),
    };
  } finally {
    unlisten?.();
  }
}

function mergeMarketRefreshResult(previous: Record<string, unknown> | null, result: Record<string, unknown>): Record<string, unknown> {
  const totalBatches = Number(result.total_batches || previous?.total_batches || 0);
  const nextBatchStart = Number(result.next_batch_start || 0);
  const failed = Number(previous?.failed_batches || 0) + Number(result.failed_batches || 0);
  const empty = Number(previous?.empty_batches || 0) + Number(result.empty_batches || 0);
  const fetched = Number(previous?.fetched || 0) + Number(result.fetched || 0);
  const processed = Number(previous?.processed_codes || 0) + Number(result.processed_codes || 0);
  const previousErrors = Array.isArray(previous?.error_samples) ? previous.error_samples : [];
  const resultErrors = Array.isArray(result.error_samples) ? result.error_samples : [];
  return {
    requested: Number(result.requested || result.total_candidates || previous?.requested || 0),
    total_candidates: Number(result.total_candidates || previous?.total_candidates || result.requested || 0),
    fetched,
    preserved: Number(result.preserved || previous?.preserved || 0),
    failed_batches: failed,
    empty_batches: empty,
    stopped_early: Boolean(result.stopped_early || previous?.stopped_early),
    stop_reason: result.stop_reason || previous?.stop_reason || null,
    batch_start: Number(result.batch_start || 0),
    batch_count: Number(result.batch_count || 0),
    next_batch_start: nextBatchStart,
    total_batches: totalBatches,
    completed_batches: Math.min(totalBatches || nextBatchStart, nextBatchStart),
    done: Boolean(result.done || result.stopped_early || (totalBatches > 0 && nextBatchStart >= totalBatches)),
    processed_codes: processed,
    error_samples: [...previousErrors, ...resultErrors].filter(Boolean).slice(-8),
  };
}

function logMarketRefreshBatchResult(
  log: (message: string, tone?: string) => void,
  result: Record<string, unknown>,
  aggregate: Record<string, unknown> | null,
): void {
  const total = Number(result.total_batches || aggregate?.total_batches || 0);
  const start = Number(result.batch_start || 0) + 1;
  const next = Number(result.next_batch_start || start);
  const fetched = Number(result.fetched || 0);
  const preserved = Number(result.preserved || 0);
  const failed = Number(result.failed_batches || 0);
  const empty = Number(result.empty_batches || 0);
  const tone = failed > 0 || empty > 0 || result.stopped_early ? "warn" : "info";
  log(`批次 ${start}-${next}/${total || "?"} 返回：新增 ${fetched} 只，保留 ${preserved} 只，空批次 ${empty}，失败 ${failed}。`, tone);
}
function createMarketRefreshLogger(onLog?: (entry: MarketRefreshLogEntry) => void) {
  return (message: string, tone = "info") => {
    if (!message) return;
    onLog?.({ time: new Date().toLocaleTimeString("zh-CN"), message, tone });
  };
}

function sanitizeMarketRefreshOptions(options: MarketRefreshOptions): Record<string, unknown> {
  const payload: Record<string, unknown> = { ...options };
  delete payload.onLog;
  if (!payload.max_candidates) payload.max_candidates = MOBILE_TENCENT_MAX_CANDIDATES;
  if (!payload.batch_count) payload.batch_count = MOBILE_TENCENT_BATCHES_PER_STEP;
  if (!payload.max_failed_batches) payload.max_failed_batches = 3;
  delete payload.validate_after_write;
  return payload;
}

export async function validateTauriMarketCache(
  invoke: InvokeFn,
  log?: (message: string, tone?: string) => void,
): Promise<Record<string, unknown>> {
  log?.("正在回读 Tauri/Rust 本地行情缓存。", "info");
  const record = asRecord(await invoke("core_mobile_market_data_read"));
  if (!record.exists) {
    const notes = Array.isArray(record.notes) ? record.notes.map(String).filter(Boolean) : [];
    throw new Error(notes[0] || "本地行情缓存不存在，请先刷新股票池。");
  }
  const data = asRecord(record.data);
  if (!Object.keys(data).length) throw new Error("本地行情缓存回传为空，无法校验。");
  const summary = asRecord(await invoke("core_validate_data_source", { payload: data }));
  const stockCount = Number(summary.stock_count || record.stock_count || 0);
  const historyCount = Number(summary.history_count || 0);
  log?.(`本地行情缓存校验通过：${stockCount} 只股票，历史序列 ${historyCount} 组，落盘 ${record.bytes || 0} B。`, "success");
  return {
    ok: true,
    stock_count: stockCount,
    history_count: historyCount,
    bytes: record.bytes || 0,
    path: record.path || "",
    generated_at: record.generated_at || null,
    summary,
  };
}

function validationNotes(validation: Record<string, unknown> | null): string[] {
  if (!validation) return [];
  if (validation.ok === false) return [`行情缓存已刷新，但回读校验失败：${validation.error || "unknown"}`];
  return [`行情缓存已落盘并通过校验：${validation.stock_count || 0} 只股票，${validation.bytes || 0} B。`];
}

async function listenMarketRefreshLogs(log: (message: string, tone?: string) => void): Promise<(() => void) | undefined> {
  const listen = getTauriListen();
  if (!listen) return undefined;
  return listen("market-refresh-log", (event) => {
    const payload = asRecord(asRecord(event).payload);
    const message = formatRustMarketRefreshLog(payload);
    if (message) log(message, String(payload.tone || "info"));
  });
}

function formatRustMarketRefreshLog(event: Record<string, unknown>): string {
  const stage = String(event.stage || "");
  const data = asRecord(event.payload);
  if (stage === "command_received") return `Rust 命令已进入：种子 ${data.seed_stock_count || 0} 只，候选上限 ${data.max_candidates || "-"}，批次 ${Number(data.batch_start || 0) + 1} 起。`;
  if (stage === "candidate_ready") return `Rust 候选池已生成：${data.candidate_count || 0} 只，本地种子 ${data.seed_candidate_count || 0} 只。`;
  if (stage === "batch_window") return `Rust 批次窗口：${Number(data.batch_start || 0) + 1}-${data.batch_end || "?"}/${data.total_batches || "?"}，单批超时 ${data.batch_timeout_seconds || "?"} 秒。`;
  if (stage === "batch_response") return `Rust 批次 ${data.batch_index || "?"} 已返回：HTTP ${data.status || "-"}，${data.byte_len || 0} B，解析 ${data.parsed_count || 0} 只，耗时 ${data.elapsed_ms || "?"} ms。`;
  if (stage === "batch_error") return `Rust 批次 ${data.batch_index || "?"} 失败：${data.error || "unknown"}。`;
  if (stage === "command_complete") return `Rust 刷新完成：新增 ${data.fetched || 0} 只，保留 ${data.preserved || 0} 只，下一批 ${data.next_batch_start || 0}/${data.total_batches || "?"}。`;
  return stage ? `Rust 阶段 ${stage}：${JSON.stringify(data)}` : "";
}

function isLikelyTradingDay(now = new Date()): boolean {
  const day = now.getDay();
  return day !== 0 && day !== 6;
}

function shouldUsePreviousCloseForMobileRefresh(now = new Date()): boolean {
  if (!isLikelyTradingDay(now)) return false;
  return now.getHours() * 60 + now.getMinutes() < 15 * 60 + 5;
}

export function isMarketStatusStale(status: unknown): boolean {
  const data = asRecord(status);
  if (asRecord(data.policy).mode === "empty") return true;
  if (Object.prototype.hasOwnProperty.call(data, "universe_count") && Number(data.universe_count || 0) <= 0) return true;
  if (data.stale !== undefined) return Boolean(data.stale);
  const quoteDate = localTradeDateCompact(data.quote_generated_at || data.generated_at || data.universe_updated_at);
  return Boolean(quoteDate && quoteDate !== expectedMarketQuoteDateCompact());
}

function mobileRefreshNotes(status: unknown, fallback = ""): string[] {
  const data = asRecord(status);
  const refresh = asRecord(data.refresh_result || data);
  const fetched = Number(refresh.fetched || 0);
  const requested = Number(refresh.requested || refresh.total_candidates || 0);
  const preserved = Number(refresh.preserved || 0);
  const failed = Number(refresh.failed_batches || 0);
  const totalBatches = Number(refresh.total_batches || 0);
  const completedBatches = Number(refresh.completed_batches || refresh.next_batch_start || 0);
  if (!Object.keys(refresh).length) return fallback ? [fallback] : [];
  const batchText = totalBatches > 0 ? `批次 ${Math.min(completedBatches, totalBatches)}/${totalBatches}` : "批次信息暂不可用";
  const base = fetched > 0
    ? `已联网更新 ${fetched} 只股票行情，保留 ${preserved} 只本地股票，候选 ${requested} 只，${batchText}。`
    : `行情暂未返回有效股票，已保留 ${preserved || Number(data.universe_count || 0)} 只本地股票，${batchText}。`;
  if (failed > 0) return [`${base} 失败批次 ${failed} 批，其余股票已继续处理。`];
  return [base];
}
async function loadMobileFinancialSnapshot(): Promise<Record<string, unknown> | null> {
  if (mobileFinancialSnapshotPromise) return mobileFinancialSnapshotPromise;
  mobileFinancialSnapshotPromise = (async () => {
    try {
      const response = await fetch(MOBILE_FINANCIAL_SNAPSHOT_URL, { cache: "no-store" });
      if (!response.ok) return null;
      const snapshot = asRecord(await response.json());
      const financials: Record<string, unknown> = {};
      const stocks = (Array.isArray(snapshot.stocks) ? snapshot.stocks : [])
        .map((stock) => normalizeMobileFinancialSnapshotStock(asRecord(stock)))
        .filter(Boolean) as Record<string, unknown>[];
      for (const stock of stocks) {
        const code = String(stock.code || "");
        const financial = normalizeMobileFinancialSnapshotFinancial(stock);
        if (code && financial) financials[code] = financial;
      }
      for (const [rawCode, value] of Object.entries(asRecord(snapshot.financials))) {
        const code = normalizeStockCode(rawCode);
        const financial = normalizeMobileFinancialSnapshotFinancial(asRecord(value));
        if (code && financial) financials[code] = financial;
      }
      return { ...snapshot, stocks, financials };
    } catch {
      return null;
    }
  })();
  return mobileFinancialSnapshotPromise;
}

function financialSnapshotPayload(snapshot: Record<string, unknown> | null): Record<string, unknown> | null {
  if (!snapshot) return null;
  const stocks = Array.isArray(snapshot.stocks) ? snapshot.stocks : [];
  const financials = asRecord(snapshot.financials);
  if (!stocks.length && !Object.keys(financials).length) return null;
  return { stocks, financials };
}

async function loadMobileFinancialSnapshotForCode(rawCode: string): Promise<Record<string, unknown> | null> {
  const code = normalizeStockCode(rawCode);
  if (!code) return null;
  const snapshot = await loadMobileFinancialSnapshot();
  const financial = asRecord(asRecord(snapshot?.financials)[code]);
  if (!Object.keys(financial).length) return null;
  return { stocks: [], financials: { [code]: financial } };
}

function normalizeMobileFinancialSnapshotStock(stock: Record<string, unknown>): Record<string, unknown> | null {
  const code = normalizeStockCode(stock.code);
  if (!code) return null;
  const item: Record<string, unknown> = { code };
  for (const field of MOBILE_DEDUCTED_FINANCIAL_FIELDS) {
    const value = parseLooseNumber(stock[field]);
    if (value !== null) item[field] = value;
  }
  const financial = normalizeMobileFinancialSnapshotFinancial(stock);
  if (financial) item.financial = financial;
  return item;
}

function normalizeMobileFinancialSnapshotFinancial(source: Record<string, unknown>): Record<string, unknown> | null {
  const financial: Record<string, unknown> = {};
  const latestEps = parseLooseNumber(source.latest_eps ?? source.eps);
  if (latestEps !== null) financial.latest_eps = latestEps;
  const latestBps = parseLooseNumber(source.latest_bps ?? source.bps);
  if (latestBps !== null) financial.latest_bps = latestBps;
  const period = normalizeFinancialPeriodKey(source.period || source.latest_period || source.latest_eps_period);
  if (period) financial.period = period;
  if (source.source) financial.source = String(source.source);
  const quarterly = (Array.isArray(source.quarterly_eps) ? source.quarterly_eps : [])
    .map((point) => normalizeQuarterlyEpsPoint(asRecord(point)))
    .filter(Boolean) as Record<string, unknown>[];
  if (quarterly.length) {
    financial.quarterly_eps = quarterly;
    if (!financial.period) financial.period = quarterly[0].period;
  }
  return Object.keys(financial).length ? financial : null;
}

function normalizeQuarterlyEpsPoint(point: Record<string, unknown>): Record<string, unknown> | null {
  const period = normalizeFinancialPeriodKey(point.period);
  const value = parseLooseNumber(point.value ?? point.raw_value);
  if (!period || value === null) return null;
  return { period, value, source: point.source ? String(point.source) : "mobile financial snapshot" };
}

function normalizeFinancialPeriodKey(value: unknown): string {
  const raw = String(value || "").trim();
  const quarter = raw.match(/(20\d{2})\s*Q\s*([1-4])/i);
  if (quarter) return `${quarter[1]}Q${quarter[2]}`;
  const date = raw.match(/(20\d{2})[-/.年\s]*(0?[1-9]|1[0-2])/);
  if (!date) return "";
  const month = Number(date[2]);
  const q = month <= 3 ? 1 : month <= 6 ? 2 : month <= 9 ? 3 : 4;
  return `${date[1]}Q${q}`;
}

export async function fetchObserveDailyHistoryForTauri(payload: Record<string, unknown>, timeoutMs: number): Promise<Record<string, unknown>[] | null> {
  const symbol = observeTencentDailySymbol(String(payload.code || ""));
  if (!symbol) return null;
  const start = normalizeDateParam(payload.start_date, "20200101");
  const end = normalizeDateParam(payload.end_date, currentSystemDateCompact());
  const param = `${symbol},day,${start},${end},${OBSERVE_HISTORY_LIMIT},`;
  const url = `https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param=${encodeURIComponent(param)}`;
  const response = await fetchWithTimeout(url, { cache: "no-store" }, timeoutMs, `WebView Tencent 日线超过 ${Math.round(timeoutMs / 1000)} 秒未返回。`);
  if (!response.ok) throw new Error(`WebView Tencent 日线 HTTP ${response.status}`);
  const data = asRecord(await response.json());
  const quote = asRecord(asRecord(data.data)[symbol]);
  const rows = quote.day || quote.qfqday || [];
  return (Array.isArray(rows) ? rows : []).map(observeTencentDailyRowToHistory).filter(Boolean) as Record<string, unknown>[];
}

function observeTencentDailySymbol(rawCode: string): string {
  const code = normalizeStockCode(rawCode);
  if (!code) return "";
  const [digits, market] = code.split(".");
  return `${String(market || "").toLowerCase()}${digits}`;
}

function observeTencentDailyRowToHistory(row: unknown): Record<string, unknown> | null {
  if (!Array.isArray(row) || row.length < 6) return null;
  const close = parseLooseNumber(row[2]);
  if (close === null) return null;
  return {
    date: String(row[0] || ""),
    open: parseLooseNumber(row[1]) ?? close,
    close,
    high: parseLooseNumber(row[3]) ?? close,
    low: parseLooseNumber(row[4]) ?? close,
    volume: parseLooseNumber(row[5]),
  };
}

async function analyzeMobileStockNews(invoke: InvokeFn, payload: Record<string, unknown>): Promise<Record<string, unknown>> {
  const mobilePayload = withAndroidNetworkOptions({
    ...payload,
    mobile_fast: true,
    include_us_market_brief: false,
    max_items: Math.max(10, Number(payload.max_items || 24)),
  });
  const cached = await invoke<Record<string, unknown>>("api_news_rag", { payload: mobilePayload });
  if (Number(cached.message_count || 0) > 0 || (Array.isArray(cached.findings) && cached.findings.length)) {
    return appendResultNotes(cached, ["移动端已使用安卓短链路或本机缓存完成分析；如源站均失败，可导入桌面端同步包补充。"]);
  }
  try {
    const offline = await analyzeMobileNewsRag(invoke, payload);
    return appendResultNotes(offline, ["移动端本机消息缓存暂无可用内容，已改用手机端已导入的上下游 RAG 包。"]);
  } catch (offlineError) {
    const cachedNotes = Array.isArray(cached.notes) ? cached.notes.join(" ") : "";
    throw new Error(`${cachedNotes || "安卓在线短链路和本机缓存暂无可用内容。"} ${(offlineError as Error).message}`);
  }
}

async function analyzeMobileNewsRag(invoke: InvokeFn, payload: Record<string, unknown>): Promise<Record<string, unknown>> {
  const seedCodes = Array.isArray(payload.seed_codes) ? payload.seed_codes : [];
  const requestedCode = normalizeStockCode(payload.code || seedCodes[0] || "");
  const detail = asRecord(await invoke("core_upstream_rag_detail", { payload: { stock_code: requestedCode, pack_version: "" } }));
  const manifest = asRecord(detail.manifest || detail);
  const manifestCode = normalizeStockCode(manifest.target_stock_code || "");
  if (requestedCode && manifestCode && stockCodeDigits(requestedCode) !== stockCodeDigits(manifestCode)) {
    throw new Error(`当前手机 RAG 包是 ${manifestCode}，不是 ${requestedCode}。请先导入目标股票的同步包。`);
  }
  const sources = mobileNewsRagSourcesFromManifest(manifest, Number(payload.max_items || 24));
  const skill = asRecord(await invoke("core_mobile_stock_skill", {
    payload: { stock_code: manifestCode || requestedCode, stock_name: manifest.target_stock_name || "", question: "分析上下游消息利好利空", sources },
  }));
  return mobileNewsSkillToNewsRagResult(skill, manifest, sources, Array.isArray(detail.notes) ? detail.notes as string[] : []);
}
function mobileNewsRagSourcesFromManifest(manifest: Record<string, unknown>, maxItems = 24): Record<string, unknown>[] {
  const sources: Record<string, unknown>[] = [];
  for (const edge of arrayRecords(manifest.relation_edges)) {
    const evidence = String(edge.evidence_text || edge.source_ref || "").trim();
    if (!evidence) continue;
    sources.push({
      title: mobileRelationSourceTitle(edge),
      summary: evidence,
      source_tier: edge.source_tier || "manual_url",
      source_name: edge.source_name || "RAG relation",
      published_at: edge.published_at || null,
      source_url: edge.source_url || null,
      evidence,
    });
  }
  for (const chunk of arrayRecords(manifest.evidence_chunks)) {
    const evidence = String(chunk.evidence_text || chunk.text || chunk.title || "").trim();
    if (!evidence) continue;
    sources.push({
      title: chunk.title || "RAG evidence chunk",
      summary: evidence,
      source_tier: chunk.source_tier || "manual_url",
      source_name: chunk.source_name || "RAG evidence",
      published_at: chunk.published_at || null,
      source_url: chunk.source_url || null,
      evidence,
    });
  }
  const seen = new Set<string>();
  return sources.filter((source) => {
    const key = [source.title, source.source_name, source.evidence].join("\n");
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  }).slice(0, clampInt(maxItems, 1, 80, 24));
}

function mobileRelationSourceTitle(edge: Record<string, unknown>): string {
  const sourceEntity = asRecord(edge.source_entity);
  const targetEntity = asRecord(edge.target_entity);
  const source = sourceEntity.entity_name || sourceEntity.stock_code || edge.source_code || "upstream";
  const target = targetEntity.entity_name || targetEntity.stock_code || edge.target_code || "target";
  return `${source} ${relationTypeLabel(edge.relation_type)} ${target}: ${relationStatusLabel(edge.status)}`;
}

function mobileNewsSkillToNewsRagResult(skill: Record<string, unknown>, manifest: Record<string, unknown>, sources: Record<string, unknown>[], detailNotes: string[]): Record<string, unknown> {
  const overview = asRecord(skill.overview);
  const code = normalizeStockCode(overview.stock_code || manifest.target_stock_code || "");
  const name = String(overview.stock_name || manifest.target_stock_name || "");
  const target = `${name || code || "目标股票"}${code ? ` (${code})` : ""}`;
  const findings = [
    ...mobileSkillFindingsToNewsFindings(skill.positive_factors, "positive", target, code),
    ...mobileSkillFindingsToNewsFindings(skill.negative_factors, "negative", target, code),
    ...mobileSkillFindingsToNewsFindings(skill.neutral_information, "mixed", target, code),
    ...mobileSkillFindingsToNewsFindings(skill.unverified_leads, "uncertain", target, code),
  ];
  return splitResultNotes({
    scope_codes: code ? [code] : [],
    relation_count: manifest.relation_edge_count ?? arrayRecords(manifest.relation_edges).length,
    message_count: sources.length,
    findings,
    notes: uniqueCompactStrings([
      String(overview.summary || ""),
      "移动端使用已导入本机 RAG 包离线分析，不会在手机端抓取公告或新闻。",
      sources.length ? "" : "当前 RAG 包没有可分析证据，请在桌面端重建包含证据片段的同步包。",
      ...detailNotes,
    ]),
  });
}

function mobileSkillFindingsToNewsFindings(items: unknown, direction: string, target: string, code: string): Record<string, unknown>[] {
  return arrayRecords(items).map((item) => ({
    target,
    direction,
    confidence: confidenceLabel(item.confidence),
    impact_chain: item.summary || item.risk_note || item.title || "",
    evidence: [{
      title: item.title || "-",
      source: item.source_name || "local rag",
      source_tier: item.source_tier || "manual_url",
      published_at: item.published_at || null,
      url: item.source_url || null,
      stock_codes: code ? [code] : [],
      relation_types: [],
      sentiment: direction,
    }],
    pending_checks: uniqueCompactStrings([String(item.risk_note || "")]),
  }));
}

function confidenceLabel(value: unknown): string {
  const score = Number(value);
  if (!Number.isFinite(score)) return "low";
  if (score >= 0.75) return "high";
  if (score >= 0.5) return "medium";
  return "low";
}

function appendResultNotes<T extends Record<string, unknown>>(result: T, notes: string[]): T {
  return splitResultNotes({ ...result, notes: uniqueNotes([...(Array.isArray(result.notes) ? result.notes as string[] : []), ...notes]) });
}

function splitResultNotes<T extends Record<string, unknown>>(result: T): T {
  const notes = Array.isArray(result.notes) ? result.notes.map(String).filter(Boolean) : [];
  const debugNotes = Array.isArray(result.debug_notes) ? result.debug_notes.map(String).filter(Boolean) : [];
  const visible: string[] = [];
  const debug = [...debugNotes];
  for (const note of notes) {
    if (isDebugNote(note)) debug.push(note);
    else visible.push(note);
  }
  return {
    ...result,
    notes: uniqueNotes(visible),
    debug_notes: uniqueNotes(debug),
  };
}

function isDebugNote(note: string): boolean {
  return [
    /^Android /i,
    /^RAG 只在/,
    /^当前范围没有供应链/,
    /^当前为个股消息模式/,
    /^上下游 RAG 请使用/,
    /^未接入模型/,
    /^没有可分析目标/,
    /^消息缓存:/,
    /news-cache\.json/i,
    /GP_NEWS_ENABLE_TRADITIONAL_MEDIA/,
  ].some((pattern) => pattern.test(note));
}

export function createTimeoutSignal(timeoutMs?: number, message?: string): { signal: AbortSignal | null; cancel: () => void } {
  if (!Number.isFinite(timeoutMs) || (timeoutMs ?? 0) <= 0 || typeof AbortController === "undefined") return { signal: null, cancel: () => {} };
  const controller = new AbortController();
  const timer = window.setTimeout(() => {
    controller.abort(createRequestAbortError(message || `请求超过 ${Math.round((timeoutMs as number) / 1000)} 秒未返回`, "TimeoutError"));
  }, timeoutMs as number);
  return { signal: controller.signal, cancel: () => window.clearTimeout(timer) };
}

export function combineAbortSignals(...signals: (AbortSignal | null | undefined)[]): AbortSignal | null {
  const activeSignals = signals.filter(Boolean) as AbortSignal[];
  if (!activeSignals.length || typeof AbortController === "undefined") return activeSignals[0] || null;
  if (activeSignals.length === 1) return activeSignals[0];
  const controller = new AbortController();
  const abort = (event: { target: AbortSignal }) => {
    if (!controller.signal.aborted) controller.abort(abortReason(event.target));
  };
  activeSignals.forEach((signal) => {
    if (signal.aborted) abort({ target: signal });
    else signal.addEventListener("abort", () => abort({ target: signal }), { once: true });
  });
  return controller.signal;
}

export function abortReason(signal: AbortSignal | null | undefined): Error {
  if (!signal) return new Error("请求已取消");
  const reason = (signal as AbortSignal & { reason?: unknown }).reason;
  if (reason instanceof Error) return reason;
  return createRequestAbortError("请求已取消", "AbortError");
}

export function createRequestAbortError(message: string, name = "AbortError"): Error {
  try {
    return new DOMException(message, name);
  } catch {
    const error = new Error(message);
    error.name = name;
    return error;
  }
}

function withAbortSignal<T>(promise: Promise<T>, signal: AbortSignal | null): Promise<T> {
  if (!signal) return promise;
  if (signal.aborted) return Promise.reject(abortReason(signal));
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => signal.addEventListener("abort", () => reject(abortReason(signal)), { once: true })),
  ]);
}

export async function requestJson<T = unknown>(method: string, url: string, payload?: unknown, headers: Record<string, string> = {}, options: RequestOptions = {}): Promise<T> {
  const timeoutSignal = createTimeoutSignal(options.timeoutMs);
  const signal = combineAbortSignals(options.signal ?? null, timeoutSignal.signal);
  try {
    const tauriResult = await withAbortSignal(requestTauriJson(method, url, payload), signal);
    if (tauriResult.handled) return tauriResult.data as T;
    const request: RequestInit = { method, headers: method === "POST" ? { "Content-Type": "application/json", ...headers } : headers };
    if (signal) request.signal = signal;
    if (payload !== undefined) request.body = JSON.stringify(payload);
    const resp = await fetch(url, request);
    if (!resp.ok) throw new Error(await resp.text() || `HTTP ${resp.status}`);
    return (await resp.json()) as T;
  } finally {
    timeoutSignal.cancel();
  }
}

export async function postJson<T = unknown>(url: string, payload?: unknown, options?: RequestOptions): Promise<T> {
  return requestJson<T>("POST", url, payload, {}, options || {});
}

export async function getJson<T = unknown>(url: string, options?: RequestOptions): Promise<T> {
  return requestJson<T>("GET", url, undefined, {}, options || {});
}

export async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  const { signal, cancel } = createTimeoutSignal(timeoutMs, message);
  try {
    return await withAbortSignal(promise, signal);
  } finally {
    cancel();
  }
}

async function fetchWithTimeout(input: RequestInfo | URL, init: RequestInit, timeoutMs: number, message: string): Promise<Response> {
  const { signal, cancel } = createTimeoutSignal(timeoutMs, message);
  try {
    return await fetch(input, { ...init, signal: signal || undefined });
  } finally {
    cancel();
  }
}

export async function openExternalUrl(rawUrl: string): Promise<void> {
  const url = normalizeExternalUrl(rawUrl);
  if (!url) return;
  const invoke = getTauriInvoke();
  if (invoke) {
    try {
      await invoke("open_external_url", { url });
      return;
    } catch (error) {
      console.warn("open_external_url failed, falling back to window.open", error);
    }
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

export function normalizeExternalUrl(rawUrl: string): string {
  try {
    const url = new URL(String(rawUrl || "").trim(), window.location.href);
    return ["http:", "https:"].includes(url.protocol) ? url.toString() : "";
  } catch {
    return "";
  }
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function arrayRecords(value: unknown): Record<string, unknown>[] {
  return Array.isArray(value) ? value.map(asRecord).filter((item) => Object.keys(item).length) : [];
}
