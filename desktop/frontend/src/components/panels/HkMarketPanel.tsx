import { useEffect, useState } from "react";
import { getJson, postJson } from "../../lib/tauri";
import type { MarketCapability } from "../../lib/marketDomain";
import type { HkSecurity } from "../../types";
import { PanelFeedback } from "../ui/PanelFeedback";
import { LocalFirstRunControl } from "../LocalFirstRunControl";
import { MarketCapabilityStatus } from "../ui/MarketCapabilityStatus";

interface HkSearchResponse { items: HkSecurity[]; source?: string; updated_at?: string | null; last_attempt_at?: string | null; last_error?: string | null; refresh_enabled?: boolean; refresh_reason?: string | null; }
interface HkMarketPanelProps { onObserveStock: (code: string) => void; capability?: MarketCapability | null; capabilityLoading?: boolean; capabilityError?: string | null; }

export function HkMarketPanel({ onObserveStock, capability, capabilityLoading = false, capabilityError }: HkMarketPanelProps) {
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

  const refreshDisabled = refreshing || meta.refresh_enabled !== true;

  const refresh = async () => {
    if (refreshDisabled) return;
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
      <div>
        <h2>港股证券目录</h2>
        {meta.source ? <p className="data-provenance">
          数据源：{meta.source || "HKEX"}{meta.updated_at ? ` · 更新于 ${meta.updated_at}` : ""}
        </p> : null}
      </div>
      <LocalFirstRunControl
        action="更新目录"
        loading={refreshing}
        onRun={() => void refresh()}
        loadingLabel="更新中…"
        cacheRunLabel="更新目录"
        buttonClassName="run-btn hk-directory-refresh"
        ariaLabel="更新港股证券目录"
        ariaDescribedBy="hk-directory-capability-status"
        disabled={refreshDisabled}
      />
    </header>
    <MarketCapabilityStatus
      label="目录能力"
      capability={capability}
      loading={capabilityLoading || !capability}
      error={capabilityError}
      reasonOverride={meta.refresh_reason}
      unavailableLabel="未开放"
      separator="："
      showCoverage={false}
      plain
      id="hk-directory-capability-status"
      className="hk-directory-capability"
    />
    <div className="form-row inline stock-code-row observe-code-row">
      <label htmlFor="hk-stock-search">代码或名称</label>
      <input id="hk-stock-search" value={query} onChange={(event) => { setQuery(event.currentTarget.value); void search(event.currentTarget.value); }} placeholder="输入代码或名称" />
    </div>
    {meta.last_error ? <p className="data-stale-notice" role="status">目录最近刷新失败：{meta.last_error}；当前保留旧目录。</p> : null}
    {loading ? <PanelFeedback kind="loading" description="正在读取港股证券目录…" /> : error ? <PanelFeedback kind="error" title="港股目录读取失败" description={error} action={<button type="button" className="action-btn" onClick={() => void search()}>重试读取缓存</button>} /> : !items.length ? <PanelFeedback kind="empty" description={query ? "没有匹配的港股证券。" : "当前目录为空。"} /> : <div className="stock-list stock-mobile-list hk-security-list" role="list">
      {items.map((item) => {
        const verifiedOrdinaryShare = item.classification_status === "verified_ordinary_share";
        return <article className="stock-row hk-security-row" role="listitem" key={item.code}>
          <button type="button" className="hk-security-select" disabled={!verifiedOrdinaryShare} onClick={() => onObserveStock(`${item.code}.HK`)}>
            <span className="stock-title">
              <strong>{item.name}</strong>
              <span className="stock-code">{item.code}.HK · {item.currency} · 每手 {item.board_lot ?? "--"} 股</span>
            </span>
            <span className={`state-pill ${verifiedOrdinaryShare ? "neutral" : "warning"}`}>{verifiedOrdinaryShare ? "普通股已核验" : "分类未核验，暂不可观察"}</span>
          </button>
        </article>;
      })}
    </div>}
  </section>;
}
