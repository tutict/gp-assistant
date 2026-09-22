export type SentimentStage = "过热" | "降温" | "中性／分歧" | "低迷" | "修复" | "证据不足";
export type SentimentVerdict = "支持" | "不支持" | "待确认";
export interface SentimentEvidence {
  id: string;
  document_id: string;
  event_id: string;
  title: string;
  excerpt: string;
  source_name: string;
  source_tier: string;
  source_verified: boolean;
  published_at: string | null;
  first_seen_at: number;
  url: string | null;
  sentiment: string;
  pool: "fact" | "discussion";
  coverage: "full_text" | "excerpt";
  duplicate_count: number;
  provenance?: {
    original_url?: string | null;
    original_source_tier?: string | null;
    retracted?: boolean | null;
    correction_of?: string | null;
    source_verification?: string | null;
  };
}
export interface SentimentTimelinePoint {
  date: string;
  positive: number | null;
  negative: number | null;
  discussion_count: number | null;
  sentiment_balance: number | null;
  close: number | null;
  volume: number | null;
  industry_return: number | null;
  industry_breadth: number | null;
}
export interface SentimentSnapshot {
  snapshot_id: string;
  stock_code: string;
  stock_name: string;
  industry: string | null;
  window_days: number;
  cutoff: number;
  generation: string;
  rule_version: string;
  evidence: SentimentEvidence[];
  timeline: SentimentTimelinePoint[];
  metrics: Record<string, number | string | null>;
  metric_references?: Partial<Record<"M1" | "M2" | "M3", string[]>>;
  coverage: { facts: number; discussions: number; price_days: number; history_days: number; industry_members: number; industry_covered: number; gaps: string[] };
  quality_gates?: Partial<Record<"messages" | "price" | "industry" | "historical_heat", boolean>>;
}
export interface SentimentDimension {
  key: "messages" | "price" | "industry";
  direction: string;
  summary: string;
  support: string[];
  against: string[];
  evidence_ids: string[];
  gaps: string[];
}
export interface SentimentAnalysis {
  analysis_id: string;
  run_id: string;
  snapshot_id: string;
  stock_code: string;
  created_at: number;
  stage: SentimentStage;
  top_risk: SentimentVerdict;
  bottom_candidate: SentimentVerdict;
  turning_signal: string;
  sufficiency: "充分" | "有限" | "不足";
  summary: string;
  dimensions: SentimentDimension[];
  support: string[];
  against: string[];
  invalidation: string[];
  evidence_ids: string[];
  model: string;
  rule_version: string;
  stale: boolean;
  snapshot: SentimentSnapshot;
}
export interface SentimentRun {
  run_id: string;
  stock_code: string;
  status: "running" | "completed" | "failed" | "cancelled";
  stage: string;
  progress: number;
  error?: string | null;
  result?: SentimentAnalysis | null;
}
export interface SentimentFollowup {
  analysis_id: string;
  answer: string;
  evidence_ids: string[];
  created_at: number;
}
