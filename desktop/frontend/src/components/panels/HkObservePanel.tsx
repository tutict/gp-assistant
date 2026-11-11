import { useCallback, useEffect, useRef, useState } from "react";
import { getJson, postJson } from "../../lib/tauri";
import { normalizeStockCode } from "../../lib/format";
import type { MarketCapability } from "../../lib/marketDomain";
import type { HkFinancialResult, HkObserveResult, HkQuarterlyUpdate, WatchlistItem } from "../../types";
import { StockCodeInput } from "../StockCodeInput";
import { LocalFirstRunControl } from "../LocalFirstRunControl";
import { PanelFeedback } from "../ui/PanelFeedback";
import { ObserveResultView } from "./ObservePanel";
import { toHkSharedObserveResult } from "./hkObserveAdapter";
import { HkStockInformation } from "./HkStockInformation";
import { formatHkDataTime, hkDataTimestamp } from "../../lib/hkFormat";

const MIN_HK_OBSERVE_HISTORY_BARS = 25;

interface HkObservePanelProps {
  code: string;
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  mobileRuntime?: boolean;
  onOpenNews?: (code: string) => void;
  onAskAgent?: (prompt: string) => void;
  capability?: MarketCapability;
  onMarketDataChanged?: () => void;
}

function normalizeHkCode(value: string): string {
  const normalized = normalizeStockCode(value, "HK");
  return normalized.endsWith(".HK") ? normalized : "";
}

function hasChineseName(value: string | null | undefined): boolean {
  return Boolean(value && /\p{Script=Han}/u.test(value));
}

function hkFinancialMeta(result: HkObserveResult, loading: boolean): string {
  if (loading) return "正在更新财务报表与季度经营披露，当前先展示缓存…";
  const financial = result.financial;
  if (!financial) return result.financial_notes?.join(" · ") || "尚无港股财务缓存，正在尝试获取。";

  const metadata = [
    financial.candidate_source || "港股财务源",
    financial.normalized_period || "报告期未知",
    financial.report_date ? `报告 ${financial.report_date}` : "",
    financial.disclosed_at ? `披露 ${financial.disclosed_at}` : "",
    result.candidate_updated_at || financial.candidate_fetched_at
      ? `主源抓取 ${formatHkDataTime(result.candidate_updated_at || financial.candidate_fetched_at)}`
      : "",
    financial.supplemental_source
      ? `${financial.supplemental_source}抓取 ${formatHkDataTime(financial.supplemental_fetched_at)}`
      : "",
    financial.verification_status === "conflict" ? "核验有冲突" : "来源候选值未核验",
    result.fx_rate
      ? `汇率参考 ${result.fx_rate.rate}（${result.fx_rate.observed_at}）`
      : "金额按来源原币展示",
  ];
  const fieldErrors = result.financial_field_errors || financial.field_errors || {};
  const sourceErrors = Object.entries(fieldErrors).flatMap(([field, error]) => {
    if (field === "sina_supplement") return [`新浪补充未完成：${error}`];
    if (field === "eastmoney_supplement") return [`东方财富主源异常：${error}`];
    return [];
  });
  return [...metadata, ...sourceErrors].filter(Boolean).join(" · ");
}

function formatQuarterlyAmount(value: number | null | undefined, currency: string, unit: string): string {
  if (value == null || !Number.isFinite(value)) return "暂无";
  return `${currency} ${new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 }).format(value)} ${unit}`;
}

function formatQuarterlyPercent(value: number | null | undefined, label: string): string {
  if (value == null || !Number.isFinite(value)) return "";
  return `${label} ${value > 0 ? "+" : ""}${value.toFixed(1)}%`;
}

function formatQuarterlyCheckTime(value: string): string {
  return formatHkDataTime(value);
}

function HkQuarterlyUpdatePanel({
  update,
  checkedAt,
  error,
  inline = false,
}: {
  update?: HkQuarterlyUpdate | null;
  checkedAt?: string | null;
  error?: string | null;
  inline?: boolean;
}) {
  if (!update) {
    if (error) return <p className="data-provenance hk-financial-error" role="status">HKEX 季度更新检查失败：{error}</p>;
    if (checkedAt) return <p className="data-provenance" role="status">HKEXnews 已于 {formatQuarterlyCheckTime(checkedAt)} 检查，未发现可识别的第三季度经营更新。</p>;
    return null;
  }

  const auditLabel = update.audit_status === "unaudited"
    ? "未经审计"
    : update.audit_status === "audited" ? "已审计" : "审计状态未注明";
  const metrics = [
    {
      label: "第三季度营收",
      value: formatQuarterlyAmount(update.quarterly_revenue, update.currency, update.unit),
      detail: [formatQuarterlyPercent(update.quarterly_revenue_yoy_percent, "同比"), formatQuarterlyPercent(update.quarterly_revenue_qoq_percent, "环比")].filter(Boolean).join(" · "),
    },
    {
      label: "第三季度利润",
      value: formatQuarterlyAmount(update.quarterly_net_profit, update.currency, update.unit),
      detail: [formatQuarterlyPercent(update.quarterly_net_profit_yoy_percent, "同比"), formatQuarterlyPercent(update.quarterly_net_profit_qoq_percent, "环比")].filter(Boolean).join(" · "),
    },
    {
      label: "前三季度营收",
      value: formatQuarterlyAmount(update.nine_month_revenue, update.currency, update.unit),
      detail: formatQuarterlyPercent(update.nine_month_revenue_yoy_percent, "同比"),
    },
    {
      label: "前三季度利润",
      value: formatQuarterlyAmount(update.nine_month_net_profit, update.currency, update.unit),
      detail: formatQuarterlyPercent(update.nine_month_net_profit_yoy_percent, "同比"),
    },
    {
      label: "第三季度综合收益",
      value: formatQuarterlyAmount(update.quarterly_total_comprehensive_income, update.currency, update.unit),
      detail: [formatQuarterlyPercent(update.quarterly_total_comprehensive_income_yoy_percent, "同比"), formatQuarterlyPercent(update.quarterly_total_comprehensive_income_qoq_percent, "环比")].filter(Boolean).join(" · "),
    },
    {
      label: "前三季度综合收益",
      value: formatQuarterlyAmount(update.nine_month_total_comprehensive_income, update.currency, update.unit),
      detail: formatQuarterlyPercent(update.nine_month_total_comprehensive_income_yoy_percent, "同比"),
    },
  ];

  return (
    <section className={`hk-financial-panel hk-quarterly-update${inline ? " hk-quarterly-inline" : ""}`} aria-label="港股季度经营更新">
      <header className="hk-financial-heading">
        <div>
          <h3>{update.period} 经营更新 · {auditLabel}</h3>
          <p>{update.period} · 截止 {update.period_end} · 披露 {update.disclosed_at}</p>
        </div>
      </header>
      <div className="hk-financial-grid">
        {metrics.map((metric) => (
          <div className="hk-financial-metric" key={metric.label}>
            <span>{metric.label}</span>
            <strong>{metric.value}</strong>
            {metric.detail && <small>{metric.detail}</small>}
          </div>
        ))}
      </div>
      {error ? <p className="data-provenance hk-financial-error" role="status">
        HKEX 季度更新检查失败：{error}；当前保留 {update.period} 缓存。
        {checkedAt ? ` 最近检查 ${formatQuarterlyCheckTime(checkedAt)}。` : ""}
      </p> : null}
      <p className="hk-financial-source">
        来源：<a href={update.source_url} target="_blank" rel="noreferrer">{update.source_name} 原始公告</a>
        <span> · 抓取 {formatHkDataTime(update.fetched_at)} · 单位：{update.currency} {update.unit}</span>
      </p>
    </section>
  );
}

export function HkObservePanel({
  code: initialCode,
  watchlist,
  onWatchlistChange,
  mobileRuntime = false,
  onOpenNews,
  onAskAgent,
  capability,
  onMarketDataChanged,
}: HkObservePanelProps) {
  const [inputCode, setInputCode] = useState("");
  const [activeCode, setActiveCode] = useState("");
  const [result, setResult] = useState<HkObserveResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [financialLoading, setFinancialLoading] = useState(false);
  const requestVersionRef = useRef(0);
  const resultRef = useRef<HkObserveResult | null>(null);
  const historyWarmupAttemptedRef = useRef(new Set<string>());
  const financialWarmupAttemptedRef = useRef(new Set<string>());
  resultRef.current = result;

  const load = useCallback(async (requestedCode: string, policy: "cache_only" | "refresh" = "cache_only"): Promise<HkObserveResult | undefined> => {
    const normalizedCode = normalizeHkCode(requestedCode);
    if (!normalizedCode) {
      setError("请输入有效港股代码，或从中文名称搜索结果中选择证券。");
      setLoading(false);
      setRefreshing(false);
      return undefined;
    }
    const version = ++requestVersionRef.current;
    const hasCachedResult = resultRef.current?.security.code === normalizedCode.slice(0, -3);
    const requestId = policy === "refresh" ? `hk-observe-${globalThis.crypto?.randomUUID?.() || Date.now()}` : "";
    const query = new URLSearchParams({ data_policy: policy, ...(requestId ? { request_id: requestId } : {}) });
    setActiveCode(normalizedCode);
    setInputCode(normalizedCode);
    setError(null);
    setLoading(policy === "cache_only" || !hasCachedResult);
    setRefreshing(policy === "refresh" && hasCachedResult);
    try {
      const data = await getJson<HkObserveResult>(`/api/hk/observe/${encodeURIComponent(normalizedCode)}?${query.toString()}`);
      if (version !== requestVersionRef.current) return;
      resultRef.current = data;
      setResult(data);
      setError(null);
      return data;
    } catch (reason) {
      if (version !== requestVersionRef.current) return undefined;
      const message = reason instanceof Error ? reason.message : String(reason);
      const previous = resultRef.current;
      if (previous?.security.code === normalizedCode.slice(0, -3)) {
        const preserved = {
          ...previous,
          stale: true,
          notes: [...previous.notes, message],
          refresh_error: policy === "refresh" ? message : previous.refresh_error,
        };
        resultRef.current = preserved;
        setResult(preserved);
        return preserved;
      } else {
        setResult(null);
        setError(message);
        return undefined;
      }
    } finally {
      if (version === requestVersionRef.current) {
        setLoading(false);
        setRefreshing(false);
        if (policy === "refresh") onMarketDataChanged?.();
      }
    }
  }, [onMarketDataChanged]);

  const loadWithHistoryWarmup = useCallback(async (requestedCode: string) => {
    const normalizedCode = normalizeHkCode(requestedCode);
    if (!normalizedCode) {
      await load(requestedCode, "cache_only");
      return;
    }
    const cached = await load(normalizedCode, "cache_only");
    if (!cached) return;
    const hasDisplayName = [
      cached.security.name_zh,
      cached.security.name,
      cached.quote?.name_zh,
      cached.quote?.name,
    ].some(hasChineseName);
    if (cached.quote && cached.history.length >= MIN_HK_OBSERVE_HISTORY_BARS && hasDisplayName) return;
    const key = normalizedCode.slice(0, -3);
    if (historyWarmupAttemptedRef.current.has(key)) return;
    historyWarmupAttemptedRef.current.add(key);
    await load(normalizedCode, "refresh");
  }, [load]);

  useEffect(() => {
    if (!initialCode) {
      requestVersionRef.current += 1;
      setInputCode("");
      setActiveCode("");
      setResult(null);
      setError(null);
      setLoading(false);
      setRefreshing(false);
      return;
    }
    const normalizedCode = normalizeHkCode(initialCode);
    if (!normalizedCode) {
      requestVersionRef.current += 1;
      setInputCode(initialCode);
      setActiveCode("");
      setResult(null);
      setError("请输入有效港股代码。");
      setLoading(false);
      return;
    }
    void loadWithHistoryWarmup(normalizedCode);
  }, [initialCode, loadWithHistoryWarmup]);

  const editCode = useCallback((value: string) => {
    setInputCode(value);
    if (normalizeHkCode(value) === activeCode) return;
    requestVersionRef.current += 1;
    setActiveCode("");
    setResult(null);
    setError(null);
    setLoading(false);
    setRefreshing(false);
  }, [activeCode]);

  const runObserve = useCallback(() => {
    const normalizedCode = normalizeHkCode(inputCode);
    if (!normalizedCode) {
      setError(inputCode.trim()
        ? "请输入港股代码，或先从中文名称搜索结果中选择证券。"
        : "请输入港股代码或中文名称。");
      return;
    }
    void loadWithHistoryWarmup(normalizedCode);
  }, [inputCode, loadWithHistoryWarmup]);

  const refreshFinancial = useCallback(async () => {
    if (!activeCode || financialLoading) return;
    const requestedCode = activeCode;
    const cacheKey = requestedCode.slice(0, -3);
    setFinancialLoading(true);
    try {
      const next = await postJson<HkFinancialResult>("/api/hk/financial-refresh", { code: requestedCode });
      setResult((previous) => previous?.security.code === cacheKey ? {
        ...previous,
        financial: next.snapshot,
        fx_rate: next.fx_rate,
        official_disclosure: next.official_disclosure,
        quarterly_update: next.quarterly_update ?? previous.quarterly_update,
        quarterly_update_checked_at: next.quarterly_update_checked_at,
        quarterly_update_error: next.quarterly_update_error,
        candidate_updated_at: next.candidate_updated_at,
        fx_updated_at: next.fx_updated_at,
        official_checked_at: next.official_checked_at,
        financial_notes: next.notes,
        financial_field_errors: next.field_errors,
        financial_stale: next.stale,
      } : previous);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      setResult((previous) => previous?.security.code === cacheKey ? {
        ...previous,
        financial_stale: true,
        financial_notes: [...(previous.financial_notes || []), message],
        financial_field_errors: { ...previous.financial_field_errors, financial: message },
      } : previous);
    } finally {
      setFinancialLoading(false);
      onMarketDataChanged?.();
    }
  }, [activeCode, financialLoading, onMarketDataChanged]);

  useEffect(() => {
    if (!activeCode || financialLoading || !result || result.security.code !== activeCode.slice(0, -3)) return;
    const quarterCheckedAt = hkDataTimestamp(result.quarterly_update_checked_at);
    const quarterCheckFresh = quarterCheckedAt != null && Date.now() - quarterCheckedAt < 24 * 60 * 60 * 1000;
    if (result.financial && quarterCheckFresh && !result.quarterly_update_error) return;
    const cacheKey = activeCode.slice(0, -3);
    if (financialWarmupAttemptedRef.current.has(cacheKey)) return;
    financialWarmupAttemptedRef.current.add(cacheKey);
    void refreshFinancial();
  }, [activeCode, financialLoading, refreshFinancial, result]);

  const saved = watchlist.some((item) => normalizeStockCode(item.code) === activeCode);
  const toggle = useCallback(() => {
    const stock = result?.security;
    if (!activeCode) return;
    if (saved) {
      onWatchlistChange(watchlist.filter((item) => normalizeStockCode(item.code) !== activeCode));
      return;
    }
    onWatchlistChange([{
      code: activeCode,
      name: result?.quote?.name_zh || stock?.name_zh || stock?.name || activeCode,
      industry: "港股",
      added_at: new Date().toISOString(),
      source: "hk-market",
    }, ...watchlist]);
  }, [activeCode, onWatchlistChange, result?.quote?.name_zh, result?.security, saved, watchlist]);

  let content;
  if (loading) {
    content = <PanelFeedback kind="loading" description="正在加载港股报价、财务和趋势数据…" />;
  } else if (error) {
    content = <PanelFeedback kind="error" title="港股观察失败" description={error} action={<button type="button" className="action-btn" onClick={() => void load(activeCode || inputCode, "refresh")}>重试</button>} />;
  } else if (!activeCode || !result) {
    content = <PanelFeedback kind="empty" description="输入港股代码或中文名称，选择搜索结果后开始观察。" />;
  } else if (!["exchange_listed_equity", "verified_ordinary_share", "user_confirmed_ordinary_share"].includes(result.security.classification_status)) {
    content = <PanelFeedback kind="error" title="港股证券分类不可用" description="当前证券没有可用的交易所股权分类记录，因此不会进入观察。" />;
  } else {
    const sharedResult = toHkSharedObserveResult(result);
    sharedResult.notes = [
      ...(sharedResult.notes || []),
      result.security.classification_status === "exchange_listed_equity" ? "HKEX 已将证券列入 Equity Securities；免费目录未提供普通股或优先股细分。" : "",
      result.security.classification_status === "user_confirmed_ordinary_share" ? "证券类别由你在本机确认为普通股，来源目录未提供该分类核验。" : "",
      result.stale ? "当前显示缓存或不完整数据，请稍后刷新。" : "",
    ].filter(Boolean);
    content = <>
      {refreshing && <p className="data-provenance" role="status" aria-live="polite">正在更新港股行情…当前先展示已有数据。</p>}
      <ObserveResultView
        result={sharedResult}
        mobileRuntime={mobileRuntime}
        expandProfessionalDetails={!mobileRuntime}
        inWatchlist={saved}
        onToggleWatchlist={toggle}
        onOpenNews={onOpenNews}
        onAskAgent={onAskAgent}
        capability={capability || result.quote_capability}
        financialMeta={hkFinancialMeta(result, financialLoading)}
        stockInformation={<HkStockInformation result={result} />}
        financialDisclosure={<HkQuarterlyUpdatePanel update={result.quarterly_update} checkedAt={result.quarterly_update_checked_at} error={result.quarterly_update_error} inline />}
        extraActions={<button type="button" className="stock-row-action" onClick={() => void load(activeCode, "refresh")}>刷新行情</button>}
        financialAction={<button type="button" className="action-btn" onClick={() => void refreshFinancial()} disabled={financialLoading}>{financialLoading ? "刷新中…" : "刷新财务"}</button>}
      />
    </>;
  }

  return (
    <div className="panel-container observe-panel-container">
      <div className="panel-controls observe-panel-controls">
        <div className="form-row inline stock-code-row observe-code-row">
          <label htmlFor="hkObserveCode">股票代码</label>
          <StockCodeInput
            id="hkObserveCode"
            value={inputCode}
            onChange={editCode}
            onCommit={(requestedCode) => void loadWithHistoryWarmup(requestedCode)}
            placeholder="输入港股代码或中文名称"
            market="hk"
            resolveBareCode
          />
        </div>
        <LocalFirstRunControl
          action="观察"
          loading={loading || refreshing}
          onRun={runObserve}
          loadingLabel={refreshing ? "更新中…" : "观察中…"}
          cacheRunLabel="开始观察"
          buttonClassName="run-btn observe-run-btn"
        />
      </div>
      <div className="panel-result observe-panel-result">
        {content}
      </div>
    </div>
  );
}
