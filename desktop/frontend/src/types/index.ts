export interface StockItem {
  code: string;
  name: string;
  industry?: string;
  price?: number;
  change_pct?: number;
  pct?: number;
  pe?: number;
  pb?: number;
  eps?: number;
  latest_eps?: number;
  roe?: number;
  market_cap_billion?: number;
  market_cap?: number;
  circulating_market_cap_billion?: number;
  total_shares?: number;
  circulating_shares?: number;
  dividend_yield?: number;
  is_st?: boolean;
  quote_time?: string;
  [key: string]: unknown;
}

export interface ScreenCriteria {
  min_roe?: number;
  max_pe?: number;
  max_pb?: number;
  min_market_cap_billion?: number;
  min_deducted_net_profit_billion?: number;
  min_deducted_net_profit_margin?: number;
  min_deducted_net_profit_growth_rate?: number;
  industry?: string;
  market_scope?: string;
  require_institution_buy_ratio_gt_sell_ratio?: boolean;
  include_st?: boolean;
  limit?: number;
  sort_by?: string;
  sort_dir?: string;
  score_profile?: "balanced" | "quality" | "trend" | "rotation" | string;
}

export interface ScreenedStock {
  stock: StockItem;
  score: number;
  reasons: string[];
  quality_score?: number;
  trend_score?: number;
  risk_score?: number;
  balanced_score?: number;
  factor_scores?: Record<string, number>;
  score_breakdown?: ScoreContribution[];
  score_explanation?: string;
  reason_tags?: string[];
  risk_tags?: string[];
  suitable_periods?: string[];
  concept?: string | null;
  theme_category?: string | null;
}

export interface ScreenResultGroup {
  key: string;
  title: string;
  description?: string;
  total?: number;
  returned?: number;
  items: ScreenedStock[];
}

export interface ScreenResult {
  total: number;
  returned: number;
  items: ScreenedStock[];
  groups?: ScreenResultGroup[];
  notes?: string[];
  algorithm_version?: string;
  market_regime?: AdaptiveMarketRegime;
  rollout?: {
    adaptive_available: boolean;
    adaptive_default_enabled: boolean;
    reason: string;
  };
}

export type AdaptiveScreenMode = "auto" | "range" | "trend" | "defensive";

export interface AdaptiveScreenRequest {
  criteria: ScreenCriteria;
  mode: AdaptiveScreenMode;
  horizon: "swing_10_30d";
  primary_limit: number;
  exploration_limit: number;
  run_id: string;
}

export interface AdaptiveRegimeEvidence {
  key: string;
  label: string;
  value: number;
  summary: string;
}

export interface AdaptiveMarketRegime {
  detected: "range" | "trend" | "defensive" | "transition" | string;
  effective: "range" | "trend" | "defensive" | "transition" | string;
  confidence: number;
  overridden: boolean;
  as_of_date?: string | null;
  evidence: AdaptiveRegimeEvidence[];
  coverage: {
    candidate_requested: number;
    candidate_usable: number;
    candidate_ratio: number;
    benchmark_requested: number;
    benchmark_usable: number;
    breadth_usable: boolean;
    breadth_requested: number;
    breadth_observed: number;
    breadth_coverage_ratio: number;
  };
}

export interface SectorScreenRequest {
  criteria: ScreenCriteria;
  per_sector_limit: number;
  max_sectors: number;
  min_sector_candidates: number;
  group_by: "concept" | "board" | "market" | "market_board";
}

export interface SectorScreenGroup {
  sector: string;
  total: number;
  returned: number;
  average_score: number;
  items: ScreenedStock[];
}

export interface SectorScreenResult {
  total: number;
  returned: number;
  sector_count: number;
  groups: SectorScreenGroup[];
  notes?: string[];
}

export interface StockRelation {
  source_code: string;
  target_code: string;
  relation_type: string;
  weight: number;
  description?: string | null;
}

export interface ScoreContribution {
  key: string;
  label: string;
  value?: number | null;
  contribution?: number | null;
  tone?: string;
}

export interface SelectionExplanation {
  basis?: string[];
  score_breakdown?: ScoreContribution[];
  risk_checks?: string[];
  verification?: string[];
}

export interface GraphStockSignal {
  stock: StockItem;
  base_score: number;
  relation_score: number;
  final_score: number;
  suggested_weight: number;
  reasons: string[];
  related?: StockRelation[];
  explanation?: SelectionExplanation;
}

export interface GraphScreenResult {
  total: number;
  returned: number;
  relation_count: number;
  items: GraphStockSignal[];
  center_context?: { mode?: string; label?: string; codes?: string[] };
  notes?: string[];
}

export interface TrendIndicatorPoint {
  date: string;
  close: number;
  open?: number | null;
  high?: number | null;
  low?: number | null;
  volume?: number | null;
  swl?: number | null;
  sws?: number | null;
  k?: number | null;
  d?: number | null;
  j?: number | null;
  red_hold?: boolean;
  cyan_watch?: boolean;
  short_buy?: boolean;
  white_exit?: boolean;
  [key: string]: unknown;
}

export interface TrendIndicatorSignal {
  code: string;
  date: string;
  close: number;
  previous_close?: number | null;
  close_change?: number | null;
  close_change_pct?: number | null;
  swl?: number | null;
  sws?: number | null;
  k?: number | null;
  d?: number | null;
  j?: number | null;
  support?: number | null;
  resistance?: number | null;
  quant_score?: number;
  quant_score_max?: number;
  pattern_score?: number;
  pattern_score_max?: number;
  pattern_signals?: string[];
  status?: string;
  reasons?: string[];
  notes?: string[];
  [key: string]: unknown;
}

export interface TrendIndicatorResult {
  stock: StockItem;
  signal: TrendIndicatorSignal;
  series: TrendIndicatorPoint[];
  chip_distribution?: Record<string, unknown> | null;
}

export interface TrendStockSignal {
  stock: StockItem;
  base_score: number;
  trend_score: number;
  final_score: number;
  signal: TrendIndicatorSignal;
  reasons?: string[];
  explanation?: SelectionExplanation;
}

export interface TrendScreenResult {
  total: number;
  returned: number;
  items: TrendStockSignal[];
  screen_style?: string;
  notes?: string[];
}

export interface FinancialIndicatorItem {
  metric_key?: string;
  label?: string;
  value?: string | number | null;
  raw_value?: string | number | null;
  period?: string | null;
  tone?: string;
}

export interface FinancialIndicatorSection {
  title?: string;
  period?: string | null;
  source?: string | null;
  items?: FinancialIndicatorItem[];
  quarterly_eps?: FinancialIndicatorItem[];
  notes?: string[];
}

export interface CapitalEvidenceSeat {
  seat_code?: string | null;
  name?: string;
  trade_date?: string | null;
  buy_amount?: number | null;
  sell_amount?: number | null;
  net_amount?: number | null;
  buy_ratio?: number | null;
  sell_ratio?: number | null;
  direction?: "buy" | "sell" | "both" | "unknown";
  change_rate?: number | null;
  reason?: string | null;
  three_day_rise_probability?: number | null;
  three_day_activity_count?: number | null;
}

export interface CapitalEvidenceItem {
  category?: string;
  source?: string;
  title?: string;
  date?: string | null;
  metrics?: Record<string, unknown>;
  sentiment?: string;
  weight?: number;
  confidence?: string;
  url?: string | null;
  score?: number | null;
  note?: string | null;
  seats?: CapitalEvidenceSeat[];
  seat_detail_status?: string;
  seat_detail_note?: string | null;
}

export interface CapitalEvidenceSection {
  key: string;
  title: string;
  score?: number | null;
  weight?: number;
  available?: boolean;
  summary?: string | null;
  items?: CapitalEvidenceItem[];
}

export interface CapitalEvidenceResult {
  stock_code: string;
  generated_at?: string;
  composite_score?: number | null;
  confidence?: string;
  model_used?: boolean;
  as_of_trade_date?: string | null;
  freshness?: string;
  contributions?: Record<string, unknown>;
  summary?: string | null;
  sections?: CapitalEvidenceSection[];
  items?: CapitalEvidenceItem[];
  notes?: string[];
}

export interface ObserveResult {
  source: string;
  stock: StockItem;
  financial_indicators?: FinancialIndicatorSection | null;
  trend?: TrendIndicatorResult | null;
  capital_evidence?: CapitalEvidenceResult | null;
  order_book?: Record<string, unknown> | null;
  notes?: string[];
}

export interface BacktestMetrics {
  total_return: number;
  annualized_return?: number | null;
  max_drawdown?: number | null;
  num_stocks: number;
  benchmark_total_return?: number | null;
  benchmark_annualized_return?: number | null;
  benchmark_max_drawdown?: number | null;
  excess_return?: number | null;
  total_transaction_cost?: number;
  total_turnover?: number;
  rebalance_count?: number;
  oos_fold_count?: number;
  evaluated_selection_count?: number;
  selection_hit_count?: number;
  precision_at_n?: number | null;
  strategy_mode?: string;
}

export interface WalkForwardFold {
  signal_date?: string | null;
  selection_date: string;
  evaluation_end_date?: string | null;
  selected_symbols: string[];
  eligible_symbol_count: number;
  evaluated_selection_count: number;
  hit_count: number;
  precision_at_n?: number | null;
  average_forward_return?: number | null;
  benchmark_forward_return?: number | null;
  average_excess_return?: number | null;
}

export interface EquityPoint {
  date: string;
  equity: number;
}


export interface AtrSnapshot {
  period: number;
  value: number;
  percent_of_close: number;
}

export interface BollingerBandsSnapshot {
  period: number;
  multiplier: number;
  upper: number;
  middle: number;
  lower: number;
  bandwidth_percent?: number | null;
  percent_b?: number | null;
}

export interface DonchianChannelSnapshot {
  period: number;
  upper: number;
  middle: number;
  lower: number;
  width_percent?: number | null;
  position_percent?: number | null;
}

export interface KeltnerChannelSnapshot {
  ema_period: number;
  atr_period: number;
  multiplier: number;
  upper: number;
  middle: number;
  lower: number;
  width_percent?: number | null;
  position_percent?: number | null;
}

export interface ChaikinVolatilitySnapshot {
  ema_period: number;
  roc_period: number;
  value: number;
}

export interface RviSnapshot {
  period: number;
  value: number;
}

export interface IndicatorUnavailable {
  indicator: string;
  reason: string;
}

export interface VolatilitySnapshot {
  symbol: string;
  date: string;
  close?: number | null;
  atr?: AtrSnapshot | null;
  bollinger_bands?: BollingerBandsSnapshot | null;
  donchian_channel?: DonchianChannelSnapshot | null;
  keltner_channel?: KeltnerChannelSnapshot | null;
  chaikin_volatility?: ChaikinVolatilitySnapshot | null;
  rvi?: RviSnapshot | null;
  unavailable?: IndicatorUnavailable[];
}

export interface BacktestResult {
  metrics: BacktestMetrics;
  equity_curve: EquityPoint[];
  benchmark_curve?: EquityPoint[];
  symbols: string[];
  benchmark_symbols?: string[];
  rebalance_dates?: string[];
  walk_forward_folds?: WalkForwardFold[];
  volatility_snapshots?: VolatilitySnapshot[];
  volatility_message?: string | null;
  strategy_mode?: string;
  notes?: string[];
  legacy_balanced_backtest?: BacktestResult;
  adaptive_release_gate?: {
    passed: boolean;
    checks: {
      key: string;
      passed: boolean;
      actual?: number | null;
      requirement: string;
    }[];
  };
}

export interface LlmClientConfig {
  api_key?: string;
  base_url?: string;
  model?: string;
  api_format?: LlmApiFormat;
  endpoint_mode?: LlmEndpointMode;
  custom_user_agent?: string;
  temperature?: number;
  timeout_seconds?: number;
  json_mode?: boolean;
  organization?: string;
  project?: string;
}

export interface NewsEvidence {
  title: string;
  summary?: string | null;
  source: string;
  source_tier?: string;
  published_at?: string | null;
  url?: string | null;
  stock_codes?: string[];
  relation_types?: string[];
  sentiment?: string;
}

export interface NewsImpactFinding {
  target: string;
  direction: string;
  confidence: string;
  impact_chain: string;
  evidence?: NewsEvidence[];
  pending_checks?: string[];
}

export interface NewsSentimentGroups {
  mode?: string;
  positive?: NewsEvidence[];
  negative?: NewsEvidence[];
  mixed?: NewsEvidence[];
  uncertain?: NewsEvidence[];
}

export interface NewsRagResult {
  scope_codes?: string[];
  relation_count?: number;
  message_count?: number;
  findings?: NewsImpactFinding[];
  sentiment_groups?: NewsSentimentGroups;
  us_market_brief?: Record<string, unknown>;
  notes?: string[];
}

export type ResearchSourceTier =
  | "filing"
  | "financial_snapshot"
  | "news"
  | "research_report"
  | "community"
  | string;

export interface ResearchMessage {
  id: string;
  document_id: string;
  stock_code?: string | null;
  title: string;
  summary: string;
  sentiment: string;
  source_tier: ResearchSourceTier;
  published_at?: string | null;
  unread: boolean;
}

export interface ResearchCitation {
  citation_id: string;
  document_id: string;
  chunk_id: string;
  title: string;
  excerpt: string;
  source_tier: ResearchSourceTier;
  source_name: string;
  published_at?: string | null;
  url?: string | null;
  page_number?: number | null;
  lexical_score: number;
  vector_score?: number | null;
  retrieval_score: number;
}

export interface ResearchAnswer {
  id?: string;
  thread_id?: string | null;
  mode: "evidence_only" | "model" | string;
  question: string;
  answer: string;
  citations: ResearchCitation[];
  created_at_epoch_ms?: number;
  model_warning?: string;
  vector_warning?: string;
}

export interface ResearchThread {
  id: string;
  title: string;
  stock_code?: string | null;
  created_at_epoch_ms: number;
  updated_at_epoch_ms: number;
}

export interface KnowledgeDocumentStatus {
  document_id: string;
  title: string;
  source_tier: ResearchSourceTier;
  chunk_count: number;
  embedding_count: number;
  indexed: boolean;
  updated_at_epoch_ms: number;
}

export interface ResearchOverview {
  schema_version: number;
  database_path?: string;
  document_count: number;
  chunk_count: number;
  unread_count: number;
  unread_by_stock?: Record<string, number>;
  source_counts?: { source_tier: ResearchSourceTier; count: number }[];
  messages: ResearchMessage[];
  retrieval?: {
    lexical?: string;
    vector?: Record<string, unknown>;
    rrf_k?: number;
    citation_limit?: number;
  };
}

export interface ResearchQueryResult extends ResearchAnswer {
  query: string;
  community_only?: boolean;
  fact_supported?: boolean;
  retrieval_mode?: "bm25" | "hybrid_rrf" | string;
}

export interface ResearchIndexStatus {
  schema_version: number;
  document_count: number;
  chunk_count: number;
  fts_count: number;
  embedding_count: number;
  database_bytes: number;
  healthy: boolean;
  vector?: Record<string, unknown>;
  documents?: KnowledgeDocumentStatus[];
}

export interface RagPackBuildResult {
  path?: string;
  document_count?: number;
  chunk_count?: number;
  content_hash?: string;
  embedding_model?: string;
  embedding_backend?: string;
  embedding_quantization?: string;
  embedding_dim?: number;
  notes?: string[];
}

export interface RagPackHit {
  chunk_id?: string;
  document_id?: string;
  score?: number;
  title?: string;
  text?: string;
  source?: string;
  source_tier?: string;
  published_at?: string | null;
  url?: string | null;
  stock_codes?: string[];
  relation_types?: string[];
  sentiment?: string;
}

export interface RagPackQueryResult {
  hits?: RagPackHit[];
  manifest?: Record<string, unknown>;
  notes?: string[];
}

export interface UpstreamRagBuildResult {
  pack_path?: string;
  manifest_path?: string;
  manifest?: Record<string, unknown>;
  quality?: { errors?: string[]; warnings?: string[]; [key: string]: unknown };
  notes?: string[];
}

export interface UpstreamRagTransferResult {
  manifest_url?: string;
  pack_url?: string;
  token?: string;
  expires_at?: string;
  qr_svg?: string | null;
  descriptor_json?: string;
  notes?: string[];
}

export interface AgentIntent {
  kind?: string;
  query?: string;
  symbols?: string[];
  window?: string | null;
  depth?: string;
  mode?: string;
}

export interface AgentToolCall {
  id?: string;
  tool?: string;
  label?: string;
  status?: string;
  input?: Record<string, unknown> | null;
  output_summary?: string | null;
  warnings?: string[];
}

export interface AgentEvidenceItem {
  title?: string;
  source?: string;
  level?: string;
  summary?: string;
}

export interface AgentAnswerSection {
  title?: string;
  bullets?: string[];
  provenance?: "model_inference" | string;
  evidence_basis?: string;
}

export interface AgentHarnessMeta {
  prompt_version?: string;
  profile_id?: "deterministic_v1" | "hot_money_early_v1" | "value_compounder_v1" | string;
  model_used?: boolean;
  model?: string | null;
}

export interface AgentResult {
  reply?: string;
  action?: string;
  intent?: AgentIntent | null;
  tool_calls?: AgentToolCall[];
  evidence_summary?: AgentEvidenceItem[];
  answer_sections?: AgentAnswerSection[];
  model_answer_sections?: AgentAnswerSection[];
  warnings?: string[];
  next_actions?: string[];
  criteria?: ScreenCriteria | null;
  backtest?: Record<string, unknown> | null;
  news_rag?: Record<string, unknown> | null;
  observe?: Record<string, unknown> | null;
  sector_screen?: Record<string, unknown> | null;
  graph_screen?: Record<string, unknown> | null;
  trend_screen?: Record<string, unknown> | null;
  data?: Record<string, unknown> | null;
  harness?: AgentHarnessMeta | null;
}

export interface AgentStreamEvent {
  run_id?: string;
  type?: "status" | "result" | "error" | string;
  stage?: string;
  label?: string;
  percent?: number;
  action?: string;
  response?: AgentResult;
  message?: string;
  payload?: unknown;
}

export interface DataStatus {
  source?: string;
  cache_dir?: string;
  cache_bytes?: number;
  cache_limit_bytes?: number;
  cache_usage?: number;
  universe_count?: number;
  universe_cache_path?: string | null;
  universe_updated_at?: string | number | null;
  quote_generated_at?: string | number | null;
  quote_trade_date?: string | null;
  current_trade_date?: string | null;
  generated_at?: string | number | null;
  universe_age_hours?: number | null;
  stale?: boolean;
  policy?: Record<string, unknown>;
  notes?: string[];
}

export interface WatchlistItem {
  code: string;
  name?: string;
  industry?: string;
  added_at?: string;
  source?: string;
  screenCriteriaSummary?: string;
}

export interface LlmSettings {
  active_provider_id?: string;
  providers?: LlmProviderSettings[];
}

export type LlmApiFormat = "openai_chat" | "openai_responses" | "anthropic_messages";
export type LlmEndpointMode = "base_url" | "full_url";

export interface LlmProviderSettings {
  id?: string;
  name?: string;
  provider?: string;
  api_key?: string;
  base_url?: string;
  model?: string;
  api_format?: LlmApiFormat;
  endpoint_mode?: LlmEndpointMode;
  custom_user_agent?: string;
  temperature?: number;
  timeout?: number;
  json_mode?: boolean;
  remember_key?: boolean;
}

export interface LlmModelOption {
  id: string;
  name?: string | null;
  owned_by?: string | null;
}

export interface LlmModelListResult {
  provider?: string;
  endpoint?: string;
  count?: number;
  models?: LlmModelOption[];
}

export interface LlmConnectionTestResult {
  ok: boolean;
  endpoint?: string;
  status?: number;
  elapsed_ms?: number;
  api_format?: LlmApiFormat;
}

export interface StockRowView {
  code: string;
  name: string;
  industry?: string;
  price?: number;
  change_pct?: number | null;
  pe?: number;
  pb?: number;
  eps?: number;
  roe?: number;
  market_cap_billion?: number;
  score?: number;
  scoreLabel?: string;
  reasons?: string[];
  concept?: string;
  qualityScore?: number;
  trendScore?: number;
  riskScore?: number;
  balancedScore?: number;
  factorScores?: Record<string, number>;
  scoreBreakdown?: ScoreContribution[];
  reasonTags?: string[];
  riskTags?: string[];
  suitablePeriods?: string[];
  explanation?: SelectionExplanation | null;
  raw?: unknown;
}

export type PanelKey =
  | "screen"
  | "sectorScreen"
  | "boardScreen"
  | "graph"
  | "trendScreen"
  | "backtest"
  | "newsRag"
  | "ragPackBuild"
  | "ragPackQuery"
  | "upstreamScan"
  | "upstreamImport"
  | "agent"
  | "observe";
