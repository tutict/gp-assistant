import type { CapabilityId, MarketCapability, MarketId } from "../../lib/marketDomain";
import { marketCapabilityLabel } from "../../lib/marketDomain";
import { MarketCapabilityStatus } from "./MarketCapabilityStatus";

interface MarketCapabilityPopoverProps {
  market: MarketId;
  label: string;
  capabilities: MarketCapability[] | null;
  capabilityIds: CapabilityId[];
  loading?: boolean;
  error?: string | null;
}

export function MarketCapabilityPopover({
  market,
  label,
  capabilities,
  capabilityIds,
  loading = false,
  error,
}: MarketCapabilityPopoverProps) {
  const items = capabilityIds
    .map((id) => capabilities?.find((capability) => capability.id === id))
    .filter((item): item is MarketCapability => Boolean(item && item.market === market));
  const unavailableCount = items.filter((item) => item.status === "unavailable").length;
  const limitedCount = items.filter((item) => item.status === "limited").length;
  const summary = loading && !capabilities
    ? "读取中"
    : error
      ? "状态异常"
      : unavailableCount > 0
        ? `${unavailableCount} 项未开放`
        : limitedCount > 0
          ? `${limitedCount} 项部分可用`
          : "已就绪";

  return (
    <details className="market-capability-popover">
      <summary aria-label={`${label}：${summary}`}>
        <span>{label}</span>
        <span className="market-capability-popover__summary-state">{summary}</span>
      </summary>
      <div className="market-capability-popover__content">
        {error ? <p role="alert">能力状态读取失败：{error}</p> : null}
        {items.map((capability) => (
          <MarketCapabilityStatus
            key={capability.id}
            label={`${label.replace("数据状态", "")}${marketCapabilityLabel(capability.id)}`}
            capability={capability}
            showCoverage
            plain
          />
        ))}
        {loading && !capabilities ? <p role="status">正在读取当前市场的能力状态。</p> : null}
      </div>
    </details>
  );
}
