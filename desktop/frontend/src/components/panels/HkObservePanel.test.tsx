import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HkObservePanel } from "./HkObservePanel";
import { getJson } from "../../lib/tauri";

vi.mock("../../lib/tauri", () => ({ getJson: vi.fn() }));
const getJsonMock = vi.mocked(getJson);

const staleResult = {
  market: "hk",
  security: { code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity" },
  quote: { code: "00700", name: "Tencent", price: 419, currency: "HKD", source: "Tencent Finance", fetched_at: "2026-10-19T12:00:00Z" },
  history: [{ date: "2026-10-18", open: 418, high: 420, low: 416, close: 419, volume_shares: 100 }],
  quote_updated_at: "2026-10-19T12:00:00Z",
  history_updated_at: "2026-10-18T16:00:00Z",
  quote_error: "报价服务暂时不可用",
  history_error: null,
  notes: ["报价：报价服务暂时不可用"],
  stale: true,
};

describe("HkObservePanel stability", () => {
  beforeEach(() => getJsonMock.mockReset());

  it("shows independent update times, field errors and an explicit refresh action", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("报价更新时间");
    expect(text).toContain("日 K 更新时间");
    expect(text).toContain("报价服务暂时不可用");
    expect(renderer.root.findByProps({ children: "刷新行情" })).toBeTruthy();
  });

  it("refreshes only after the user presses the refresh button", async () => {
    getJsonMock.mockResolvedValue(staleResult);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    expect(getJsonMock).toHaveBeenCalledTimes(1);
    await act(async () => { renderer.root.findByProps({ children: "刷新行情" }).props.onClick(); await Promise.resolve(); });
    expect(getJsonMock).toHaveBeenLastCalledWith("/api/hk/observe/00700.HK?data_policy=refresh");
  });

  it("keeps the last successful observation visible when refresh fails", async () => {
    getJsonMock.mockResolvedValueOnce({ ...staleResult, stale: false, notes: [] }).mockRejectedValueOnce(new Error("Tencent unavailable"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkObservePanel code="00700.HK" watchlist={[]} onWatchlistChange={vi.fn()} />); });
    await act(async () => { renderer.root.findByProps({ children: "刷新行情" }).props.onClick(); await Promise.resolve(); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("Tencent");
    expect(text).toContain("Tencent unavailable");
  });
});
