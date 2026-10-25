import { useEffect, useState } from "react";
import { getJson, postJson } from "../../lib/tauri";
import type { HkSecurity } from "../../types";
import { PanelFeedback } from "../ui/PanelFeedback";

interface HkSearchResponse { items: HkSecurity[]; source?: string; updated_at?: string | null; last_attempt_at?: string | null; last_error?: string | null; }
interface HkMarketPanelProps { onObserveStock: (code: string) => void; }

export function HkMarketPanel({ onObserveStock }: HkMarketPanelProps) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<HkSecurity[]>([]);
  const [meta, setMeta] = useState<HkSearchResponse>({ items: [] });
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const preserveCacheAfterReadFailure = (reason: unknown) => {
    const message = reason instanceof Error ? reason.message : String(reason);
    if (items.length) {
      setError(null);
      setMeta((previous) => ({ ...previous, last_error: message }));
    } else {
      setError(message);
    }
  };

  const search = async (nextQuery = query) => {
    setLoading(true);
    setError(null);
    try {
      const response = await getJson<HkSearchResponse>(`/api/hk/stock-search?q=${encodeURIComponent(nextQuery)}&limit=50`);
      setItems(response.items || []);
      setMeta(response);
    } catch (reason) {
      preserveCacheAfterReadFailure(reason);
    } finally { setLoading(false); }
  };

  useEffect(() => { void search(""); }, []);

  const refresh = async () => {
    setRefreshing(true);
    setError(null);
    try { await postJson("/api/hk/refresh"); await search(); }
    catch (reason) {
      preserveCacheAfterReadFailure(reason);
    }
    finally { setRefreshing(false); }
  };

  return <section className="panel-container hk-market-panel" aria-label="港股证券目录">
    <header className="panel-heading">
      <div><span className="eyebrow">港股行情</span><h2>港股证券目录</h2><p>普通股 · 报价与日 K · 数据源：{meta.source || "HKEX"}</p></div>
      <button type="button" className="action-btn" onClick={() => void refresh()} disabled={refreshing}>{refreshing ? "更新中…" : "更新目录"}</button>
    </header>
    <div className="form-row inline">
      <label htmlFor="hk-stock-search">代码或名称</label>
      <input id="hk-stock-search" value={query} onChange={(event) => { setQuery(event.currentTarget.value); void search(event.currentTarget.value); }} placeholder="如 00700 或 腾讯" />
    </div>
    {meta.last_error ? <p className="data-stale-notice">目录最近刷新失败：{meta.last_error}；当前保留旧目录。</p> : null}
    {loading ? <PanelFeedback kind="loading" description="正在读取港股证券目录…" /> : error ? <PanelFeedback kind="error" title="港股目录读取失败" description={error} action={<button type="button" className="action-btn" onClick={() => void search()}>重试</button>} /> : !items.length ? <PanelFeedback kind="empty" description={query ? "没有匹配的港股证券。" : "请先点击「更新目录」加载港股证券列表。"} /> : <div className="hk-security-list" role="list">
      {items.map((item) => <button type="button" role="listitem" className="hk-security-row" key={item.code} onClick={() => onObserveStock(`${item.code}.HK`)}><strong>{item.name}</strong><span>{item.code}.HK · {item.currency} · 每手 {item.board_lot ?? "--"} 股</span></button>)}
    </div>}
    {meta.updated_at ? <small className="data-provenance">目录更新时间：{meta.updated_at}</small> : null}
  </section>;
}
