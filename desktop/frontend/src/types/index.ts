export interface StockItem {
  code: string;
  name: string;
  industry?: string;
  price?: number;
  change_pct?: number;
  pct?: number;
  pe?: number;
  pb?: number;
  roe?: number;
  market_cap_billion?: number;
  market_cap?: number;
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
  require_institution_buy_ratio_gt_sell_ratio?: boolean;
  include_st?: boolean;
  limit?: number;
  sort_by?: string;
  sort_dir?: string;
  score_profile?: "quality" | "rotation" | string;
}

export interface ScreenedStock {
  stock: StockItem;
  score: number;
  reasons: string[];
  factor_scores?: Record<string, number>;
  score_explanation?: string;
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
  items?: FinancialIndicatorItem[];
  quarterly_eps?: FinancialIndicatorItem[];
  notes?: string[];
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
}

export interface EquityPoint {
  date: string;
  equity: number;
}

export interface BacktestResult {
  metrics: BacktestMetrics;
  equity_curve: EquityPoint[];
  benchmark_curve?: EquityPoint[];
  symbols: string[];
  benchmark_symbols?: string[];
  rebalance_dates?: string[];
  notes?: string[];
}

export interface LlmClientConfig {
  api_key?: string;
  base_url?: string;
  model?: string;
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

export interface AgentResult {
  reply?: string;
  action?: string;
  criteria?: ScreenCriteria | null;
  backtest?: Record<string, unknown> | null;
  news_rag?: Record<string, unknown> | null;
  observe?: Record<string, unknown> | null;
  sector_screen?: Record<string, unknown> | null;
  graph_screen?: Record<string, unknown> | null;
  trend_screen?: Record<string, unknown> | null;
  data?: Record<string, unknown> | null;
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
  universe_updated_at?: string | null;
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
  api_key?: string;
  base_url?: string;
  model?: string;
  temperature?: number;
  timeout?: number;
  json_mode?: boolean;
  remember_key?: boolean;
}

export interface StockRowView {
  code: string;
  name: string;
  industry?: string;
  price?: number;
  change_pct?: number | null;
  pe?: number;
  pb?: number;
  roe?: number;
  market_cap_billion?: number;
  score?: number;
  scoreLabel?: string;
  reasons?: string[];
  concept?: string;
  factorScores?: Record<string, number>;
  explanation?: SelectionExplanation | null;
  raw?: unknown;
}

export type PanelKey =
  | "screen"
  | "sectorScreen"
  | "boardScreen"
  | "graph"
  | "trendAnalyze"
  | "trendScreen"
  | "backtest"
  | "newsRag"
  | "ragPackBuild"
  | "ragPackQuery"
  | "upstreamScan"
  | "upstreamImport"
  | "agent"
  | "observe";
