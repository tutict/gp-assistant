import { useEffect, useState } from "react";
import { getJson, postJson } from "../../lib/tauri";
import type { MarketCapability } from "../../lib/marketDomain";
import type { HkSecurity } from "../../types";
import { PanelFeedback } from "../ui/PanelFeedback";

interface HkSearchResponse { items: HkSecurity[]; source?: string; updated_at?: string | null; last_attempt_at?: string | null; last_error?: string | null; }
interface HkMarketPanelProps { onObserveStock: (code: string) => void; capability?: MarketCapability | null; capabilityError?: string | null; }

export function HkMarketPanel({ onObserveStock, capability, capabilityError }: HkMarketPanelProps) {
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
      <div><span className="eyebrow">港股行情</span><h2>港股证券目录</h2><p>HKEX Equity 类目录 · 普通股分类核验后开放观察 · 数据源：{meta.source || "尚无目录缓存"}</p>
        {capability ? <p className="data-provenance">目录能力：{marketCapabilityLabel(capability.status)}{capability.reason ? ` · ${capability.reason}` : ""}</p> : null}
        {capabilityError ? <p className="data-stale-notice" role="alert">目录能力状态读取失败：{capabilityError}</p> : null}
      </div>
      <button type="button" className="action-btn" onClick={() => void refresh()} disabled={refreshing}>{refreshing ? "更新中…" : "更新目录"}</button>
    </header>
    <div className="form-row inline">
      <label htmlFor="hk-stock-search">代码或名称</label>
      <input id="hk-stock-search" value={query} onChange={(event) => { setQuery(event.currentTarget.value); void search(event.currentTarget.value); }} placeholder="如 00700 或 腾讯" />
    </div>
    {meta.last_error ? <p className="data-stale-notice">目录最近刷新失败：{meta.last_error}；当前保留旧目录。</p> : null}
    {loading ? <PanelFeedback kind="loading" description="正在读取港股证券目录…" /> : error ? <PanelFeedback kind="error" title="港股目录读取失败" description={error} action={<button type="button" className="action-btn" onClick={() => void search()}>重试读取缓存</button>} /> : !items.length ? <PanelFeedback kind="empty" description={query ? "没有匹配的港股证券。" : "当前没有目录缓存；HKEX 自动访问授权完成后才能更新目录。"} /> : <div className="hk-security-list" role="list">
      {items.map((item) => {
        const verifiedOrdinaryShare = item.classification_status === "verified_ordinary_share";
        return <button type="button" role="listitem" className="hk-security-row" key={item.code} disabled={!verifiedOrdinaryShare} onClick={() => onObserveStock(`${item.code}.HK`)}>
          <strong>{item.name}</strong>
          <span>{item.code}.HK · {item.currency} · 每手 {item.board_lot ?? "--"} 股 · {verifiedOrdinaryShare ? "普通股已核验" : "分类未核验，暂不可观察"}</span>
        </button>;
      })}
    </div>}
    {meta.updated_at ? <small className="data-provenance">目录更新时间：{meta.updated_at}</small> : null}
  </section>;
}

function marketCapabilityLabel(status: MarketCapability["status"]): string {
  switch (status) {
    case "available": return "可用";
    case "limited": return "部分可用";
    case "unavailable": return "不可用";
  }
}
