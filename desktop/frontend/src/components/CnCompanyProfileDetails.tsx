import { useEffect, useRef, useState } from "react";
import type { CnCompanyProfileResult } from "../types";
import { formatDateTime } from "../lib/format";
import { postJson } from "../lib/tauri";

function companyResult(value: unknown, code: string): CnCompanyProfileResult {
  const result = value as Partial<CnCompanyProfileResult> | null;
  const profile = result?.profile;
  if (!result || result.code !== code || typeof result.stale !== "boolean"
    || (result.error !== null && typeof result.error !== "string")
    || (profile !== null && (!profile || profile.code !== code
      || !["name", "industry", "business_summary", "company_summary", "source", "fetched_at"]
        .every(key => typeof profile[key as keyof typeof profile] === "string")
      || !profile.name.trim() || !profile.source.trim()
      || (!profile.business_summary.trim() && !profile.company_summary.trim())
      || !Number.isSafeInteger(Number(profile.fetched_at)) || Number(profile.fetched_at) <= 0
      || !Number.isFinite(new Date(Number(profile.fetched_at)).getTime())))) {
    throw new Error("未返回匹配当前股票的公司资料");
  }
  return result as CnCompanyProfileResult;
}

async function readCompany(code: string, refresh: boolean, signal?: AbortSignal) {
  return companyResult(await postJson<unknown>("/api/cn/company-profile", { code, refresh }, { timeoutMs: 16000, signal }), code);
}

export function CnCompanyProfileDetails({ code, initialResult, onResult }: {
  code: string;
  initialResult?: CnCompanyProfileResult;
  onResult: (result: CnCompanyProfileResult) => void;
}) {
  const initial = initialResult?.code === code ? initialResult : undefined;
  const [result, setResult] = useState(initial);
  const [loading, setLoading] = useState(!initial);
  const activeRequest = useRef<AbortController | null>(null);
  const profile = result?.code === code ? result.profile : null;
  const error = result?.code === code ? result.error : null;

  useEffect(() => {
    if (initial) {
      setResult(initial);
      setLoading(false);
    } else {
      const controller = new AbortController();
      activeRequest.current = controller;
      setLoading(true);
      void (async () => {
        let next: CnCompanyProfileResult;
        try {
          next = await readCompany(code, false, controller.signal);
        } catch (reason) {
          next = { code, profile: null, stale: false, error: reason instanceof Error ? reason.message : String(reason) };
        }
        if (!controller.signal.aborted && activeRequest.current === controller) {
          setResult(next);
          setLoading(false);
          onResult(next);
        }
      })();
    }
    return () => { activeRequest.current?.abort(); activeRequest.current = null; };
  }, [code, initial, onResult]);

  const refresh = async () => {
    if (loading) return;
    const controller = new AbortController();
    activeRequest.current?.abort();
    activeRequest.current = controller;
    setLoading(true);
    let next: CnCompanyProfileResult;
    try {
      next = await readCompany(code, true, controller.signal);
      if (!next.profile && profile) {
        next = { ...next, profile, stale: true, error: next.error || "数据源暂未返回公司资料。" };
      }
    } catch (reason) {
      next = { code, profile: profile || null, stale: Boolean(profile), error: reason instanceof Error ? reason.message : String(reason) };
    }
    if (controller.signal.aborted || activeRequest.current !== controller) return;
    setResult(next);
    setLoading(false);
    onResult(next);
  };

  return <section className="stock-company-profile" aria-label={`${code}公司资料`}>
    <header>
      <h4>公司资料</h4>
      <button type="button" className="stock-row-action" disabled={loading} onClick={() => void refresh()} aria-label={`刷新 ${code} 公司资料`}>{loading ? "读取中…" : "刷新公司资料"}</button>
    </header>
    {profile ? <>
      <p>公司：{profile.name}{profile.industry ? ` · 行业：${profile.industry}` : ""}</p>
      {profile.company_summary && profile.company_summary !== profile.business_summary ? <p>公司简介：{profile.company_summary}</p> : null}
      {profile.business_summary ? <p>主营业务：{profile.business_summary}</p> : null}
      <p className="data-provenance">公司资料来源：{profile.source} · 抓取 {formatDateTime(new Date(Number(profile.fetched_at)).toISOString())}{result?.stale ? " · 本地缓存" : ""}</p>
    </> : <p role={loading ? "status" : undefined}>{loading ? "正在读取公司资料…" : "暂未获取公司资料，可刷新重试。"}</p>}
    {error && !loading ? <p className="stock-risk-summary" role="status">公司资料获取未完成：{error}</p> : null}
  </section>;
}
