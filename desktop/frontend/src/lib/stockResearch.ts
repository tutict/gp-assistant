import type { ScoringMetadata } from "../types";

export interface ResearchDataset {
  dataset_id: string;
  name: string;
  start_date: string;
  end_date: string;
  real_data: boolean;
  source: string;
  security_count?: number;
  bar_count?: number;
}

export interface ResearchRun {
  run_id: string;
  dataset_id: string;
  kind: string;
  status: string;
  parameter_set_id?: string;
}

export interface ResearchStatus {
  available: boolean;
  app_data_dir?: string;
  datasets: ResearchDataset[];
  runs: ResearchRun[];
  active_parameter_set: ScoringMetadata;
  active_dataset_id?: string;
}

export interface ResearchMetrics {
  total_return?: number;
  annualized_return?: number;
  max_drawdown?: number;
  annualized_turnover?: number;
  total_transaction_cost?: number;
}

export interface ResearchReportView {
  runId: string;
  datasetId: string;
  realData: boolean;
  qualityPassed: boolean;
  legacy: boolean;
  qualified: boolean;
  parameters: ScoringMetadata;
  metrics?: ResearchMetrics;
  baseline?: ResearchMetrics;
  issues: string[];
  checks: { key: string; passed: boolean; requirement: string; actual?: number }[];
}

function researchObject(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? Object.fromEntries(Object.entries(value)) : {};
}

function researchText(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function researchNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function scoringMetadata(value: unknown): ScoringMetadata {
  const metadata = researchObject(value);
  return {
    algorithm_version: researchText(metadata.algorithm_version),
    parameter_set_id: researchText(metadata.parameter_set_id),
    scoring_revision: researchText(metadata.scoring_revision),
    implementation_fingerprint: researchText(metadata.implementation_fingerprint),
    metric_contract_version: researchNumber(metadata.metric_contract_version),
  };
}

export function parseResearchStatus(value: unknown): ResearchStatus {
  const status = researchObject(value);
  if (typeof status.available !== "boolean" || !Array.isArray(status.datasets) || !Array.isArray(status.runs)) {
    throw new Error("研究状态格式无效");
  }
  const datasets = status.datasets.map((raw): ResearchDataset => {
    const dataset = researchObject(raw);
    if (!researchText(dataset.dataset_id)) throw new Error("研究数据集缺少版本标识");
    return {
      dataset_id: researchText(dataset.dataset_id), name: researchText(dataset.name),
      start_date: researchText(dataset.start_date), end_date: researchText(dataset.end_date),
      source: researchText(dataset.source), real_data: dataset.real_data === true,
      security_count: researchNumber(dataset.security_count), bar_count: researchNumber(dataset.bar_count),
    };
  });
  const runs = status.runs.map((raw): ResearchRun => {
    const run = researchObject(raw);
    if (!researchText(run.run_id)) throw new Error("研究报告缺少运行标识");
    return {
      run_id: researchText(run.run_id), dataset_id: researchText(run.dataset_id),
      kind: researchText(run.kind), status: researchText(run.status),
      parameter_set_id: researchText(run.parameter_set_id) || undefined,
    };
  });
  return {
    available: status.available, app_data_dir: researchText(status.app_data_dir) || undefined,
    datasets, runs, active_parameter_set: scoringMetadata(status.active_parameter_set),
    active_dataset_id: researchText(status.active_dataset_id) || undefined,
  };
}

function portfolioMetrics(value: unknown): ResearchMetrics | undefined {
  const metrics = researchObject(value);
  if (!Object.keys(metrics).length) return undefined;
  return {
    total_return: researchNumber(metrics.total_return), annualized_return: researchNumber(metrics.annualized_return),
    max_drawdown: researchNumber(metrics.max_drawdown), annualized_turnover: researchNumber(metrics.annualized_turnover),
    total_transaction_cost: researchNumber(metrics.total_transaction_cost),
  };
}

export function parseResearchReport(value: unknown): ResearchReportView {
  const envelope = researchObject(value);
  const cli = envelope.ok === true ? researchObject(envelope.data) : envelope;
  const body = (cli.kind === "evaluation" || cli.kind === "calibration") && cli.report && typeof cli.report === "object"
    ? researchObject(cli.report) : cli;
  const evaluation = body.test && typeof body.test === "object" ? researchObject(body.test) : body;
  const quality = researchObject(evaluation.data_quality);
  const parameters = scoringMetadata(body.parameters || evaluation.parameters);
  const gate = researchObject(body.gate);
  const checks = Array.isArray(gate.checks) ? gate.checks.map((raw) => {
    const check = researchObject(raw);
    return { key: researchText(check.key), passed: check.passed === true, requirement: researchText(check.requirement), actual: researchNumber(check.actual) };
  }) : [];
  const legacy = !researchText(evaluation.data_fingerprint) || !researchText(evaluation.implementation_fingerprint)
    || !researchText(evaluation.research_fingerprint) || !researchText(evaluation.parameter_fingerprint)
    || !parameters.parameter_set_id || !parameters.scoring_revision || parameters.metric_contract_version !== 2;
  const realData = quality.real_data === true;
  const qualityPassed = quality.passed === true;
  // Native verification belongs to the current flat response, never nested report content.
  const qualificationVerified = body === envelope && envelope.qualification_verified === true;
  return {
    runId: researchText(body.run_id), datasetId: researchText(body.dataset_id), realData, qualityPassed, legacy, parameters,
    qualified: !legacy && realData && qualityPassed && gate.passed === true && checks.length > 0
      && checks.every((check) => check.passed) && qualificationVerified,
    metrics: portfolioMetrics(evaluation.metrics),
    baseline: portfolioMetrics(researchObject(body.fixed_baseline).metrics),
    issues: Array.isArray(quality.issues) ? quality.issues.filter((issue): issue is string => typeof issue === "string") : [],
    checks,
  };
}
