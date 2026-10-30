import type { ReactNode } from "react";
import type { MarketCapability, MarketId, SecurityId, ObservationMetadata } from "../../lib/marketDomain";
import { MarketCapabilityStatus } from "../ui/MarketCapabilityStatus";

export interface ObservationPanelFrameProps {
  market: MarketId;
  title: string;
  security: SecurityId;
  provenance: ObservationMetadata[];
  capability: MarketCapability;
  issues: string[];
  actions: ReactNode;
  children: ReactNode;
  className?: string;
  showDiagnostics?: boolean;
}

export function ObservationPanelFrame({
  market,
  title,
  security,
  provenance,
  capability,
  issues,
  actions,
  children,
  className,
  showDiagnostics = true,
}: ObservationPanelFrameProps) {
  const sources = provenance.map((item, index) => (
    <li className="observation-panel-frame__source" key={`${item.source_id}-${item.observed_at || "unknown"}-${index}`}>
      <strong>{item.source_id}</strong>
      <span>抓取：{item.fetched_at || "未提供"}</span>
      <span>观测：{item.observed_at || "未知"}</span>
      {item.disclosed_at ? <span>披露：{item.disclosed_at}</span> : null}
      {item.report_period ? <span>报告期：{item.report_period}</span> : null}
      {item.verification_status !== "verified" ? <span>核验：{verificationLabel(item.verification_status)}</span> : null}
      {item.quality_flags.length ? <span>质量标记：{item.quality_flags.join("、")}</span> : null}
      {item.missing_reason ? <span>缺失：{item.missing_reason}</span> : null}
    </li>
  ));
  const uniqueIssues = [...new Set(issues.map((issue) => issue.trim()).filter(Boolean))];

  return (
    <section className={["panel-container", "observation-panel-frame", className].filter(Boolean).join(" ")} aria-label={`${marketLabel(market)}观察`}>
      <header className="panel-heading observation-panel-frame__heading">
        <div>
          <span className="eyebrow">{marketLabel(market)}观察</span>
          <h2>{title}（{security.code}）</h2>
          {showDiagnostics ? <p className="observation-panel-frame__identity">证券身份：{security.market} · {security.kind} · {security.code}</p> : null}
        </div>
        <div className="observation-panel-frame__actions">{actions}</div>
      </header>

      {showDiagnostics ? <section className="observation-panel-frame__evidence" aria-label="来源与能力状态">
        {sources.length ? <ul className="observation-panel-frame__sources" aria-label="数据来源">{sources}</ul> : <p className="data-provenance">本次响应未提供逐来源时间信息。</p>}
        <MarketCapabilityStatus
          className="observation-panel-frame__capability"
          label="观察能力"
          capability={capability}
          unavailableLabel="不可用"
          separator="："
          showCoverage={false}
          plain
          ariaLabel="观察能力状态"
        />
        {capability.coverage ? <p className="observation-panel-frame__coverage">
            覆盖 {capability.coverage.available_count}/{capability.coverage.requested_count}，缺失 {capability.coverage.missing_count}
            {capability.coverage.start_date || capability.coverage.end_date ? ` · ${capability.coverage.start_date || "起始未知"} 至 ${capability.coverage.end_date || "结束未知"}` : ""}
            {capability.coverage.missing_fields.length ? ` · 缺失字段：${capability.coverage.missing_fields.join("、")}` : ""}
        </p> : null}
        {uniqueIssues.length ? <ul className="observation-panel-frame__issues" aria-label="数据问题">{uniqueIssues.map((issue) => <li key={issue}>{issue}</li>)}</ul> : null}
      </section> : null}

      <div className="observation-panel-frame__body">{children}</div>
    </section>
  );
}

function marketLabel(market: MarketId): string {
  return market === "hk" ? "港股" : "A股";
}

function verificationLabel(status: string): string {
  switch (status) {
    case "conflict": return "有冲突";
    case "metadata_only": return "仅元数据";
    case "unavailable": return "不可用";
    case "unverified": return "未核验";
    default: return status;
  }
}
