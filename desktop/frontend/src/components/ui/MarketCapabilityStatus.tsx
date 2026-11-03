import type { MarketCapability } from "../../lib/marketDomain";
import { marketCapabilityStatusLabel } from "../../lib/marketDomain";

interface MarketCapabilityStatusProps {
  label: string;
  capability?: MarketCapability | null;
  loading?: boolean;
  error?: string | null;
  reasonOverride?: string | null;
  unavailableLabel?: string;
  separator?: string;
  showCoverage?: boolean;
  plain?: boolean;
  id?: string;
  className?: string;
  ariaLabel?: string;
}

export function MarketCapabilityStatus({
  label,
  capability,
  loading = false,
  error,
  reasonOverride,
  unavailableLabel = "未开放",
  separator = " · ",
  showCoverage = true,
  plain = false,
  id,
  className,
  ariaLabel,
}: MarketCapabilityStatusProps) {
  if (!capability && !loading && !error && !reasonOverride) return null;

  const title = capability
    ? `${label}${separator}${marketCapabilityStatusLabel(capability.status, unavailableLabel)}`
    : `${label}${separator}${loading ? "正在读取" : "状态未知"}`;
  const role = error ? "alert" : "status";
  const sources = capability?.sources || [];
  const latestSourceTimestamp = sources
    .map((source) => source.fetched_at || source.observed_at || source.disclosed_at)
    .filter((value): value is string => Boolean(value))
    .sort((left, right) => evidenceTimestamp(left) - evidenceTimestamp(right))
    .at(-1);
  const coverage = capability?.coverage;

  return (
    <div
      id={id}
      className={[
        "market-capability-status",
        capability ? `status-${capability.status}` : "status-unknown",
        plain ? "market-capability-status-plain" : "",
        className || "",
      ].filter(Boolean).join(" ")}
      role={role}
      aria-label={ariaLabel || `${label}状态`}
      aria-live={error ? "assertive" : "polite"}
    >
      <strong>{title}</strong>
      {(reasonOverride || capability?.reason) ? <p>{localizeCapabilityReason(reasonOverride || capability?.reason || "")}</p> : null}
      {showCoverage && coverage && coverage.requested_count > 0 ? (
        <>
          <p className="market-capability-coverage">
            覆盖 {coverage.available_count}/{coverage.requested_count}
            {coverage.missing_count > 0 ? ` · 待补齐 ${coverage.missing_count} 条` : " · 已覆盖"}
          </p>
          {coverage.missing_fields.length ? (
            <details className="market-capability-details">
              <summary>查看缺失项 · {coverage.missing_fields.length} 项</summary>
              <p>{coverage.missing_fields.map(localizeEvidenceFlag).join("、")}</p>
            </details>
          ) : null}
        </>
      ) : null}
      {sources.length ? (
        <>
          <p className="market-capability-source-summary">
            来源 {sources.length} 个{latestSourceTimestamp ? ` · 最近 ${formatEvidenceDate(latestSourceTimestamp)}` : ""}
          </p>
          <details className="market-capability-details">
            <summary>数据依据 · {sources.length} 个来源</summary>
            <ul className="market-capability-sources" aria-label={`${label}数据来源证据`}>
              {sources.slice(0, 6).map((source, index) => (
                <li key={`${source.market}:${source.domain}:${source.source_id}:${source.report_period || index}`}>
                  <span>来源：{sourceLabel(source.source_id)}</span>
                  <span>数据域：{domainLabel(source.domain)}</span>
                  <span>新鲜度：{freshnessLabel(source.freshness)} · 核验：{verificationLabel(source.verification_status)}</span>
                  {source.fetched_at ? <span>抓取：{formatEvidenceTimestamp(source.fetched_at)}</span> : null}
                  {source.observed_at ? <span>观测：{formatEvidenceDate(source.observed_at)}</span> : null}
                  {source.disclosed_at ? <span>披露：{formatEvidenceTimestamp(source.disclosed_at)}</span> : null}
                  {source.report_period ? <span>报告期：{source.report_period}</span> : null}
                  {source.quality_flags.length ? <span>说明：{source.quality_flags.map(localizeEvidenceFlag).join("、")}</span> : null}
                </li>
              ))}
              {sources.length > 6 ? <li>其余 {sources.length - 6} 个来源按市场与数据域记录。</li> : null}
            </ul>
          </details>
        </>
      ) : null}
      {loading ? <p>正在读取当前市场的能力状态。</p> : null}
      {error ? (
        <>
          <p>能力状态暂时无法读取，相关数据状态未知。</p>
          <details className="market-capability-details">
            <summary>错误详情</summary>
            <p>{error.slice(0, 500)}{error.length > 500 ? "…" : ""}</p>
          </details>
        </>
      ) : null}
    </div>
  );
}

function freshnessLabel(value: "fresh" | "stale" | "unknown"): string {
  switch (value) {
    case "fresh": return "新鲜";
    case "stale": return "过期";
    case "unknown": return "未知";
  }
}

function verificationLabel(value: string): string {
  switch (value) {
    case "verified": return "已核验";
    case "metadata_only": return "仅元数据";
    case "conflict": return "有冲突";
    case "unavailable": return "不可用";
    case "unverified": return "未核验";
    default: return "待确认";
  }
}

function sourceLabel(value: string): string {
  if (value === "tencent") return "腾讯行情";
  if (value === "legacy_market_cache") return "本地缓存";
  if (value.startsWith("data/cache/")) return "本地缓存文件";
  if (value === "frankfurter_ecb") return "欧洲央行参考汇率";
  return value.split(/[\\/]/).at(-1)?.replace(/[-_]+/g, " ") || "数据来源";
}

function domainLabel(value: string): string {
  const labels: Record<string, string> = {
    security_master: "证券目录",
    quote: "行情",
    daily_history: "日线历史",
    financial_snapshot: "财务快照",
    factor_snapshot: "因子快照",
    capital_evidence: "资金证据",
    news: "新闻",
    disclosure: "公告",
    relations: "关联关系",
  };
  return labels[value] || value.replace(/[-_]+/g, " ");
}

function localizeEvidenceFlag(value: string): string {
  const labels: Record<string, string> = {
    complete_exchange_universe: "全市场证券覆盖待确认",
    provider_rights: "数据授权待确认",
    provider_rights_unverified: "数据授权待确认",
    complete_universe_unverified: "全市场覆盖待确认",
    complete_screening_factor_coverage: "筛选字段覆盖不足",
    cached_quote_stale: "行情已过期",
    external_cache_status_only: "仅有缓存状态",
    point_in_time_publication_date_missing: "缺少财报公开时间",
    delisted_universe_missing: "缺少退市证券记录",
    corporate_actions_missing: "缺少公司行动记录",
    strict_out_of_sample_folds_missing: "严格样本外折次不足",
    market_rules_missing: "缺少交易规则证据",
    model_processing_rights: "模型处理权限待确认",
  };
  return labels[value] || value.replace(/[-_]+/g, " ");
}

function formatEvidenceTimestamp(value: string): string {
  const raw = value.trim();
  if (/^\d{8}$/.test(raw)) return formatEvidenceDate(raw);
  if (/^\d{4}-\d{2}-\d{2}$/.test(raw)) return raw;
  const numeric = /^\d{10,13}$/.test(raw) ? Number(raw) : null;
  const date = numeric === null
    ? new Date(raw)
    : new Date(numeric < 1_000_000_000_000 ? numeric * 1000 : numeric);
  if (Number.isNaN(date.getTime())) return value;
  return `${date.toISOString().slice(0, 16).replace("T", " ")} UTC`;
}

function formatEvidenceDate(value: string): string {
  const raw = value.trim();
  if (/^\d{8}$/.test(raw)) return `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
  if (/^\d{4}-\d{2}-\d{2}$/.test(raw)) return raw;
  const numeric = /^\d{10,13}$/.test(raw) ? Number(raw) : null;
  const date = numeric === null
    ? new Date(raw)
    : new Date(numeric < 1_000_000_000_000 ? numeric * 1000 : numeric);
  if (Number.isNaN(date.getTime())) return raw.slice(0, 10);
  return date.toISOString().slice(0, 10);
}

function evidenceTimestamp(value: string): number {
  const numeric = /^\d{10,13}$/.test(value) ? Number(value) : null;
  if (numeric !== null) return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  const compactDate = /^(\d{4})(\d{2})(\d{2})$/.exec(value);
  if (compactDate) return Date.parse(`${compactDate[1]}-${compactDate[2]}-${compactDate[3]}T00:00:00Z`);
  return Date.parse(value) || 0;
}

function localizeCapabilityReason(reason: string): string {
  if (reason.startsWith("A-share observation uses cached quote/history")) {
    return "A 股观察显示本地行情和历史数据；数据授权与逐证券历史新鲜度尚未核验。";
  }
  if (reason.startsWith("A-share cache is readable")) {
    return "A 股缓存可读，但全市场证券覆盖和公开应用数据授权尚未核验。";
  }
  if (reason.startsWith("Cached financial snapshots exist")) {
    return "仅部分 A 股证券有财务快照；数据授权和财报公开时间尚未核验。";
  }
  if (reason.startsWith("Cached quotes and financial fields")) {
    return "本地行情和财务字段覆盖尚不完整，公开应用的数据授权也未核验。";
  }
  if (reason.startsWith("Strict release backtests still require")) {
    return "严格回测仍需点时财报、退市证券、公司行动和获准的历史行情证据。";
  }
  if (reason.startsWith("No authorized market-scoped news source")) {
    return "尚未登记获准使用的 A 股新闻来源及缓存、RAG 使用权限。";
  }
  if (reason.startsWith("A-share sentiment evidence coverage")) {
    return "A 股情绪证据覆盖和模型处理权限尚未登记。";
  }
  if (reason.startsWith("The research Agent does not yet report")) {
    return "研究 Agent 尚未报告经核验的市场范围与工具来源契约。";
  }
  if (reason.startsWith("HKEX directory cache is empty")) {
    return "目录缓存为空；HKEX 自动访问授权审核完成前，不能执行在线更新。";
  }
  if (reason.startsWith("HKEX directory cache is stale")) {
    return "目录缓存已过期或上次刷新失败；获得 HKEX 自动访问授权后才能更新目录。";
  }
  if (reason.startsWith("HKEX directory cache is readable")) {
    return "当前显示本地目录缓存；自动更新需等待 HKEX 自动访问授权。";
  }
  if (reason.startsWith("HKEX directory refresh is disabled") || reason.startsWith("HKEX automatic directory refresh is disabled")) {
    return "目录自动更新尚未开放；使用审核完成前，在线更新不可用。";
  }
  if (reason.includes("security-directory refresh is disabled") || reason.includes("security directory refresh is disabled")) {
    return "HKEX 自动访问授权和使用审核完成前，目录在线更新暂不可用。";
  }
  if (reason.includes("written automated-access permission")) {
    return "获得 HKEX 自动访问授权并完成审核前，目录更新暂不开放。";
  }
  if (reason === "strategy adapter or required data is not registered") {
    return "筛选策略适配器或所需数据尚未注册。";
  }
  if (reason === "security directory adapter is not registered") {
    return "证券目录适配器尚未注册。";
  }
  if (reason === "observation adapter or required market data is not registered") {
    return "观察适配器或所需市场数据尚未注册。";
  }
  return reason;
}
