// Tauri runtime bridge — handles API routing between web and Tauri (Android/desktop) runtimes

declare global {
  interface Window {
    __TAURI__?: {
      core?: {
        invoke: <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>;
      };
      event?: {
        listen: (event: string, handler: (event: unknown) => void) => Promise<() => void>;
      };
    };
  }
}

export function isTauriRuntime(): boolean {
  return Boolean(window.__TAURI__?.core?.invoke);
}

export function isMobileTauriRuntime(): boolean {
  return isTauriRuntime() && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent || "");
}

export function getTauriInvoke() {
  return window.__TAURI__?.core?.invoke;
}

// --- Tauri route definitions ---

type TauriRouteHandler = (ctx: {
  invoke: NonNullable<ReturnType<typeof getTauriInvoke>>;
  parsed: URL;
  path: string;
  payload?: unknown;
}) => Promise<unknown>;

const STOCK_SEARCH_LIMIT = 5;

export const TAURI_GET_ROUTES: Record<string, TauriRouteHandler> = {
  "/health": async ({ invoke }) => invoke("api_health"),
  "/api/strategies": async ({ invoke }) => invoke("api_strategies"),
  "/api/data-sources": async ({ invoke }) => invoke("api_data_sources"),
  "/api/data-sources/status": async ({ invoke }) => invoke("api_market_status"),
  "/api/rag-pack/status": async ({ invoke }) => invoke("api_rag_pack_status"),
  "/api/upstream-rag/mobile/list": async ({ invoke }) => invoke("core_upstream_rag_list"),
  "/api/upstream-rag/status": async ({ invoke }) => invoke("api_upstream_rag_status"),
  "/api/upstream-rag/mobile/detail": async ({ invoke, parsed }) =>
    invoke("core_upstream_rag_detail", {
      payload: {
        stock_code: parsed.searchParams.get("stock_code") || "",
        pack_version: parsed.searchParams.get("pack_version") || "",
      },
    }),
  "/api/stock-search": async ({ invoke, parsed }) =>
    invoke("api_stock_search", {
      payload: {
        q: parsed.searchParams.get("q") || "",
        limit: parsed.searchParams.get("limit") || STOCK_SEARCH_LIMIT,
      },
    }),
};

interface TauriPrefixRoute {
  prefix: string;
  handler: TauriRouteHandler;
}

export const TAURI_GET_PREFIX_ROUTES: TauriPrefixRoute[] = [
  {
    prefix: "/api/stocks/",
    handler: async ({ invoke, path }) => {
      const code = decodeURIComponent(path.slice("/api/stocks/".length));
      return invoke("api_stock_get", { payload: { code } });
    },
  },
  {
    prefix: "/api/minutes/",
    handler: async ({ invoke, path, parsed }) => {
      const code = decodeURIComponent(path.slice("/api/minutes/".length));
      return invoke("api_minutes", {
        payload: {
          code,
          start: parsed.searchParams.get("start") || "",
          end: parsed.searchParams.get("end") || "",
          period: parsed.searchParams.get("period") || "1",
          limit: parsed.searchParams.get("limit") || 500,
        },
      });
    },
  },
  {
    prefix: "/api/order-book/",
    handler: async ({ invoke, path }) => {
      const code = decodeURIComponent(path.slice("/api/order-book/".length));
      return invoke("api_order_book", { payload: { code } });
    },
  },
  {
    prefix: "/api/observe/",
    handler: async ({ invoke, path, parsed }) => {
      const code = decodeURIComponent(path.slice("/api/observe/".length));
      return invoke("api_observe", {
        payload: {
          code,
          start_date: parsed.searchParams.get("start_date") || "20200101",
          end_date: parsed.searchParams.get("end_date") || "",
          series_limit: parsed.searchParams.get("series_limit") || 120,
          include_order_book: parsed.searchParams.get("include_order_book") === "true",
          mobile_fast_observe: isMobileTauriRuntime(),
        },
      });
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
  "/api/news-rag": async ({ invoke, payload }) => invoke("api_news_rag", { payload }),
  "/api/rag-pack/build": async ({ invoke, payload }) => invoke("api_rag_pack_build", { payload }),
  "/api/rag-pack/build-from-news-cache": async ({ invoke, payload }) => invoke("api_rag_pack_build_from_news_cache", { payload }),
  "/api/rag-pack/query": async ({ invoke, payload }) => invoke("api_rag_pack_query", { payload }),
  "/api/upstream-rag/build": async ({ invoke, payload }) => invoke("api_upstream_rag_build", { payload }),
  "/api/upstream-rag/transfer/start": async ({ invoke, payload }) => invoke("api_upstream_rag_transfer_start", { payload }),
  "/api/data-sources/auto-refresh-universe": async ({ invoke, payload }) => invoke("api_market_refresh", { payload: payload || defaultCachePolicy() }),
  "/api/data-sources/refresh-universe": async ({ invoke, payload }) => invoke("api_market_refresh", { payload: payload || defaultCachePolicy() }),
  "/api/data-sources/prune-cache": async ({ invoke }) => invoke("api_market_clear_cache"),
  "/api/upstream-rag/mobile/import": async ({ invoke, payload }) => invoke("core_upstream_rag_import", { payload }),
  "/api/upstream-rag/mobile/detail": async ({ invoke, payload }) => invoke("core_upstream_rag_detail", { payload }),
  "/api/upstream-rag/mobile/rollback": async ({ invoke, payload }) => invoke("core_upstream_rag_rollback", { payload }),
  "/api/agent": async ({ invoke, payload }) => {
    const request = payload && typeof payload === "object" ? (payload as Record<string, unknown>) : {};
    return invoke("api_agent_stream", {
      payload: {
        message: String(request.message || ""),
        run_id: String(request.run_id || `react-agent-${Date.now()}`),
      },
    });
  },
  "/api/agent/stream": async ({ invoke, payload }) => {
    const request = payload && typeof payload === "object" ? (payload as Record<string, unknown>) : {};
    return invoke("api_agent_stream", {
      payload: {
        message: String(request.message || ""),
        run_id: String(request.run_id || `react-agent-${Date.now()}`),
      },
    });
  },
};

function defaultCachePolicy(): Record<string, unknown> {
  return {
    mode: "light",
    max_bytes: 209715200,
    daily_days: 500,
    minute_days: 3,
  };
}

function tauriRouteHandler(method: string, path: string): TauriRouteHandler | null {
  if (method === "GET") {
    return (
      TAURI_GET_ROUTES[path] ||
      TAURI_GET_PREFIX_ROUTES.find((route) => path.startsWith(route.prefix))?.handler ||
      null
    );
  }
  if (method === "POST") return TAURI_POST_ROUTES[path] || null;
  return null;
}

async function requestTauriJson(method: string, url: string, payload?: unknown): Promise<{ handled: boolean; data?: unknown }> {
  const invoke = getTauriInvoke();
  if (!invoke) return { handled: false };

  const normalizedMethod = String(method || "GET").toUpperCase();
  const parsed = new URL(url, window.location.href);
  const path = parsed.pathname;
  const handler = tauriRouteHandler(normalizedMethod, path);

  if (handler) return { handled: true, data: await handler({ invoke, parsed, path, payload }) };
  if (normalizedMethod === "GET" || normalizedMethod === "POST") {
    throw new Error(`移动端暂不支持该接口：${path}`);
  }
  return { handled: false };
}

// --- Abort signal utilities ---

export function createTimeoutSignal(timeoutMs?: number, message?: string): { signal: AbortSignal | null; cancel: () => void } {
  if (!Number.isFinite(timeoutMs) || (timeoutMs ?? 0) <= 0 || typeof AbortController === "undefined") {
    return { signal: null, cancel: () => {} };
  }
  const controller = new AbortController();
  const timer = window.setTimeout(() => {
    controller.abort(createRequestAbortError(message || `请求超过 ${Math.round((timeoutMs as number) / 1000)} 秒未返回`, "TimeoutError"));
  }, timeoutMs as number);
  return {
    signal: controller.signal,
    cancel: () => window.clearTimeout(timer),
  };
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
    new Promise<T>((_, reject) => {
      signal.addEventListener("abort", () => reject(abortReason(signal)), { once: true });
    }),
  ]);
}

// --- HTTP request ---

export interface RequestOptions {
  timeoutMs?: number;
  signal?: AbortSignal | null;
  headers?: Record<string, string>;
}

export async function requestJson<T = unknown>(
  method: string,
  url: string,
  payload?: unknown,
  headers: Record<string, string> = {},
  options: RequestOptions = {},
): Promise<T> {
  const timeoutSignal = createTimeoutSignal(options.timeoutMs);
  const signal = combineAbortSignals(options.signal ?? null, timeoutSignal.signal);
  try {
    const tauriResult = await withAbortSignal(requestTauriJson(method, url, payload), signal);
    if (tauriResult.handled) return tauriResult.data as T;

    const request: RequestInit = {
      method,
      headers: method === "POST" ? { "Content-Type": "application/json", ...headers } : headers,
    };
    if (signal) request.signal = signal;
    if (payload !== undefined) request.body = JSON.stringify(payload);

    const resp = await fetch(url, request);
    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(text || `HTTP ${resp.status}`);
    }
    return (await resp.json()) as T;
  } finally {
    timeoutSignal.cancel();
  }
}

export async function postJson<T = unknown>(url: string, payload?: unknown): Promise<T> {
  return requestJson<T>("POST", url, payload);
}

export async function getJson<T = unknown>(url: string): Promise<T> {
  return requestJson<T>("GET", url);
}

export async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  const { signal, cancel } = createTimeoutSignal(timeoutMs, message);
  try {
    return await withAbortSignal(promise, signal);
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
