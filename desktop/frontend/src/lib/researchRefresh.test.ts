import { describe, expect, it, vi } from "vitest";
vi.mock("./tauri", () => ({ postJson: vi.fn() }));
import { refreshResearchWatchlist } from "./researchRefresh";

describe("refreshResearchWatchlist", () => {
  it("refreshes every unique stock and isolates individual failures", async () => {
    const request = vi.fn(async (_path: string, payload: Record<string, unknown>) => {
      if (payload.code === "000001.SZ") throw new Error("offline source");
      return {};
    });

    const result = await refreshResearchWatchlist([
      { code: "600000.sh", name: "浦发银行" },
      { code: "000001.SZ", name: "平安银行" },
      { code: "600000.SH", name: "重复项" },
    ], request);

    expect(request).toHaveBeenCalledTimes(2);
    expect(request).toHaveBeenNthCalledWith(1, "/api/research/refresh", expect.objectContaining({
      code: "600000.SH",
      days: 30,
    }));
    expect(request).toHaveBeenNthCalledWith(2, "/api/research/refresh", expect.objectContaining({
      code: "000001.SZ",
      days: 30,
      include_public_sources: false,
    }));
    expect(request).toHaveBeenNthCalledWith(1, "/api/research/refresh", expect.objectContaining({
      include_public_sources: true,
      watchlist_stocks: expect.arrayContaining([
        expect.objectContaining({ code: "600000.SH" }),
      ]),
    }));
    expect(result.refreshed).toEqual(["600000.SH"]);
    expect(result.failed).toEqual([{ code: "000001.SZ", error: "offline source" }]);
  });
});
