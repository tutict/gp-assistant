import type { MarketCapability } from "../../lib/marketDomain";
import { marketCapabilityLabel } from "../../lib/marketDomain";
import { MarketCapabilityStatus } from "../ui/MarketCapabilityStatus";

export function HkUnavailablePanel({
  feature,
  capability,
  additionalCapabilities = [],
  loading = false,
  error,
  onSwitchMarket,
}: {
  feature: string;
  capability?: MarketCapability | null;
  additionalCapabilities?: MarketCapability[];
  loading?: boolean;
  error?: string | null;
  onSwitchMarket?: () => void;
}) {
  return (
    <section className="panel-container hk-unavailable-panel" aria-label={`${feature}能力状态`}>
      <div className="panel-feedback panel-feedback-empty">
        <div>
          {capability || loading || error ? <MarketCapabilityStatus label={feature} capability={capability} loading={loading} error={error} plain /> : <strong>{feature} · 状态未知</strong>}
          {additionalCapabilities.map((item) => <MarketCapabilityStatus key={item.id} label={marketCapabilityLabel(item.id)} capability={item} plain />)}
          {onSwitchMarket ? <button type="button" className="action-btn" onClick={onSwitchMarket}>切换到 A 股</button> : null}
        </div>
      </div>
    </section>
  );
}
