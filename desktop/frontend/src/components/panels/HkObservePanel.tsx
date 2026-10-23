import { useEffect, useState } from "react";
import { getJson } from "../../lib/tauri";
import { normalizeStockCode } from "../../lib/format";
import type { HkObserveResult, WatchlistItem } from "../../types";
import { PanelFeedback } from "../ui/PanelFeedback";

interface HkObservePanelProps { code: string; watchlist: WatchlistItem[]; onWatchlistChange: (items: WatchlistItem[]) => void; }

export function HkObservePanel({ code: rawCode, watchlist, onWatchlistChange }: HkObservePanelProps) {
  const code = normalizeStockCode(rawCode);
  const [result, setResult] = useState<HkObserveResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const load = async (policy: "cache_only" | "refresh" = "cache_only") => {
    setLoading(true); setError(null);
    try { setResult(await getJson<HkObserveResult>(`/api/hk/observe/${encodeURIComponent(code)}?data_policy=${policy}`)); }
    catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      if (result) {
        setResult((previous) => previous ? { ...previous, stale: true, notes: [...previous.notes, message], quote_error: policy === "refresh" ? message : previous.quote_error } : previous);
      } else {
        setError(message);
      }
    }
    finally { setLoading(false); }
  };
  useEffect(() => { if (code.endsWith(".HK")) void load("cache_only"); else { setLoading(false); setError("请输入有效港股代码。"); } }, [code]);
  const saved = watchlist.some((item) => normalizeStockCode(item.code) === code);
  const toggle = () => saved ? onWatchlistChange(watchlist.filter((item) => normalizeStockCode(item.code) !== code)) : onWatchlistChange([{ code, name: result?.security.name, industry: "港股", added_at: new Date().toISOString(), source: "hk-market" }, ...watchlist]);
  if (loading) return <PanelFeedback kind="loading" description="正在加载港股报价与日 K…" />;
  if (error) return <PanelFeedback kind="error" title="港股观察失败" description={error} action={<button type="button" className="action-btn" onClick={() => void load("refresh")}>重试</button>} />;
  if (!result) return <PanelFeedback kind="empty" description="没有港股观察数据。" />;
  return <section className="panel-container hk-observe-panel" aria-label="港股观察">
    <header className="panel-heading"><div><span className="eyebrow">港股观察</span><h2>{result.security.name}（{result.security.code}.HK）</h2><p>来源：{result.quote?.source || "Tencent Finance"} · 报价时间：{result.quote?.quote_time || "未知"}</p><p>报价更新时间：{result.quote_updated_at || "未知"} · 日 K 更新时间：{result.history_updated_at || "未知"}</p></div><div className="hk-observe-actions"><button type="button" className="action-btn" onClick={() => void load("refresh")}>刷新行情</button><button type="button" className="action-btn" onClick={toggle}>{saved ? "移出自选" : "加入自选"}</button></div></header>
    {result.stale ? <p className="data-stale-notice">当前显示缓存或不完整数据：{[...result.notes, result.quote_error || "", result.history_error || ""].filter(Boolean).join("；") || "请稍后刷新。"}</p> : null}
    <div className="hk-quote-grid"><div><span>现价</span><strong>{result.quote?.currency || result.security.currency} {result.quote?.price?.toFixed(3) || "--"}</strong></div><div><span>涨跌幅</span><strong>{result.quote?.change_pct == null ? "--" : `${(result.quote.change_pct * 100).toFixed(2)}%`}</strong></div><div><span>成交量</span><strong>{result.quote?.volume_shares?.toLocaleString() || "--"} 股</strong></div><div><span>成交额</span><strong>{result.quote?.turnover_hkd?.toLocaleString(undefined, { maximumFractionDigits: 0 }) || "--"} {result.quote?.currency || result.security.currency}</strong></div></div>
    <p className="data-provenance" role="status" aria-label="港股报价能力">
      报价能力：{quoteCapabilityStatusLabel(result.quote_capability.status)}
      {result.quote_capability.reason ? ` · ${result.quote_capability.reason}` : ""}
    </p>
    <div className="hk-daily-table-wrap"><table className="hk-daily-table"><thead><tr><th>日期</th><th>开</th><th>高</th><th>低</th><th>收</th><th>成交量（股）</th></tr></thead><tbody>{result.history.slice(-120).reverse().map((bar) => <tr key={bar.date}><td>{bar.date}</td><td>{bar.open.toFixed(3)}</td><td>{bar.high.toFixed(3)}</td><td>{bar.low.toFixed(3)}</td><td>{bar.close.toFixed(3)}</td><td>{bar.volume_shares?.toLocaleString() || "--"}</td></tr>)}</tbody></table></div>
  </section>;
}
function quoteCapabilityStatusLabel(status: HkObserveResult["quote_capability"]["status"]): string {
  switch (status) {
    case "available": return "可用";
    case "limited": return "部分可用";
    case "unavailable": return "不可用";
  }
}
