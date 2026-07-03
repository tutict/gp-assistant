import { describe, expect, it, vi } from "vitest";
import { fetchObserveDailyHistoryRows, type TimedFetch } from "./observeHistory";

function jsonResponse(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

describe("fetchObserveDailyHistoryRows", () => {
  it("prefers Eastmoney rows for mobile observe history hydration", async () => {
    const timedFetch = vi.fn(async () => jsonResponse({
      rc: 0,
      data: {
        klines: [
          "2024-01-02,4.51,4.57,4.61,4.45,3649741,1661806864.00",
        ],
      },
    }));

    const rows = await fetchObserveDailyHistoryRows({
      code: "000100.SZ",
      start_date: "20200101",
      end_date: "20241231",
    }, 5000, timedFetch as TimedFetch);

    expect(rows).toHaveLength(1);
    expect(rows?.[0]).toMatchObject({
      date: "2024-01-02",
      open: 4.51,
      close: 4.57,
      high: 4.61,
      low: 4.45,
      volume: 3649741,
    });
    expect(timedFetch).toHaveBeenCalledTimes(1);
    expect(String((timedFetch.mock.calls[0] as unknown[] | undefined)?.[0] || "")).toContain("push2his.eastmoney.com");
  });

  it("falls back to Tencent rows when Eastmoney returns nothing usable", async () => {
    const timedFetch = vi.fn()
      .mockResolvedValueOnce(jsonResponse({ rc: 0, data: { klines: [] } }))
      .mockResolvedValueOnce(jsonResponse({
        data: {
          sz000100: {
            day: [
              ["2024-01-02", "4.51", "4.57", "4.61", "4.45", "3649741"],
            ],
          },
        },
      }));

    const rows = await fetchObserveDailyHistoryRows({
      code: "000100.SZ",
      start_date: "20200101",
      end_date: "20241231",
    }, 5000, timedFetch as TimedFetch);

    expect(rows).toHaveLength(1);
    expect(rows?.[0]).toMatchObject({
      date: "2024-01-02",
      close: 4.57,
      volume: 3649741,
    });
    expect(timedFetch).toHaveBeenCalledTimes(2);
    expect(String((timedFetch.mock.calls[1] as unknown[] | undefined)?.[0] || "")).toContain("web.ifzq.gtimg.cn");
  });
});
