import { useState } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import { StockList } from "./StockList";
import type { StockRowView, WatchlistItem } from "../types";
import { normalizeScreenRows } from "../lib/contracts";

const items: StockRowView[] = [
  {code:"600000.SH",name:"第一只",score:90,price:0,pe:0,change_pct:0.018,explanation:{basis:["第一项依据"]}},
  {code:"000001.SZ",name:"第二只",score:80,price:NaN,riskTags:["波动较大"]},
];
function Probe() {
  const [watchlist,setWatchlist]=useState<WatchlistItem[]>([]);
  return <StockList items={items} watchlist={watchlist} onToggleWatchlist={item=>setWatchlist(current=>current.some(x=>x.code===item.code)?current.filter(x=>x.code!==item.code):[...current,{code:item.code,name:item.name}])} />;
}
describe("stock comparison interactions",()=>{
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
