import { useEffect, useState } from "react";
import { getJson, postJson } from "../../lib/tauri";
import type { MarketCapability } from "../../lib/marketDomain";
import type { HkSecurity } from "../../types";
import { PanelFeedback } from "../ui/PanelFeedback";
import { LocalFirstRunControl } from "../LocalFirstRunControl";
import { MarketCapabilityStatus } from "../ui/MarketCapabilityStatus";

interface HkQuoteRefreshSummary { requested_count: number; attempted_count: number; updated_count: number; failed_count: number; unsupported_currency_count: number; error_samples?: string[]; }
interface HkSearchResponse { items: HkSecurity[]; source?: string; updated_at?: string | null; last_attempt_at?: string | null; last_error?: string | null; refresh_enabled?: boolean; refresh_reason?: string | null; }
interface HkMarketPanelProps { onObserveStock: (code: string) => void; onClassificationChange?: () => void; capability?: MarketCapability | null; capabilityLoading?: boolean; capabilityError?: string | null; }

export function HkMarketPanel({ onObserveStock, onClassificationChange, capability, capabilityLoading = false, capabilityError }: HkMarketPanelProps) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<HkSecurity[]>([]);
  const [meta, setMeta] = useState<HkSearchResponse>({ items: [] });
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [confirmingCode, setConfirmingCode] = useState<string | null>(null);
  const [quoteRefresh, setQuoteRefresh] = useState<HkQuoteRefreshSummary | null>(null);
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
      const response = await getJson<HkSearchResponse>(`/api/hk/stock-search?q=${encodeURIComponent(nextQuery.trim())}&limit=50`);
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
    try {
      const result = await postJson<{ quote_refresh?: HkQuoteRefreshSummary }>("/api/hk/refresh");
      setQuoteRefresh(result.quote_refresh || null);
      await search();
    }
    catch (reason) {
      preserveCacheAfterReadFailure(reason);
    }
    finally { setRefreshing(false); }
  };
  const confirmOrdinaryShare = async (code: string) => {
    setConfirmingCode(code);
    setError(null);
    try {
      await postJson("/api/hk/confirm-ordinary-share", { code: `${code}.HK` });
      await search();
      onClassificationChange?.();
    } catch (reason) {
      preserveCacheAfterReadFailure(reason);
    } finally {
      setConfirmingCode(null);
    }
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
        action="刷新目录与行情"
        loading={refreshing}
        onRun={() => void refresh()}
        loadingLabel="更新中…"
        cacheRunLabel="刷新目录与行情"
        buttonClassName="run-btn hk-directory-refresh"
        ariaLabel="刷新港股证券目录与批量行情"
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
      <input id="hk-stock-search" value={query} onChange={(event) => { setQuery(event.currentTarget.value); void search(event.currentTarget.value); }} placeholder="输入港股代码或中文名" />
    </div>
    <p className="hk-classification-note">HKEX 免费目录会自动开放其“Equity Securities”上市股权。目录未进一步区分普通股与优先股；这类证券会标为“HKEX 上市股权”，不会标成普通股已核验。需要时仍可在本机手动确认。</p>
    {quoteRefresh ? <p className="hk-quote-refresh-summary" role="status">
      批量行情：更新 {quoteRefresh.updated_count}/{quoteRefresh.attempted_count} 只，失败 {quoteRefresh.failed_count} 只，非 HKD 证券暂未支持 {quoteRefresh.unsupported_currency_count} 只。
      {quoteRefresh.error_samples?.length ? ` 示例：${quoteRefresh.error_samples.join("；")}` : ""}
    </p> : null}
    {meta.last_error ? <p className="data-stale-notice" role="status">目录最近刷新失败：{meta.last_error}；当前保留旧目录。</p> : null}
    {loading ? <PanelFeedback kind="loading" description="正在读取港股证券目录…" /> : error ? <PanelFeedback kind="error" title="港股目录读取失败" description={error} action={<button type="button" className="action-btn" onClick={() => void search()}>重试读取缓存</button>} /> : !query.trim() ? <PanelFeedback kind="empty" description="输入代码或中文名后显示匹配证券。" /> : !items.length ? <PanelFeedback kind="empty" description="没有匹配的港股证券。" /> : <div className="stock-list stock-mobile-list hk-security-list" role="list">
      {items.map((item) => {
        const sourceVerified = item.classification_status === "verified_ordinary_share";
        const userConfirmed = item.classification_status === "user_confirmed_ordinary_share";
        const exchangeListedEquity = item.classification_status === "exchange_listed_equity";
        const ordinaryShareAccepted = sourceVerified || userConfirmed || exchangeListedEquity;
        const quoteSupported = item.currency.toUpperCase() === "HKD";
        const canObserve = ordinaryShareAccepted && quoteSupported;
        return <article className="stock-row hk-security-row" role="listitem" key={item.code}>
          <button type="button" className="hk-security-select" disabled={!canObserve} onClick={() => onObserveStock(`${item.code}.HK`)}>
            <span className="stock-title">
              <strong>{item.name_zh || item.name}</strong>
              <span className="stock-code">{item.code}.HK · {item.currency} · 每手 {item.board_lot ?? "--"} 股</span>
            </span>
            <span className={`state-pill ${sourceVerified || exchangeListedEquity ? "neutral" : userConfirmed ? "positive" : "warning"}`}>
              {!quoteSupported
                ? `${exchangeListedEquity ? "HKEX 上市股权" : userConfirmed ? "已由用户确认普通股" : sourceVerified ? "来源已核验普通股" : "分类未核验"} · 当前仅支持 HKD 报价`
                : exchangeListedEquity ? "HKEX 上市股权" : sourceVerified ? "来源已核验普通股" : userConfirmed ? "已由用户确认普通股" : "分类未核验，暂不可观察"}
            </span>
          </button>
          {!ordinaryShareAccepted && <button
            type="button"
            className="action-btn hk-confirm-ordinary"
            disabled={confirmingCode === item.code}
            onClick={() => void confirmOrdinaryShare(item.code)}
            aria-label={`确认 ${item.code}.HK 为普通股`}
          >{confirmingCode === item.code ? "保存中…" : "确认普通股"}</button>}
        </article>;
      })}
    </div>}
  </section>;
}
