import { useEffect, useState } from "react";
import type { HkFinancialMetric, HkFinancialResult, HkFinancialSnapshot } from "../types";
import { normalizeHkScreenFinancialSnapshot } from "../lib/contracts";
import { finiteHkNumber, formatHkDataTime, formatHkFinancialMetric, hkDataTimestamp } from "../lib/hkFormat";
import { getJson, postJson } from "../lib/tauri";

const additionalMetrics = [
  ["bps", "每股净资产"], ["diluted_eps", "摊薄每股收益"],
  ["revenue_yoy", "营业收入同比"], ["net_profit_yoy", "归母净利润同比"],
  ["gross_margin", "毛利率"], ["net_margin", "净利率"],
  ["asset_liability_ratio", "资产负债率"], ["current_ratio", "流动比率"],
  ["operating_cash_flow", "经营现金流"], ["operating_cash_flow_per_share", "每股经营现金流"],
] as const;

export function HkScreenFinancialDetails({ code, snapshot, initialError, onSnapshot }: { code: string; snapshot?: HkFinancialSnapshot; initialError?: string; onSnapshot?: (snapshot: HkFinancialSnapshot) => void }) {
  const [fetched, setFetched] = useState<HkFinancialSnapshot>();
  const [loading, setLoading] = useState(!snapshot && !initialError);
  const [error, setError] = useState<string | null>(snapshot?.field_errors?.financial || initialError || null);
  const financial = fetched && (!snapshot || (hkDataTimestamp(fetched.candidate_fetched_at) ?? 0) >= (hkDataTimestamp(snapshot.candidate_fetched_at) ?? 0)) ? fetched : snapshot;
  const profile = financial?.company_profile?.code === financial?.code ? financial?.company_profile : undefined;
  const coreFields: Array<[string, HkFinancialMetric | null | undefined, string]> = financial ? [
    ["每股收益", financial.eps, "currency/share"], ["净资产收益率", financial.roe, "percent"],
    ["营业总收入", financial.revenue, "currency_total"], ["归母净利润", financial.net_profit, "currency_total"],
  ] : [];
  const missingCore = coreFields.filter(([,metric,unit]) => !metric || finiteHkNumber(metric.value) == null || metric.unit !== unit || (unit !== "percent" && metric.currency !== financial?.currency)).map(([label]) => label);

  useEffect(() => {
    if (snapshot) { setLoading(false); setError(snapshot.field_errors?.financial || null); return; }
    if (initialError) { setLoading(false); setError(initialError); return; }
    let disposed = false;
    void (async () => {
      try {
        const cached = await getJson<HkFinancialResult>(`/api/hk/financial-status?code=${encodeURIComponent(code)}`);
        if (disposed) return;
        let nextError = cached.field_errors?.financial || null;
        setError(nextError);
        let next = normalizeHkScreenFinancialSnapshot(cached.snapshot, code);
        if (!next) {
          const refreshed = await postJson<HkFinancialResult>("/api/hk/financial-refresh", { code });
          next = normalizeHkScreenFinancialSnapshot(refreshed.snapshot, code);
          if (!next) throw new Error(refreshed.notes?.join("；") || "数据源暂未返回财务报表。");
          nextError = refreshed.field_errors?.financial || null;
        }
        if (!disposed) {
          setFetched(next);
          setError(nextError);
          onSnapshot?.({...next, field_errors: {...next.field_errors, ...(nextError ? {financial:nextError} : {})}});
        }
      } catch (reason) {
        if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        if (!disposed) setLoading(false);
      }
    })();
    return () => { disposed = true; };
  }, [code, snapshot, initialError, onSnapshot]);

  const refresh = async () => {
    if (loading) return;
    setLoading(true);
    setError(null);
    try {
      const result = await postJson<HkFinancialResult>("/api/hk/financial-refresh", { code });
      const next = normalizeHkScreenFinancialSnapshot(result.snapshot, code);
      if (!next) throw new Error(result.notes?.join("；") || "数据源暂未返回财务报表。");
      setFetched(next);
      onSnapshot?.({...next, field_errors: {...next.field_errors, ...(result.field_errors?.financial ? {financial:result.field_errors.financial} : {})}});
      setError(result.field_errors?.financial || null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally { setLoading(false); }
  };

  const metrics: Array<[string, HkFinancialMetric | null | undefined]> = financial ? [
    ["每股收益", financial.eps], ["营业总收入", financial.revenue], ["归母净利润", financial.net_profit], ["净资产收益率", financial.roe],
    ...additionalMetrics.map(([key, label]): [string, HkFinancialMetric | undefined] => [label, financial.additional_metrics?.[key]]),
    ["股息率", financial.dividend_yield], ["总股本", financial.total_shares], ["流通股", financial.circulating_shares],
  ] : [];

  return <section className="stock-hk-financial-details" aria-label={`${code}财务数据`}>
    <header>
      <h4>财务数据{financial ? ` · ${financial.normalized_period}` : ""}</h4>
      <button type="button" className="stock-row-action" disabled={loading} onClick={() => void refresh()} aria-label={`刷新 ${code} 财务数据`}>{loading ? "读取中…" : "刷新财务"}</button>
    </header>
    {financial ? <>
      {profile ? <>
        <p>公司：{profile.name}{profile.industry ? ` · 行业：${profile.industry}` : ""}</p>
        {profile.business_summary ? <p>主营业务：{profile.business_summary}</p> : null}
        <p className="data-provenance">公司资料来源：{profile.source} · 抓取 {formatHkDataTime(profile.fetched_at)}</p>
      </> : null}
      <p className="data-provenance">报表截止 {financial.report_date || "暂无"}{financial.disclosed_at ? ` · 披露 ${financial.disclosed_at}` : ""} · 报表币种 {financial.currency}</p>
      <p className={missingCore.length ? "stock-risk-summary" : "data-provenance"}>{missingCore.length ? `财务缺项：${missingCore.join("、")} · ${4 - missingCore.length}/4` : "核心报表字段齐全 · 4/4"}</p>
      <dl className="stock-detail-metrics stock-hk-financial-grid">
        {metrics.map(([label, metric]) => <div key={label}><dt>{label}</dt><dd>{formatHkFinancialMetric(metric)}</dd></div>)}
      </dl>
      <p className="data-provenance">来源：{financial.candidate_source} · 抓取 {formatHkDataTime(financial.candidate_fetched_at)} · {financial.verification_status === "conflict" ? "核验存在冲突" : financial.verification_status === "verified" ? "已按官方披露核验" : "来源候选值未核验"}</p>
    </> : loading ? <p role="status">正在读取财务数据…</p> : <p>暂未获取财务报表，刷新后显示数据。</p>}
    {error && !loading ? <p className="stock-risk-summary" role="status">财务获取未完成：{error}</p> : null}
  </section>;
}
