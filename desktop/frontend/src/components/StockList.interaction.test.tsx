import { useState } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import { StockList } from "./StockList";
import type { StockRowView, WatchlistItem } from "../types";
import { normalizeScreenRows } from "../lib/contracts";
import { getJson, postJson } from "../lib/tauri";

vi.mock("../lib/tauri", () => ({ getJson: vi.fn(), postJson: vi.fn() }));

const items: StockRowView[] = [
  {code:"600000.SH",name:"第一只",score:90,price:0,pe:0,change_pct:0.018,explanation:{basis:["第一项依据"]}},
  {code:"000001.SZ",name:"第二只",score:80,price:NaN,riskTags:["波动较大"]},
];
function Probe() {
  const [watchlist,setWatchlist]=useState<WatchlistItem[]>([]);
  return <StockList items={items} watchlist={watchlist} onToggleWatchlist={item=>setWatchlist(current=>current.some(x=>x.code===item.code)?current.filter(x=>x.code!==item.code):[...current,{code:item.code,name:item.name}])} />;
}
describe("stock comparison interactions",()=>{
  it("shows an A-share loss as loss while retaining the reported EPS", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT",true);
    const rows=normalizeScreenRows({items:[{stock:{code:"000002.SZ",name:"万科A",price:6.08,pe:-6.32,pb:0.42,latest_eps:-2.01},score:10}]});
    let renderer!: ReactTestRenderer;
    await act(async()=>{renderer=create(<StockList items={rows} watchlist={[]} onToggleWatchlist={()=>undefined} />);});
    const text=JSON.stringify(renderer.toJSON());
    expect(text).toContain("亏损");
    expect(text).toContain("-2.01");
    await act(async()=>renderer.unmount());
    vi.unstubAllGlobals();
  });
  it("shows the sourced Chinese company context beside the financial coverage", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const metric = (value: number, unit: string, currency = "CNY") => ({ value, unit, currency, source: "Eastmoney HK financial indicators" });
    const rows = normalizeScreenRows({ items: [{ stock: {
      code: "01490.HK", name: "AI X TECH", industry: "媒体及娱乐", price: 0.25,
      hk_financial_snapshot: {
        code: "01490", currency: "CNY", normalized_period: "2026H1", raw_report_period: "2026年 中报",
        candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "1793970111143", verification_status: "unverified",
        eps: metric(0.014, "currency/share"), roe: metric(2.917261819081, "percent", ""), revenue: metric(68674000, "currency_total"), net_profit: metric(16992000, "currency_total"),
        company_profile: { code: "01490", name: "AI X Tech Inc.", name_zh: null, industry: "媒体及娱乐", business_summary: "主要运营汽车资讯线上媒体平台。", source: "Eastmoney HK company profile", fetched_at: "1793970111143" },
      },
    }, score: 13.95 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("汽车资讯");
    expect(text).toContain("Eastmoney HK company profile");
    expect(text).toContain("核心报表字段齐全");
    expect(text).toContain("4/4");
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("explains an HK English short name and preserves a small negative USD EPS", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const rows = normalizeScreenRows({ items: [{ stock: {
      code: "01490.HK", name: "AI X TECH", price: 0.25, pe: -80.64, latest_eps: -0.0016,
      quote_currency: "HKD",
    }, score: 13.95 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    const rendered = JSON.stringify(renderer.toJSON());
    expect(rendered).toContain("英文简称");
    expect(rendered).toContain("亏损");
    expect(rendered).toContain("-0.0016");
    expect(rendered).not.toContain('"-0.00"');
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("shows a failed-refresh warning when opening a snapshot retained by the cache API", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.mocked(getJson).mockReset().mockResolvedValue({ snapshot: {
      code: "02555", currency: "CNY", normalized_period: "2026H1", raw_report_period: "2026年 中报",
      candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "1793970111143", verification_status: "unverified",
      revenue: { value: 2654769000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
    }, stale: true, field_errors: { financial: "cached financial refresh failed" } });
    vi.mocked(postJson).mockReset();
    const rows = normalizeScreenRows({ items: [{ stock: { code: "02555.HK", name: "茶百道" }, score: 15 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    const rendered = JSON.stringify(renderer.toJSON());
    expect(rendered).toContain("CNY 26.55亿");
    expect(rendered).toContain("cached financial refresh failed");
    expect(postJson).not.toHaveBeenCalled();
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("keeps one set of current EPS and ROE values in an expanded HK row after refresh", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const financial = {
      code: "02555", currency: "CNY", normalized_period: "2026H1", raw_report_period: "2026年 中报",
      candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "1793970111143", verification_status: "unverified",
      eps: { value: 0.228, currency: "CNY", unit: "currency/share", source: "Eastmoney HK financial indicators" },
      roe: { value: 8.55, currency: "", unit: "percent", source: "Eastmoney HK financial indicators" },
    };
    vi.mocked(postJson).mockReset().mockResolvedValue({ snapshot: {
      ...financial, normalized_period: "2026Q3", candidate_fetched_at: "1793970112143",
      eps: { ...financial.eps, value: 0.35 }, roe: { ...financial.roe, value: 12 },
    } });
    const rows = normalizeScreenRows({ items: [{ stock: { code: "02555.HK", name: "茶百道", latest_eps: 0.228, roe: 0.0855, hk_financial_snapshot: financial }, score: 15 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    await act(async () => renderer.root.findByProps({ "aria-label": "刷新 02555.HK 财务数据" }).props.onClick());
    const detail = renderer.root.findByProps({ className: "stock-details" });
    const text = JSON.stringify(detail.findAll(node => typeof node.type === "string").map(node => node.children.filter(child => typeof child === "string")));
    expect(text).toContain("CNY 0.35/股");
    expect(text).toContain("12%");
    expect(text).not.toContain("CNY 0.228/股");
    expect(text).not.toContain("8.55%");
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("keeps cached figures visible and reports a financial refresh failure returned by the API", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const financial = {
      code: "02555", currency: "CNY", normalized_period: "2026H1", raw_report_period: "2026年 中报",
      candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "1793970111143", verification_status: "unverified",
      revenue: { value: 2654769000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
    };
    vi.mocked(postJson).mockReset().mockResolvedValue({ snapshot: financial, stale: true, field_errors: { financial: "financial source timeout" } });
    const rows = normalizeScreenRows({ items: [{ stock: { code: "02555.HK", name: "茶百道", hk_financial_snapshot: financial }, score: 15 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    await act(async () => renderer.root.findByProps({ "aria-label": "刷新 02555.HK 财务数据" }).props.onClick());
    const rendered = JSON.stringify(renderer.toJSON());
    expect(rendered).toContain("CNY 26.55亿");
    expect(rendered).toContain("financial source timeout");
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("does not start a financial refresh after the user closes an HK detail", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    let finishCacheRead!: (value: unknown) => void;
    vi.mocked(getJson).mockReset().mockReturnValue(new Promise(resolve => { finishCacheRead = resolve; }));
    vi.mocked(postJson).mockReset().mockResolvedValue({ snapshot: null });
    const rows = normalizeScreenRows({ items: [{ stock: { code: "02555.HK", name: "茶百道" }, score: 15 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    const toggle = () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle"));
    await act(async () => toggle().props.onClick());
    await act(async () => toggle().props.onClick());
    await act(async () => { finishCacheRead({ snapshot: null }); await Promise.resolve(); });
    expect(toggle().props["aria-expanded"]).toBe(false);
    expect(postJson).not.toHaveBeenCalled();
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("fetches missing HK financial data only when its details are opened", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.mocked(getJson).mockReset().mockResolvedValue({ snapshot: null });
    vi.mocked(postJson).mockReset().mockResolvedValue({ snapshot: {
      code: "02555", currency: "CNY", normalized_period: "2026H1", raw_report_period: "2026年 中报",
      report_date: "2026-06-30", candidate_source: "Eastmoney HK financial indicators", candidate_fetched_at: "1793970111143",
      verification_status: "unverified",
      revenue: { value: 2654769000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
    } });
    const rows = normalizeScreenRows({ items: [{ stock: { code: "02555.HK", name: "茶百道", price: 3.86 }, score: 15.52 }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    expect(getJson).not.toHaveBeenCalled();
    expect(postJson).not.toHaveBeenCalled();
    await act(async () => {
      renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick();
      await Promise.resolve();
    });
    expect(JSON.stringify(renderer.toJSON())).toContain("CNY 26.55亿");
    expect(getJson).toHaveBeenCalledWith("/api/hk/financial-status?code=02555.HK");
    expect(postJson).toHaveBeenCalledWith("/api/hk/financial-refresh", { code: "02555.HK" });
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("shows cached HK financial details with their report period, currency and source", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const rows = normalizeScreenRows({ items: [{
      stock: {
        code: "02555.HK", name: "茶百道", price: 3.86, pe: 6.4, pb: 1.27,
        latest_eps: 0.228, roe: 0.08549423031387, quote_currency: "HKD", market_cap_billion: 5.70367,
        hk_financial_snapshot: {
          code: "02555", currency: "CNY", normalized_period: "2026H1", raw_report_period: "2026年 中报",
          report_date: "2026-06-30", disclosed_at: "2026-09-18", candidate_source: "Eastmoney HK financial indicators",
          candidate_fetched_at: "1793970111143", verification_status: "unverified",
          eps: { value: 0.228, currency: "CNY", unit: "currency/share", source: "Eastmoney HK financial indicators" },
          revenue: { value: 2654769000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
          net_profit: { value: 344729000, currency: "CNY", unit: "currency_total", source: "Eastmoney HK financial indicators" },
          additional_metrics: {
            revenue_yoy: { value: 6.209877778, currency: "", unit: "percent", source: "Eastmoney HK financial indicators" },
            net_margin: { value: 12.9852729183, currency: "", unit: "percent", source: "Eastmoney HK financial indicators" },
            bps: { value: 2.633982665196, currency: "CNY", unit: "currency/share", source: "Eastmoney HK financial indicators" },
          },
        },
      }, score: 15.52, reasons: [],
    }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    const rendered = JSON.stringify(renderer.toJSON());
    expect(rendered).toContain("营业总收入");
    expect(rendered).toContain("CNY 26.55亿");
    expect(rendered).toContain("CNY 3.45亿");
    expect(rendered).toContain("CNY 0.228/股");
    expect(rendered).toContain("2026H1");
    expect(rendered).toContain("2026-06-30");
    expect(rendered).toContain("Eastmoney HK financial indicators");
    expect(rendered).toContain("HKD 57.04亿");
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("dates legacy trend evidence and separates KDJ risk from supporting reasons", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const rows = normalizeScreenRows({ items: [{
      stock: { code: "688279.SH", name: "峰岹科技", price: 173.73, change_pct: -0.1114, quote_time: "20261030161451" },
      final_score: 73.37, trend_score: 73.37,
      reasons: ["high_quant_score", "kdj_overbought", "swl_above_sws", "swl_strength", "unknown_reason", "主题:半导体"],
      signal: { date: "2026-09-23", close: 215.42, k: 82.43, d: 73.38, j: 100.53,
        kdj_overbought: true, kdj_oversold: false, risk_flags: ["kdj_overbought"] },
      explanation: { basis: [
        "Passed the current base screen with raw score 14.69.",
        "trend_continuation signal with technical 89.29, pattern 13.85, quality 81.00.",
      ] },
    }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<StockList items={rows} watchlist={[]} onToggleWatchlist={() => undefined} />); });
    await act(async () => renderer.root.find((node) => node.type === "button" && node.props.className.includes("stock-details-toggle")).props.onClick());
    const support = renderer.root.find((node) => node.type === "ul" && node.props["aria-label"] === "支持依据");
    const risks = renderer.root.find((node) => node.type === "ul" && node.props["aria-label"] === "风险提示");
    const supportText = JSON.stringify(support.children.map((node) => typeof node === "string" ? node : node.children));
    expect(supportText).not.toContain("KDJ超买");
    expect(supportText).not.toContain("未识别原因");
    expect(supportText).not.toContain("Passed the current");
    expect(supportText).not.toContain("trend_continuation");
    expect(supportText.match(/短期均线高于长期均线/g)).toHaveLength(1);
    expect(supportText).toContain("主题:半导体");
    const rendered = JSON.stringify(renderer.toJSON());
    expect(rendered).toContain("历史指标待更新");
    expect(rendered).toContain("2026-09-23");
    expect(rendered).toContain("2026-10-30");
    expect(rendered).toContain("82.43");
    expect(rendered).toContain("100.53");
    expect(JSON.stringify(risks.children.map((node) => typeof node === "string" ? node : node.children))).toContain("KDJ超买");
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("keeps one expanded detail, preserves it on favorite, and formats zero and missing values",async()=>{
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT",true);
    let renderer!:ReactTestRenderer;
    await act(async()=>{renderer=create(<Probe/>);});
    const toggles=()=>renderer.root.findAll(node=>node.type==='button'&&node.props.className.includes('stock-details-toggle'));
    await act(async()=>toggles()[0].props.onClick());
    expect(toggles()[0].props['aria-expanded']).toBe(true);
    const favorites=renderer.root.findAll(node=>node.type==='button'&&node.props.className.includes('watchlist-action'));
    await act(async()=>favorites[0].props.onClick());
    expect(toggles()[0].props['aria-expanded']).toBe(true);
    expect(favorites[0].props['aria-pressed']).toBe(true);
    await act(async()=>toggles()[1].props.onClick());
    expect(toggles()[0].props['aria-expanded']).toBe(false);
    expect(toggles()[1].props['aria-expanded']).toBe(true);
    const html=JSON.stringify(renderer.toJSON());
    expect(html).toContain('0.00');
    expect(html).toContain('+1.80%');
    expect(html).toContain('—');
    expect(html).toContain('波动较大');
    await act(async()=>renderer.unmount());
    vi.unstubAllGlobals();
  });

  it("opens news for the selected row without leaving the list", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const onNewsStock = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<StockList items={items} watchlist={[]} onToggleWatchlist={() => undefined} onNewsStock={onNewsStock} />);
    });
    const news = renderer.root.findAll((node) => node.type === "button" && node.children.includes("消息"));
    await act(async () => news[0].props.onClick());
    expect(onNewsStock).toHaveBeenCalledWith("600000.SH");
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });
});
