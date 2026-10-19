import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HkMarketPanel } from "./HkMarketPanel";
import { getJson, postJson } from "../../lib/tauri";

vi.mock("../../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));

const getJsonMock = vi.mocked(getJson);
const postJsonMock = vi.mocked(postJson);

describe("HkMarketPanel stability", () => {
  beforeEach(() => { getJsonMock.mockReset(); postJsonMock.mockReset(); });

  it("explains that an empty cache needs an explicit directory refresh", async () => {
    getJsonMock.mockResolvedValue({ items: [], source: "HKEX" });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });
    expect(JSON.stringify(renderer.toJSON())).toContain("请先点击「更新目录」");
    expect(postJsonMock).not.toHaveBeenCalled();
  });

  it("refreshes the directory only after the user clicks update", async () => {
    getJsonMock.mockResolvedValueOnce({ items: [], source: "HKEX" }).mockResolvedValueOnce({
      items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity" }],
      source: "HKEX",
      updated_at: "2026-10-19T12:00:00Z",
    });
    postJsonMock.mockResolvedValue({ security_count: 1 });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });
    await act(async () => { renderer.root.findByProps({ children: "更新目录" }).props.onClick(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/refresh");
    expect(JSON.stringify(renderer.toJSON())).toContain("Tencent");
    expect(JSON.stringify(renderer.toJSON())).toContain("2026-10-19T12:00:00Z");
  });

  it("keeps an existing directory visible when an explicit refresh fails", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity" }],
      source: "HKEX",
      updated_at: "old-directory",
    });
    postJsonMock.mockRejectedValue(new Error("HKEX unavailable"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });
    await act(async () => { renderer.root.findByProps({ children: "更新目录" }).props.onClick(); await Promise.resolve(); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("Tencent");
    expect(text).toContain("HKEX unavailable");
  });
});
