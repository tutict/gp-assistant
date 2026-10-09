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
  const text = error instanceof Error ? error.message : String(error);
  try {
    return JSON.parse(text)?.code === "LOCAL_DATA_MISSING";
  } catch {
    return false;
  }
}
export function formatLocalDataError(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  try {
    const parsed = JSON.parse(text);
    if (parsed.code === "LOCAL_DATA_MISSING") {
      const details = (Array.isArray(parsed.missing) ? parsed.missing : []).slice(0, 5).map((item: Record<string, unknown>) =>
        [item.code, item.kind, item.required_bars != null ? "日线 " + (item.available_bars ?? 0) + "/" + item.required_bars : item.reason, item.start_date && item.end_date ? item.start_date + "–" + item.end_date : ""].filter(Boolean).join(" · ")
      ).join("；");
      return "本地数据不足（" + (parsed.missing_count ?? 0) + " 项）：" + details + "。请显式联网刷新数据后重试；不会自动联网或切换算法。";
    }
    if (parsed.code === "REFRESH_REQUIRED") return "冷启动发布验证必须显式使用联网刷新模式（data_policy=refresh）。";
    if (parsed.code === "INVALID_DATA_POLICY") return "数据模式无效：仅支持本地缓存或显式联网刷新。";
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
