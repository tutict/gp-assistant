import type { DataStatus } from "../types";
/** Additive metadata: never replace missing data with a successful-looking result. */
export type DataPolicy = "cache_only" | "refresh";
export type DataPolicyPreference = "auto" | "cache_only" | "refresh";
export function resolveLocalDataPolicy(preference: DataPolicyPreference, unavailable: boolean, recommended: boolean): DataPolicy {
  if (preference === "refresh") return "refresh";
  if (preference === "cache_only") return "cache_only";
  return unavailable || recommended ? "refresh" : "cache_only";
}

export function localDataUnavailable(status: DataStatus | null | undefined): boolean {
  if (status === undefined) return false;
  if (status === null) return true;
  const count = Number(status.universe_count);
  return !Number.isFinite(count) || count <= 0 || status.policy?.mode === "empty";
}

export function shouldSuggestLocalRefresh(error: unknown): boolean {
  const seen = new Set<unknown>();
  let current: unknown = error;
  for (let depth = 0; depth < 4 && current != null; depth += 1) {
    if (typeof current === "object" && seen.has(current)) return false;
    if (typeof current === "object" && current !== null) seen.add(current);
    const text = current instanceof Error ? current.message : String(current);
    try {
      if (JSON.parse(text)?.code === "LOCAL_DATA_MISSING") return true;
    } catch {
      // Try the nested cause below. Tauri wraps native errors with a formatted Error.
    }
    current = current instanceof Error
      ? current.cause
      : (typeof current === "object" && current !== null && "cause" in current
        ? (current as { cause?: unknown }).cause
        : undefined);
  }
  return false;
}
export function formatLocalDataError(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  try {
    const parsed = JSON.parse(text);
    if (parsed.code === "LOCAL_DATA_MISSING") {
      const missing = Array.isArray(parsed.missing) ? parsed.missing as Record<string, unknown>[] : [];
      if (String(parsed.operation || "").startsWith("hk_") && missing.some((item) => item.kind === "hk_quotes")) {
        return "港股行情缓存为空。请在「选股」页点击港股目录上的“刷新目录与行情”，批量获取可用 HKD 行情后重试。";
      }
      const date = (value: unknown) => String(value ?? "未知").replace(/^(\d{4})(\d{2})(\d{2})$/, "$1-$2-$3");
      if (String(parsed.operation || "").startsWith("hk_") && missing.some(item => item.kind === "history")) {
        const details = missing.slice(0, 5).map(item => [item.code,
          item.reason || "所选港股历史区间尚未补齐",
          item.as_of ? `日线截至 ${date(item.as_of)}` : "暂无日线",
          item.start_date && item.end_date ? `${date(item.start_date)} 至 ${date(item.end_date)}` : "",
        ].filter(Boolean).join(" · ")).join("；");
        return `港股日线准备未完成（${parsed.missing_count ?? missing.length} 项）：${details}。请在本页重新运行以补齐日线；区间过长时可缩短日期。`;
      }
      const details = missing.slice(0, 5).map((item: Record<string, unknown>) => {
        const history = item.history_behind_quote === true
          ? `日线截至 ${date(item.as_of)}，行情截至 ${date(item.quote_as_of)}，日期落后`
          : item.required_bars != null ? "日线 " + (item.available_bars ?? 0) + "/" + item.required_bars : item.reason;
        return [item.code, item.kind === "history" ? "日线" : item.kind, history,
          item.start_date && item.end_date ? item.start_date + "–" + item.end_date : ""].filter(Boolean).join(" · ");
      }).join("；");
      return "本地数据不足（" + (parsed.missing_count ?? 0) + " 项）：" + details + "。请先点击顶部「刷新」补齐行情，再重试。";
    }
    if (parsed.code === "REFRESH_REQUIRED") return "冷启动发布验证必须显式使用联网刷新模式（data_policy=refresh）。";
    if (parsed.code === "INVALID_DATA_POLICY") return "数据模式无效：请先点击顶部「刷新」准备行情。";
  } catch { /* Preserve non-JSON errors. */ }
  return text;
}
export function localDataSummary(result: unknown): string | null {
  if (!result || typeof result !== "object" || !("data_metadata" in result)) return null;
  const meta = result.data_metadata as Record<string, unknown> | null;
  if (!meta) return null;
  const coverage = Array.isArray(meta.coverage) ? meta.coverage as Record<string, unknown>[] : [];
  const ranges = coverage.slice(0, 3).map(item => String(item.code ?? item.kind) + "：" + (item.available_bars ?? "?") + "/" + (item.required_bars ?? "?") + " 条").join("，");
  return [meta.data_policy === "refresh" ? "显式联网刷新 / 本地数据" : "本地缓存（未联网）", "行情截至 " + (meta.quote_as_of || "未知"), "历史截至 " + (meta.history_as_of || "未使用"), ranges, meta.coverage_count ? "覆盖检查 " + meta.coverage_count + " 项" : "", "缓存日期不代表实时行情"].filter(Boolean).join(" · ");
}
