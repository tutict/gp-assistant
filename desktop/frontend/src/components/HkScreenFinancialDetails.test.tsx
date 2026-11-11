import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HkScreenFinancialDetails } from "./HkScreenFinancialDetails";
import { StockList } from "./StockList";
import { normalizeSectorGroups } from "../lib/contracts";
import { getJson, postJson } from "../lib/tauri";
import type { HkFinancialResult, HkFinancialSnapshot } from "../types";

vi.mock("../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));

const snapshot: HkFinancialSnapshot = {
  code: "01798", currency: "CNY", normalized_period: "2026Q3", raw_report_period: "2026年三季报",
  report_date: "2026-09-30", candidate_source: "Eastmoney HK financial indicators",
  candidate_fetched_at: "1794116685279", verification_status: "unverified",
  eps: { value: 0.29, currency: "CNY", unit: "currency/share", source: "Eastmoney HK financial indicators" },
  roe: { value: 7.6, currency: "", unit: "percent", source: "Eastmoney HK financial indicators" },
  revenue: { value: 8880000000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
  net_profit: { value: 2400000000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
};
const result: HkFinancialResult = { market: "hk", code: "01798", snapshot, notes: [], stale: false, field_errors: {} };
const missing: HkFinancialResult = { market: "hk", code: "01798", snapshot: null, notes: [], stale: false, field_errors: { financial: "尚无财务缓存，请点击刷新财务。" } };
let renderer: ReactTestRenderer | undefined;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(getJson).mockReset();
  vi.mocked(postJson).mockReset();
});
afterEach(async () => {
  if (renderer) await act(async () => renderer?.unmount());
  renderer = undefined;
  vi.unstubAllGlobals();
});

describe("HK grouped screening financial details", () => {
  it("shows a grouped preparation failure immediately and retries only on the refresh action", async () => {
    const rows = normalizeSectorGroups({groups:[{sector:"新能源与储能",items:[{stock:{code:"01798.HK",name:"大唐新能源",hk_financial_error:"财务数据源请求超时"},score:15.7}]}]})[0].rows;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer!.root.find(node => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    expect(JSON.stringify(renderer!.toJSON())).toContain("财务数据源请求超时");
    expect(JSON.stringify(renderer!.toJSON())).not.toContain("正在读取财务数据");
    expect(getJson).not.toHaveBeenCalled();
    expect(postJson).not.toHaveBeenCalled();
    vi.mocked(postJson).mockResolvedValue(result);
    await act(async () => renderer!.root.findByProps({"aria-label":"刷新 01798.HK 财务数据"}).props.onClick());
    expect(JSON.stringify(renderer!.toJSON())).toContain("CNY 88.8亿");
    expect(JSON.stringify(renderer!.toJSON())).not.toContain("财务数据源请求超时");
  });

  it("shows pending financial retrieval without announcing failure before it finishes", async () => {
    let complete!: (value: HkFinancialResult) => void;
    vi.mocked(getJson).mockResolvedValue(missing);
    vi.mocked(postJson).mockReturnValue(new Promise<HkFinancialResult>(resolve => { complete = resolve; }));
    await act(async () => { renderer = create(<HkScreenFinancialDetails code="01798.HK" />); });
    expect(JSON.stringify(renderer!.toJSON())).toContain("正在读取财务数据");
    expect(JSON.stringify(renderer!.toJSON())).not.toContain("财务获取未完成");
    await act(async () => complete(result));
    expect(JSON.stringify(renderer!.toJSON())).toContain("CNY 88.8亿");
  });

  it("keeps retrieved financial figures ready when a grouped stock is reopened", async () => {
    const rows = normalizeSectorGroups({ groups: [{ sector: "新能源与储能", items: [{ stock: { code: "01798.HK", name: "大唐新能源" }, score: 15.7 }] }] })[0].rows;
    vi.mocked(getJson).mockResolvedValueOnce(missing).mockReturnValue(new Promise(() => undefined));
    vi.mocked(postJson).mockResolvedValue(result);
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    const toggle = () => renderer!.root.find(node => node.type === "button" && node.props.className.includes("stock-details-toggle"));
    await act(async () => toggle().props.onClick());
    expect(JSON.stringify(renderer!.toJSON())).toContain("CNY 88.8亿");
    await act(async () => toggle().props.onClick());
    await act(async () => toggle().props.onClick());
    expect(JSON.stringify(renderer!.toJSON())).toContain("CNY 88.8亿");
    expect(JSON.stringify(renderer!.toJSON())).not.toContain("正在读取财务数据");
    expect(getJson).toHaveBeenCalledTimes(1);
    expect(postJson).toHaveBeenCalledTimes(1);
  });

  it("renders financial figures bundled with the grouped result without fetching on expansion", async () => {
    const rows = normalizeSectorGroups({ groups: [{ sector: "新能源与储能", items: [{ stock: { code: "01798.HK", name: "大唐新能源", hk_financial_snapshot: snapshot }, score: 15.7 }] }] })[0].rows;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer!.root.find(node => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    const text = JSON.stringify(renderer!.toJSON());
    expect(text).toContain("CNY 88.8亿");
    expect(text).toContain("2026Q3");
    expect(text).toContain("核心报表字段齐全");
    expect(getJson).not.toHaveBeenCalled();
    expect(postJson).not.toHaveBeenCalled();
  });
});
