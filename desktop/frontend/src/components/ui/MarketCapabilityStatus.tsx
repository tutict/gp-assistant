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
      {showCoverage && capability?.coverage && capability.coverage.requested_count > 0 ? (
        <p>
          覆盖 {capability.coverage.available_count}/{capability.coverage.requested_count}，
          缺失 {capability.coverage.missing_count}
          {capability.coverage.missing_fields.length
            ? ` · 缺失证据：${capability.coverage.missing_fields.join("、")}`
            : ""}
        </p>
      ) : null}
      {loading ? <p>正在读取当前市场的能力状态。</p> : null}
      {error ? <p>能力状态读取失败：{error}</p> : null}
    </div>
  );
}

function localizeCapabilityReason(reason: string): string {
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
