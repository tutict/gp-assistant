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

async function searchFor(renderer: ReactTestRenderer, query: string) {
  await act(async () => {
    renderer.root.findByProps({ id: "hk-stock-search" }).props.onChange({ currentTarget: { value: query } });
    await Promise.resolve();
  });
}

describe("HkMarketPanel stability", () => {
  beforeEach(() => { getJsonMock.mockReset(); postJsonMock.mockReset(); });

  it("hides the full directory and shows Chinese-name matches only after the user searches", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00001", name: "CKH HOLDINGS", name_zh: "长和", currency: "HKD", board_lot: 500, security_type: "Equity / Equity Securities (Main Board)", classification_status: "exchange_listed_equity" }],
      source: "HKEX",
      refresh_enabled: true,
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });

    expect(renderer.root.findAllByProps({ className: "stock-row hk-security-row" })).toHaveLength(0);
    expect(JSON.stringify(renderer.toJSON())).toContain("输入代码或中文名后显示匹配证券");

    await searchFor(renderer, "长和");

    expect(getJsonMock).toHaveBeenLastCalledWith("/api/hk/stock-search?q=%E9%95%BF%E5%92%8C&limit=50");
    expect(JSON.stringify(renderer.toJSON())).toContain("长和");
    expect(JSON.stringify(renderer.toJSON())).not.toContain("CKH HOLDINGS");
  });

  it("explains when the directory provider is temporarily unavailable", async () => {
    getJsonMock.mockResolvedValue({
      items: [], source: "", refresh_enabled: false,
      refresh_reason: "HKEX directory endpoint is temporarily unavailable",
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} />); });
    expect(JSON.stringify(renderer.toJSON())).toContain("输入代码或中文名后显示匹配证券。");
    expect(JSON.stringify(renderer.toJSON())).toContain("HKEX directory endpoint is temporarily unavailable");
    expect(JSON.stringify(renderer.toJSON())).not.toContain("数据源：");
    expect(postJsonMock).not.toHaveBeenCalled();
  });

  it("does not attempt a directory refresh while the provider reports unavailable", async () => {
    getJsonMock.mockResolvedValue({
      items: [], source: "HKEX", refresh_enabled: false,
      refresh_reason: "HKEX directory endpoint is temporarily unavailable",
    });
    const capability: MarketCapability = {
      market: "hk",
      id: "security_directory",
      status: "unavailable",
      reason: "HKEX directory endpoint is temporarily unavailable",
      coverage: null,
    };
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={capability} />);
    });

    const updateButton = renderer.root
      .findAllByType("button")
      .find((button) => button.children.join("") === "刷新目录与行情");
    expect(updateButton).toBeTruthy();
    expect(updateButton?.props.disabled).toBe(true);
    expect(JSON.stringify(renderer.toJSON())).toContain("HKEX directory endpoint is temporarily unavailable");

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
      .find((button) => button.children.join("") === "刷新目录与行情");
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
    postJsonMock.mockResolvedValue({
      security_count: 1,
      quote_refresh: { requested_count: 1, attempted_count: 1, updated_count: 1, failed_count: 0, unsupported_currency_count: 0 },
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={vi.fn()} capability={availableDirectoryCapability} />); });
    await act(async () => { renderer.root.findByProps({ children: "刷新目录与行情" }).props.onClick(); await Promise.resolve(); });
    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/refresh");
    expect(JSON.stringify(renderer.toJSON())).toContain("输入代码或中文名后显示匹配证券。");
    expect(renderer.root.findAllByProps({ className: "stock-row hk-security-row" })).toHaveLength(0);
    expect(renderer.root.findByProps({ className: "hk-quote-refresh-summary" }).children.join("")).toContain("批量行情：更新 1/1 只");
    expect(JSON.stringify(renderer.toJSON())).toContain("2026-10-19T12:00:00Z");
  });

  it("lets a user confirm a local ordinary-share classification", async () => {
    getJsonMock
      .mockResolvedValueOnce({
        items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "unverified" }],
        source: "HKEX",
        refresh_enabled: true,
      })
      .mockResolvedValueOnce({
        items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "unverified" }],
        source: "HKEX",
        refresh_enabled: true,
      })
      .mockResolvedValueOnce({
        items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity", classification_status: "user_confirmed_ordinary_share" }],
        source: "HKEX",
        refresh_enabled: true,
      });
    postJsonMock.mockResolvedValue({ code: "00700", classification_status: "user_confirmed_ordinary_share" });
    const onObserveStock = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={onObserveStock} />); });
    await searchFor(renderer, "00700");

    expect(renderer.root.findByProps({ children: "确认普通股" })).toBeTruthy();
    await act(async () => { await renderer.root.findByProps({ children: "确认普通股" }).props.onClick(); });

    expect(postJsonMock).toHaveBeenCalledWith("/api/hk/confirm-ordinary-share", { code: "00700.HK" });
    expect(JSON.stringify(renderer.toJSON())).toContain("已由用户确认普通股");
    const select = renderer.root.findByProps({ className: "hk-security-select" });
    expect(select.props.disabled).toBe(false);
    await act(async () => select.props.onClick());
    expect(onObserveStock).toHaveBeenCalledWith("00700.HK");
  });

  it("allows an HKEX-listed equity to be observed without per-security confirmation", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00700", name: "Tencent", currency: "HKD", board_lot: 100, security_type: "Equity / Equity Securities (Main Board)", classification_status: "exchange_listed_equity" }],
      source: "HKEX",
      refresh_enabled: true,
    });
    const onObserveStock = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={onObserveStock} />); });
    await searchFor(renderer, "00700");

    const select = renderer.root.findByProps({ className: "hk-security-select" });
    expect(select.props.disabled).toBe(false);
    expect(JSON.stringify(renderer.toJSON())).toContain("HKEX 上市股权");
    expect(renderer.root.findAllByProps({ children: "确认普通股" })).toHaveLength(0);

    await act(async () => select.props.onClick());
    expect(onObserveStock).toHaveBeenCalledWith("00700.HK");
  });

  it("keeps confirmed CNY-counter shares out of observation without a CNY quote adapter", async () => {
    getJsonMock.mockResolvedValue({
      items: [{ code: "00012", name: "Dual Counter", currency: "CNY", board_lot: 100, security_type: "Equity", classification_status: "user_confirmed_ordinary_share" }],
      source: "HKEX",
      refresh_enabled: true,
    });
    const onObserveStock = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkMarketPanel onObserveStock={onObserveStock} />); });
    await searchFor(renderer, "00012");

    const row = renderer.root.findByProps({ className: "stock-row hk-security-row" });
    expect(JSON.stringify(renderer.toJSON())).toContain("当前仅支持 HKD 报价");
    expect(row.findByProps({ className: "hk-security-select" }).props.disabled).toBe(true);
    expect(onObserveStock).not.toHaveBeenCalled();
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
    await searchFor(renderer, "00700");
    await act(async () => { renderer.root.findByProps({ children: "刷新目录与行情" }).props.onClick(); await Promise.resolve(); });
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
    await act(async () => { renderer.root.findByProps({ children: "刷新目录与行情" }).props.onClick(); await Promise.resolve(); });

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
    await searchFor(renderer, "00012");

    const row = renderer.root.findByProps({ className: "stock-row hk-security-row" });
    const rowText = row.findAllByType("span").map((span) => span.children.join("")).join(" ");
    expect(rowText).toContain("00012.HK · CNY · 每手 100 股");
    expect(rowText).toContain("分类未核验 · 当前仅支持 HKD 报价");
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
    await searchFor(renderer, "00700");
    expect(renderer.root.findByProps({ className: "hk-security-select" }).props.disabled).toBe(true);
    expect(onObserveStock).not.toHaveBeenCalled();
  });
});
