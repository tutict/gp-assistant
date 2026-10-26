import { useEffect, useState } from "react";
import { getJson, postJson } from "../../lib/tauri";
import { normalizeStockCode } from "../../lib/format";
import { parseSecurityId, type ObservationMetadata } from "../../lib/marketDomain";
import type { HkFinancialMetric, HkFinancialResult, HkFinancialSnapshot, HkFxRate, HkObserveResult, WatchlistItem } from "../../types";
import { PanelFeedback } from "../ui/PanelFeedback";
import { ObservationPanelFrame } from "./ObservationPanelFrame";

interface HkObservePanelProps { code: string; watchlist: WatchlistItem[]; onWatchlistChange: (items: WatchlistItem[]) => void; }
const FINANCIAL_METRIC_FIELDS = new Set(["revenue", "net_profit", "eps", "roe", "pe", "pb", "dividend_yield", "market_cap", "total_shares", "circulating_shares"]);

export function HkObservePanel({ code: rawCode, watchlist, onWatchlistChange }: HkObservePanelProps) {
  const code = normalizeStockCode(rawCode);
  const [result, setResult] = useState<HkObserveResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [financialLoading, setFinancialLoading] = useState(false);
  const load = async (policy: "cache_only" | "refresh" = "cache_only") => {
    setLoading(true); setError(null);
    const requestId = policy === "refresh" ? `hk-observe-${globalThis.crypto?.randomUUID?.() || Date.now()}` : "";
    const query = new URLSearchParams({ data_policy: policy, ...(requestId ? { request_id: requestId } : {}) });
    try { setResult(await getJson<HkObserveResult>(`/api/hk/observe/${encodeURIComponent(code)}?${query.toString()}`)); }
    catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      if (result) {
        setResult((previous) => previous ? {
          ...previous,
          stale: true,
          notes: [...previous.notes, message],
          refresh_error: policy === "refresh" ? message : previous.refresh_error,
        } : previous);
      } else {
        setError(message);
      }
    }
    finally { setLoading(false); }
  };
  useEffect(() => { if (code.endsWith(".HK")) void load("cache_only"); else { setLoading(false); setError("请输入有效港股代码。"); } }, [code]);
  const saved = watchlist.some((item) => normalizeStockCode(item.code) === code);
  const toggle = () => saved ? onWatchlistChange(watchlist.filter((item) => normalizeStockCode(item.code) !== code)) : onWatchlistChange([{ code, name: result?.security.name, industry: "港股", added_at: new Date().toISOString(), source: "hk-market" }, ...watchlist]);
  const refreshFinancial = async () => {
    setFinancialLoading(true);
    try {
      const next = await postJson<HkFinancialResult>("/api/hk/financial-refresh", { code });
      setResult((previous) => previous ? {
        ...previous,
        financial: next.snapshot,
        fx_rate: next.fx_rate,
        official_disclosure: next.official_disclosure,
        candidate_updated_at: next.candidate_updated_at,
        fx_updated_at: next.fx_updated_at,
        official_checked_at: next.official_checked_at,
        financial_notes: next.notes,
        financial_field_errors: next.field_errors,
        financial_stale: next.stale,
      } : previous);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setResult((previous) => previous ? {
        ...previous,
        financial_stale: true,
        financial_notes: [...(previous.financial_notes || []), message],
        financial_field_errors: { ...previous.financial_field_errors, financial: message },
      } : previous);
    } finally { setFinancialLoading(false); }
  };
  if (loading) return <PanelFeedback kind="loading" description="正在加载港股报价与日 K…" />;
  if (error) return <PanelFeedback kind="error" title="港股观察失败" description={error} action={<button type="button" className="action-btn" onClick={() => void load("refresh")}>重试</button>} />;
  if (!result) return <PanelFeedback kind="empty" description="没有港股观察数据。" />;
  if (result.security.classification_status !== "verified_ordinary_share") return <PanelFeedback kind="error" title="港股普通股分类未核验" description="当前证券目录无法证明该证券属于普通股，因此不会进入观察。" />;
  const security = parseSecurityId("hk", result.security.code, "ordinary_share");
  if (!security) return <PanelFeedback kind="error" title="港股证券身份无效" description="观察结果缺少规范的五位证券代码，无法创建证券身份。" />;
  return <ObservationPanelFrame
    market="hk"
    title={result.security.name}
    security={security}
    provenance={hkObservationProvenance(result)}
    capability={result.quote_capability}
    issues={[...result.notes, result.quote_error || "", result.history_error || "", result.refresh_error ? `刷新：${result.refresh_error}` : "", result.provider_readiness_error || "", ...(result.financial_notes || [])]}
    actions={<div className="hk-observe-actions">
      <button type="button" className="action-btn" onClick={() => void load("refresh")}>刷新行情</button>
      <button type="button" className="action-btn" onClick={toggle}>{saved ? "移出自选" : "加入自选"}</button>
    </div>}
    className="hk-observe-panel"
  >
    {result.stale ? <p className="data-stale-notice">当前显示缓存或不完整数据：{[...result.notes, result.quote_error || "", result.history_error || "", result.refresh_error ? `刷新：${result.refresh_error}` : ""].filter(Boolean).join("；") || "请稍后刷新。"}</p> : null}
    <div className="hk-quote-grid"><div><span>现价</span><strong>{result.quote?.currency || result.security.currency} {result.quote?.price?.toFixed(3) || "--"}</strong></div><div><span>涨跌幅</span><strong>{result.quote?.change_pct == null ? "--" : `${(result.quote.change_pct * 100).toFixed(2)}%`}</strong></div><div><span>成交量</span><strong>{result.quote?.volume_shares?.toLocaleString() || "--"} 股</strong></div><div><span>成交额</span><strong>{result.quote?.turnover_hkd?.toLocaleString(undefined, { maximumFractionDigits: 0 }) || "--"} {result.quote?.currency || result.security.currency}</strong></div></div>
    <HkFinancialSnapshotView
      snapshot={result.financial}
      fx={result.fx_rate}
      disclosure={result.official_disclosure}
      candidateUpdatedAt={result.candidate_updated_at}
      fxUpdatedAt={result.fx_updated_at}
      officialCheckedAt={result.official_checked_at}
      stale={result.financial_stale ?? !result.financial}
      notes={result.financial_notes || []}
      fieldErrors={result.financial_field_errors || result.financial?.field_errors || {}}
      loading={financialLoading}
      onRefresh={() => void refreshFinancial()}
    />
    <div className="hk-daily-table-wrap"><table className="hk-daily-table"><thead><tr><th>日期</th><th>开</th><th>高</th><th>低</th><th>收</th><th>成交量（股）</th></tr></thead><tbody>{result.history.slice(-120).reverse().map((bar) => <tr key={bar.date}><td>{bar.date}</td><td>{bar.open.toFixed(3)}</td><td>{bar.high.toFixed(3)}</td><td>{bar.low.toFixed(3)}</td><td>{bar.close.toFixed(3)}</td><td>{bar.volume_shares?.toLocaleString() || "--"}</td></tr>)}</tbody></table></div>
  </ObservationPanelFrame>;
}

function hkObservationProvenance(result: HkObserveResult): ObservationMetadata[] {
  const records: ObservationMetadata[] = [];
  const append = (item: ObservationMetadata) => records.push(item);
  if (result.quote) {
    append({ source_id: result.quote.source, fetched_at: result.quote.fetched_at || null, observed_at: result.quote.quote_time || null, disclosed_at: null, report_period: null, verification_status: "unverified", quality_flags: [], missing_reason: null });
  }
  if (result.history.length) {
    append({ source_id: result.history_source || "Tencent HK history", fetched_at: result.history_updated_at || null, observed_at: result.history.at(-1)?.date || null, disclosed_at: null, report_period: null, verification_status: "unverified", quality_flags: [], missing_reason: result.history_source ? null : "来源标识未包含在观察响应中" });
  }
  if (result.financial) {
    append({ source_id: result.financial.candidate_source, fetched_at: result.candidate_updated_at || result.financial.candidate_fetched_at, observed_at: null, disclosed_at: result.financial.disclosed_at || null, report_period: result.financial.raw_report_period, verification_status: result.financial.verification_status, quality_flags: result.financial.verification_conflicts || [], missing_reason: null });
  }
  if (result.fx_rate) {
    append({ source_id: result.fx_rate.source, fetched_at: result.fx_updated_at || result.fx_rate.fetched_at, observed_at: result.fx_rate.observed_at, disclosed_at: null, report_period: null, verification_status: "unverified", quality_flags: ["daily_reference_cross_rate"], missing_reason: null });
  }
  if (result.official_disclosure) {
    append({ source_id: result.official_disclosure.source_name, fetched_at: result.official_checked_at || result.official_disclosure.checked_at, observed_at: null, disclosed_at: result.official_disclosure.disclosed_at || null, report_period: result.official_disclosure.report_period || null, verification_status: result.official_disclosure.status, quality_flags: ["metadata_only"], missing_reason: result.official_disclosure.error || null });
  }
  return records;
}

function HkFinancialSnapshotView({
  snapshot,
  fx,
  disclosure,
  candidateUpdatedAt,
  fxUpdatedAt,
  officialCheckedAt,
  stale,
  notes,
  fieldErrors,
  loading,
  onRefresh,
}: {
  snapshot?: HkFinancialSnapshot | null;
  fx?: HkFxRate | null;
  disclosure?: HkObserveResult["official_disclosure"];
  candidateUpdatedAt?: string | null;
  fxUpdatedAt?: string | null;
  officialCheckedAt?: string | null;
  stale: boolean;
  notes: string[];
  fieldErrors: Record<string, string>;
  loading: boolean;
  onRefresh: () => void;
}) {
  const showCurrency = (metric?: HkFinancialMetric | null) => {
    if (metric?.value == null) return "暂无";
    const amountPerShare = metric.unit === "currency/share";
    if (metric.unit !== "currency_total" && !amountPerShare) {
      if (metric.unit === "percent") return `${formatMetricNumber(metric.value)}%`;
      if (metric.unit === "shares") return `${formatMetricNumber(metric.value)} 股`;
      return formatMetricNumber(metric.value);
    }
    const suffix = amountPerShare ? "/股" : "";
    if (!["HKD", "CNY"].includes(metric.currency)) return `${metric.currency || "原币"} ${formatMetricNumber(metric.value)}${suffix}`;
    if (!fx || fx.rate <= 0) return `${metric.currency} ${formatMetricNumber(metric.value)}${suffix}`;
    const hkd = metric.currency === "HKD" ? metric.value : metric.value / fx.rate;
    const cny = metric.currency === "CNY" ? metric.value : metric.value * fx.rate;
    return `HKD ${formatMetricNumber(hkd)}${suffix} · CNY ${formatMetricNumber(cny)}${suffix}`;
  };
  const metadataStatusLabel = officialMetadataStatusLabel(disclosure?.status || (snapshot?.official_source_url ? "metadata_only" : "unavailable"));
  const verificationLabel = financialVerificationLabel(snapshot?.verification_status);
  const errorEntries = Object.entries(fieldErrors).filter(([field]) => !FINANCIAL_METRIC_FIELDS.has(field));
  const visibleNotes = notes.filter((note) => !Object.values(fieldErrors).some((error) => error && note.includes(error)));

  return <section className="hk-financial-panel" aria-label="港股财务快照">
    <header className="hk-financial-heading">
      <div>
        <h3>财务快照</h3>
        {snapshot ? <p>{snapshot.raw_report_period} · {snapshot.normalized_period}</p> : <p>尚无财务缓存</p>}
      </div>
      <button type="button" className="action-btn" onClick={onRefresh} disabled={loading}>{loading ? "刷新中…" : "刷新财务"}</button>
    </header>
    {stale ? <p className="data-stale-notice">财务数据有缺失或本次刷新失败；已有缓存会继续显示，可点击“刷新财务”重试。</p> : null}
    {snapshot ? <>
      <div className="hk-financial-period">
        <span>原始报告期：{snapshot.raw_report_period}</span>
        <span>归一化标签：{snapshot.normalized_period}</span>
        <span>报告日期：{snapshot.report_date || "未知"}</span>
        <span>披露日期：{snapshot.disclosed_at || "未知"}</span>
      </div>
      <p className="hk-financial-meta">候选来源：{snapshot.candidate_source} · 更新时间：{candidateUpdatedAt || snapshot.candidate_fetched_at}</p>
    </> : <p className="hk-financial-meta">首次进入只读缓存；授权门槛开放前，刷新仅尝试 ECB 日汇率，Eastmoney 候选财务与 HKEXnews 查询保持关闭。</p>}
    <p className="hk-financial-meta">
      汇率：{fx ? `1 HKD = ${formatMetricNumber(fx.rate)} CNY（${fx.source}；观测 ${fx.observed_at}；更新 ${fxUpdatedAt || fx.fetched_at}）` : "不可用，仅显示原币"}
      {fx?.last_error ? ` · 最近失败：${fx.last_error}` : ""}
    </p>
    {snapshot ? <div className="hk-financial-grid">
      <Metric label="营业收入" metric={snapshot.revenue} value={showCurrency(snapshot.revenue)} />
      <Metric label="归母净利润" metric={snapshot.net_profit} value={showCurrency(snapshot.net_profit)} />
      <Metric label="EPS" metric={snapshot.eps} value={showCurrency(snapshot.eps)} />
      <Metric label="ROE" metric={snapshot.roe} value={showCurrency(snapshot.roe)} />
      <Metric label="PE" metric={snapshot.pe} value={showCurrency(snapshot.pe)} />
      <Metric label="PB" metric={snapshot.pb} value={showCurrency(snapshot.pb)} />
      <Metric label="股息率" metric={snapshot.dividend_yield} value={showCurrency(snapshot.dividend_yield)} />
      <Metric label="总市值" metric={snapshot.market_cap} value={showCurrency(snapshot.market_cap)} />
      <Metric label="总股本" metric={snapshot.total_shares} value={showCurrency(snapshot.total_shares)} />
      <Metric label="流通股本" metric={snapshot.circulating_shares} value={showCurrency(snapshot.circulating_shares)} />
    </div> : null}
    {errorEntries.length ? <ul className="hk-financial-errors">{errorEntries.map(([field, message]) => <li key={field}><strong>{financialFieldLabel(field)}：</strong>{message}</li>)}</ul> : null}
    {visibleNotes.map((note, index) => <p className="hk-financial-error" key={`${note}-${index}`}>{note}</p>)}
    {snapshot?.verification_status === "conflict" ? <section className="hk-financial-conflicts" aria-label="官方披露数值冲突">
      <strong>候选值与官方记录存在差异</strong>
      {Object.entries(snapshot.official_values || {}).map(([field, metric]) => <p key={field}>{financialFieldLabel(field)}：候选 {showCurrency(snapshot[field as keyof HkFinancialSnapshot] as HkFinancialMetric)} · 官方 {showCurrency(metric)}</p>)}
    </section> : null}
    <div className="hk-financial-source">
      <p>官方披露：{disclosure?.source_name || snapshot?.official_source_name || "HKEXnews"} · 元数据状态：{metadataStatusLabel} · 数值校验：{verificationLabel} · 检查时间：{officialCheckedAt || disclosure?.checked_at || "未知"}</p>
      {disclosure?.source_url || snapshot?.official_source_url ? <a href={disclosure?.source_url || snapshot?.official_source_url || undefined} target="_blank" rel="noreferrer">打开官方公告查询入口</a> : <span>未找到匹配的官方披露链接。</span>}
      <p>未解析公告正文；元数据状态不代表财务数值已核验。</p>
    </div>
  </section>;
}

function formatMetricNumber(value: number): string {
  return value.toLocaleString(undefined, { maximumFractionDigits: 2 });
}

function financialVerificationLabel(status?: string): string {
  switch (status) {
    case "verified": return "已按可比官方字段核验";
    case "conflict": return "候选值与官方值冲突";
    case "metadata_only": return "仅有官方披露元数据，数值未核验";
    case "unavailable": return "官方披露元数据不可用";
    default: return "未核验";
  }
}

function officialMetadataStatusLabel(status?: string): string {
  switch (status) {
    case "metadata_only": return "已找到官方披露查询入口";
    case "unavailable": return "官方披露元数据不可用";
    default: return "未找到匹配的官方披露元数据";
  }
}

function financialFieldLabel(field: string): string {
  const labels: Record<string, string> = {
    financial: "候选财务",
    fx_rate: "汇率",
    official_disclosure: "官方披露",
    revenue: "营业收入",
    net_profit: "归母净利润",
    eps: "EPS",
    roe: "ROE",
    pe: "PE",
    pb: "PB",
    dividend_yield: "股息率",
    market_cap: "总市值",
    total_shares: "总股本",
    circulating_shares: "流通股本",
  };
  return labels[field] || field;
}

function Metric({ label, metric, value }: { label: string; metric?: HkFinancialMetric | null; value: string }) {
  return <div className="hk-financial-metric">
    <span>{label}</span>
    <strong>{value}</strong>
    {metric?.source ? <small>来源：{metric.source}</small> : null}
    {metric?.reason ? <small className="hk-financial-error">缺失原因：{metric.reason}</small> : null}
  </div>;
}
