import { useState } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import { StockList } from "./StockList";
import type { StockRowView, WatchlistItem } from "../types";

const items: StockRowView[] = [
  {code:"600000.SH",name:"第一只",score:90,price:0,pe:0,change_pct:0.018,explanation:{basis:["第一项依据"]}},
  {code:"000001.SZ",name:"第二只",score:80,price:NaN,riskTags:["波动较大"]},
];
function Probe() {
  const [watchlist,setWatchlist]=useState<WatchlistItem[]>([]);
  return <StockList items={items} watchlist={watchlist} onToggleWatchlist={item=>setWatchlist(current=>current.some(x=>x.code===item.code)?current.filter(x=>x.code!==item.code):[...current,{code:item.code,name:item.name}])} />;
}
describe("stock comparison interactions",()=>{
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
});
