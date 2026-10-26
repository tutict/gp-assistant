import type { MarketCapability } from "../../lib/marketDomain";

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
          <strong>{feature} · {capability ? capabilityStatusLabel(capability.status) : loading ? "正在读取能力状态" : "状态未知"}</strong>
          {[capability, ...additionalCapabilities].filter((item): item is MarketCapability => Boolean(item)).map((item) => <div key={item.id}>
            <strong>{capabilityLabel(item.id)}：{capabilityStatusLabel(item.status)}</strong>
            {item.reason ? <p>{item.reason}</p> : null}
            {item.coverage ? <p>
              覆盖 {item.coverage.available_count}/{item.coverage.requested_count}，缺失 {item.coverage.missing_count}
              {item.coverage.missing_fields.length ? ` · 缺失证据：${item.coverage.missing_fields.join("、")}` : ""}
            </p> : null}
          </div>)}
          {loading ? <p>正在读取当前市场的能力注册表。</p> : null}
          {error ? <p role="alert">能力状态读取失败：{error}</p> : null}
          {onSwitchMarket ? <button type="button" className="action-btn" onClick={onSwitchMarket}>切换到 A 股</button> : null}
        </div>
      </div>
    </section>
  );
}

function capabilityStatusLabel(status: MarketCapability["status"]): string {
  switch (status) {
    case "available": return "可用";
    case "limited": return "部分可用";
    case "unavailable": return "未开放";
  }
}

function capabilityLabel(id: MarketCapability["id"]): string {
  const labels: Record<MarketCapability["id"], string> = {
    security_directory: "证券目录",
    observe: "证券观察",
    financial_snapshot: "财务快照",
    screening: "筛选",
    backtest: "回测",
    news_research: "新闻与 RAG",
    sentiment: "情绪",
    research_agent: "研究 Agent",
  };
  return labels[id];
}
