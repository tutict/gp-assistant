import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HkMarketPanel } from "./HkMarketPanel";
import { getJson, postJson } from "../../lib/tauri";
import type { MarketCapability } from "../../lib/marketDomain";

vi.mock("../../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));

const getJsonMock = vi.mocked(getJson);
const postJsonMock = vi.mocked(postJson);

const availableDirectoryCapability: MarketCapability = {
  market: "hk",
  id: "security_directory",
  status: "available",
  reason: null,
  coverage: { requested_count: 1, available_count: 1, missing_count: 0, start_date: null, end_date: null, missing_fields: [] },
};

describe("HkMarketPanel stability", () => {
  beforeEach(() => { getJsonMock.mockReset(); postJsonMock.mockReset(); });

  it("explains that an empty cache needs an explicit directory refresh", async () => {
    getJsonMock.mockResolvedValue({
      items: [], source: "", refresh_enabled: false,
      refresh_reason: "HKEX security-directory refresh is disabled until written permission for automated access is documented",
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });
    expect(JSON.stringify(renderer.toJSON())).toContain("当前目录为空。");
    expect(JSON.stringify(renderer.toJSON())).toContain("HKEX 自动访问授权和使用审核完成前，目录在线更新暂不可用。");
    expect(JSON.stringify(renderer.toJSON())).not.toContain("数据源：");
    expect(postJsonMock).not.toHaveBeenCalled();
  });

  it("does not attempt a directory refresh while the provider gate is closed", async () => {
    getJsonMock.mockResolvedValue({
      items: [], source: "HKEX", refresh_enabled: false,
      refresh_reason: "HKEX security-directory refresh is disabled until written permission for automated access is documented",
    });
    const capability: MarketCapability = {
      market: "hk",
      id: "security_directory",
      status: "unavailable",
      reason: "HKEX directory cache is empty; live refresh remains disabled pending written automated-access permission",
      coverage: null,
    };
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={capability} />);
    });

    const updateButton = renderer.root
      .findAllByType("button")
      .find((button) => button.children.join("") === "更新目录");
    expect(updateButton).toBeTruthy();
    expect(updateButton?.props.disabled).toBe(true);
    expect(JSON.stringify(renderer.toJSON())).toContain("HKEX 自动访问授权和使用审核完成前，目录在线更新暂不可用。");

    await act(async () => {
      updateButton?.props.onClick?.();
      await Promise.resolve();
    });
    expect(postJsonMock).not.toHaveBeenCalled();
  });

  it("allows explicit refresh when the cache capability is unavailable but provider readiness is confirmed", async () => {
    getJsonMock
      .mockResolvedValueOnce({ items: [], source: "HKEX", refresh_enabled: true })
      .mockResolvedValueOnce({ items: [], source: "HKEX", refresh_enabled: true });
    postJsonMock.mockResolvedValue({ security_count: 0 });
    const capability: MarketCapability = {
      ...availableDirectoryCapability,
      status: "unavailable",
      reason: "directory cache is empty",
    };
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={capability} />);
    });
    const updateButton = renderer.root.findAllByType("button")
      .find((button) => button.children.join("") === "更新目录");
    expect(updateButton?.props.disabled).toBe(false);
    await act(async () => { updateButton?.props.onClick?.(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/refresh");
  });

  it("refreshes the directory only after the user clicks update", async () => {
    getJsonMock.mockResolvedValueOnce({ items: [], source: "HKEX", refresh_enabled: true }).mockResolvedValueOnce({
      items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "verified_ordinary_share" }],
      source: "HKEX",
      updated_at: "2026-10-19T12:00:00Z",
      refresh_enabled: true,
    });
    postJsonMock.mockResolvedValue({ security_count: 1 });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={availableDirectoryCapability} />); });
    await act(async () => { renderer.root.findByProps({ children: "更新目录" }).props.onClick(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/refresh");
    expect(JSON.stringify(renderer.toJSON())).toContain("Tencent");
    expect(JSON.stringify(renderer.toJSON())).toContain("2026-10-19T12:00:00Z");
  });

  it("keeps an existing directory visible when an explicit refresh fails", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "unverified" }],
      source: "HKEX",
      updated_at: "old-directory",
      refresh_enabled: true,
    });
    postJsonMock.mockRejectedValue(new Error("HKEX unavailable"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={availableDirectoryCapability} />); });
    await act(async () => { renderer.root.findByProps({ children: "更新目录" }).props.onClick(); await Promise.resolve(); });
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("Tencent");
    expect(text).toContain("HKEX unavailable");
    expect(text).toContain("分类未核验，暂不可观察");
  });

  it("preserves the filtered cache and source metadata when refresh succeeds but the follow-up cache read fails", async () => {
    const cachedItem = { code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity" };
    getJsonMock
      .mockResolvedValueOnce({ items: [cachedItem], source: "HKEX cache", updated_at: "cached-at", refresh_enabled: true })
      .mockResolvedValueOnce({ items: [cachedItem], source: "HKEX cache", updated_at: "cached-at", refresh_enabled: true })
      .mockRejectedValueOnce(new Error("directory cache read failed"));
    postJsonMock.mockResolvedValue({ security_count: 1 });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={availableDirectoryCapability} />); });
    await act(async () => {
      renderer.root.findByProps({ id: "hk-stock-search" }).props.onChange({ currentTarget: { value: "腾讯" } });
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(renderer.root.findByProps({ className: "stock-row hk-security-row" })).toBeTruthy();
    await act(async () => { renderer.root.findByProps({ children: "更新目录" }).props.onClick(); await Promise.resolve(); });

    const text = JSON.stringify(renderer.toJSON());
    expect(renderer.root.findByProps({ id: "hk-stock-search" }).props.value).toBe("腾讯");
    expect(renderer.root.findByProps({ className: "stock-row hk-security-row" })).toBeTruthy();
    expect(text).toContain("cached-at");
    expect(text).toContain("HKEX cache");
    expect(text).toContain("directory cache read failed");
    expect(text).not.toContain("港股目录读取失败");
  });

  it("displays the security-master currency without converting CNY to HKD", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00012", name: "Dual Counter", currency: "CNY", board_lot: 100, security_type: "Equity", classification_status: "unverified" }],
      source: "HKEX",
      updated_at: "2026-10-22T00:00:00Z",
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });

    const row = renderer.root.findByProps({ className: "stock-row hk-security-row" });
    const rowText = row.findAllByType("span").map((span) => span.children.join("")).join(" ");
    expect(rowText).toContain("00012.HK · CNY · 每手 100 股");
    expect(rowText).toContain("分类未核验，暂不可观察");
    expect(row.findByProps({ className: "hk-security-select" }).props.disabled).toBe(true);
  });

  it("does not enter observation for a directory entry without verified ordinary-share classification", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "unverified" }],
      source: "HKEX",
    });
    const onObserveStock = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={onObserveStock} />); });
    expect(renderer.root.findByProps({ className: "hk-security-select" }).props.disabled).toBe(true);
    expect(onObserveStock).not.toHaveBeenCalled();
  });
});
