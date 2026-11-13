import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StockList } from "./StockList";
import { postJson } from "../lib/tauri";
import type { StockRowView } from "../types";

vi.mock("../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));
vi.mock("../hooks/useMediaQuery", () => ({ useMediaQuery: () => false }));

const stock: StockRowView = {
  code: "600909.SH", name: "华安证券", industry: "证券Ⅱ", price: 6.91,
  qualityScore: 19.04, eps: 1.03, concept: "金融地产", score: 15.14,
};
const response = {
  code: "600909.SH", stale: false, error: null,
  profile: {
    code: "600909.SH", name: "华安证券股份有限公司", industry: "金融-非银行金融-证券",
    business_summary: "证券自营业务投资股票、基金、债券等金融资产",
    company_summary: "业务涵盖证券经纪、资产管理、私募投资、并购重组等领域。",
    source: "东方财富公司资料", fetched_at: "1794388700000",
  },
};
let renderer: ReactTestRenderer | undefined;

async function renderStock() {
  await act(async () => { renderer = create(<StockList items={[stock]} watchlist={[]} onToggleWatchlist={() => undefined} />); });
}
async function toggleDetails() {
  await act(async () => renderer!.root.find(node => node.type === "button" && node.props.className?.includes("stock-details-toggle")).props.onClick());
}
async function refreshCompany() {
  await act(async () => renderer!.root.findByProps({ "aria-label": "刷新 600909.SH 公司资料" }).props.onClick());
}
function text() { return JSON.stringify(renderer!.toJSON()); }

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(postJson).mockReset().mockResolvedValue(response);
});
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  vi.unstubAllGlobals();
});

describe("A-share company context in stock details", () => {
  it("reads company context on expansion and shows the sourced business and company overview", async () => {
    await renderStock();
    expect(postJson).not.toHaveBeenCalled();
    await toggleDetails();
    expect(text()).toContain("证券自营业务投资股票、基金、债券等金融资产");
    expect(text()).toContain("业务涵盖证券经纪、资产管理、私募投资、并购重组等领域。");
    expect(text()).toContain("东方财富公司资料");
    expect(text()).toContain("2026");
    expect(text()).toContain("金融地产");
  });

  it("retains company context when the same row is closed and reopened", async () => {
    await renderStock();
    await toggleDetails();
    await toggleDetails();
    await toggleDetails();
    expect(text()).toContain("华安证券股份有限公司");
    expect(postJson).toHaveBeenCalledTimes(1);
  });

  it("rejects company context returned for another security", async () => {
    vi.mocked(postJson).mockResolvedValue({ ...response, profile: { ...response.profile, code: "000001.SZ", name: "错误公司" } });
    await renderStock();
    await toggleDetails();
    expect(text()).not.toContain("错误公司");
    expect(text()).toContain("未返回匹配当前股票的公司资料");
    expect(text()).toContain("19.04");
  });

  it("keeps selection details readable on a failed lookup and recovers through refresh", async () => {
    vi.mocked(postJson).mockRejectedValueOnce(new Error("公司资料连接超时")).mockResolvedValueOnce(response);
    await renderStock();
    await toggleDetails();
    expect(text()).toContain("公司资料连接超时");
    expect(text()).toContain("19.04");
    await act(async () => renderer!.root.find(node => node.type === "button" && node.props["aria-label"] === "刷新 600909.SH 公司资料").props.onClick());
    expect(text()).toContain("证券自营业务投资股票、基金、债券等金融资产");
    expect(text()).not.toContain("公司资料连接超时");
  });

  it("retains the sourced profile and its original timestamp when a refresh has no cached profile", async () => {
    vi.mocked(postJson).mockResolvedValueOnce(response).mockResolvedValueOnce({
      code: stock.code, profile: null, stale: false, error: "公司资料连接超时",
    });
    await renderStock();
    await toggleDetails();
    const originalProvenance = renderer!.root.findByProps({ className: "data-provenance" }).children.join("");
    await refreshCompany();
    expect(text()).toContain("证券自营业务投资股票、基金、债券等金融资产");
    expect(text()).toContain("东方财富公司资料");
    expect(renderer!.root.findByProps({ className: "data-provenance" }).children.join("")).toContain(originalProvenance);
    expect(text()).toContain("本地缓存");
    expect(text()).toContain("公司资料连接超时");
  });

  it("does not let a refresh completed after closing replace newer company context", async () => {
    let completeOldRefresh!: (value: unknown) => void;
    const updated = { ...response, profile: { ...response.profile, business_summary: "最新证券经纪和资产管理业务", fetched_at: "1794388900000" } };
    vi.mocked(postJson).mockResolvedValueOnce(response)
      .mockReturnValueOnce(new Promise(resolve => { completeOldRefresh = resolve; }))
      .mockResolvedValueOnce(updated);
    await renderStock();
    await toggleDetails();
    await refreshCompany();
    await toggleDetails();
    await toggleDetails();
    await refreshCompany();
    expect(text()).toContain("最新证券经纪和资产管理业务");
    await act(async () => { completeOldRefresh(response); await Promise.resolve(); });
    expect(text()).toContain("最新证券经纪和资产管理业务");
    expect(text()).not.toContain("证券自营业务投资股票、基金、债券等金融资产");
  });
});
