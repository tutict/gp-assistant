import { describe, expect, it } from "vitest";
import { shouldSuggestLocalRefresh, localDataUnavailable, resolveLocalDataPolicy } from "./localData";
describe("local-first action state", () => {
  it("treats an empty or missing market cache as refresh-required", () => {
    expect(localDataUnavailable(null)).toBe(true);
    expect(localDataUnavailable({ universe_count: 0 })).toBe(true);
    expect(localDataUnavailable({ universe_count: 120, stale: true })).toBe(false);
  });
  it("suggests refresh only for structured local-data misses", () => {
    expect(shouldSuggestLocalRefresh(JSON.stringify({ code: "LOCAL_DATA_MISSING" }))).toBe(true);
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
