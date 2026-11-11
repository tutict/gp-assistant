import { describe, expect, it } from "vitest";
import { formatLocalDataError, shouldSuggestLocalRefresh, localDataUnavailable, resolveLocalDataPolicy } from "./localData";
describe("local-first action state", () => {
  it("explains an HK range or trading-date failure even when its row count is sufficient", () => {
    const message = formatLocalDataError(JSON.stringify({ code:"LOCAL_DATA_MISSING", operation:"hk_backtest", missing_count:1,
      missing:[{code:"02555.HK",kind:"history",required_bars:2,available_bars:608,as_of:"20261012",start_date:"20200101",end_date:"20261108",reason:"日线落后于本机港股行情"}]
    }));
    expect(message).toContain("02555.HK");
    expect(message).toContain("日线落后于本机港股行情");
    expect(message).toContain("2026-10-12");
    expect(message).toContain("重新运行");
    expect(message).not.toContain("608/2");
    expect(message).not.toContain("顶部「刷新」");
  });
  it("explains a stale daily-history date instead of claiming its sufficient row count is missing", () => {
    const message = formatLocalDataError(JSON.stringify({ code: "LOCAL_DATA_MISSING", missing_count: 1,
      missing: [{ code: "688279.SH", kind: "history", required_bars: 45, available_bars: 89,
        as_of: "20260923", quote_as_of: "20261030", history_behind_quote: true }],
    }));
    expect(message).toContain("日线截至 2026-09-23");
    expect(message).toContain("行情截至 2026-10-30");
    expect(message).toContain("日期落后");
    expect(message).not.toContain("89/45");
  });
  it("directs HK screening to the directory quote batch when HKD quotes are missing", () => {
    const message = formatLocalDataError(JSON.stringify({
      code: "LOCAL_DATA_MISSING",
      operation: "hk_screen",
      missing_count: 1,
      missing: [{ kind: "hk_quotes", reason: "No HKD-listed equity quotes are cached yet." }],
    }));
    expect(message).toContain("港股目录");
    expect(message).toContain("刷新目录与行情");
    expect(message).toContain("批量获取");
    expect(message).not.toContain("顶部「刷新」");
  });
  it("treats an empty or missing market cache as refresh-required", () => {
    expect(localDataUnavailable(null)).toBe(true);
    expect(localDataUnavailable({ universe_count: 0 })).toBe(true);
    expect(localDataUnavailable({ universe_count: 120, stale: true })).toBe(false);
  });
  it("suggests refresh only for structured local-data misses", () => {
    expect(shouldSuggestLocalRefresh(JSON.stringify({ code: "LOCAL_DATA_MISSING" }))).toBe(true);
    expect(shouldSuggestLocalRefresh(new Error("本地数据不足", { cause: JSON.stringify({ code: "LOCAL_DATA_MISSING" }) }))).toBe(true);
    expect(shouldSuggestLocalRefresh(new Error("网络请求失败"))).toBe(false);
    expect(shouldSuggestLocalRefresh("invalid data policy")).toBe(false);
  });
  it("keeps automatic refresh conditional and exposes local/always-refresh overrides", () => {
    expect(resolveLocalDataPolicy("auto", false, false)).toBe("cache_only");
    expect(resolveLocalDataPolicy("auto", false, true)).toBe("refresh");
    expect(resolveLocalDataPolicy("auto", true, false)).toBe("refresh");
    expect(resolveLocalDataPolicy("cache_only", true, true)).toBe("cache_only");
    expect(resolveLocalDataPolicy("refresh", false, false)).toBe("refresh");
  });
});
