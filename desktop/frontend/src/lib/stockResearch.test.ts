import { describe, expect, it } from "vitest";
import { parseResearchReport, parseResearchStatus } from "./stockResearch";

describe("research report evidence", () => {
  const completeReport = {
    run_id: "verified-run", dataset_id: "verified-dataset", data_fingerprint: "data",
    implementation_fingerprint: "core", research_fingerprint: "research", parameter_fingerprint: "parameters",
    parameters: { algorithm_version: "adaptive_swing_v2", parameter_set_id: "verified-params", scoring_revision: "unit-rank-fix/v1", metric_contract_version: 2 },
    data_quality: { passed: true, real_data: true, issues: [] },
    metrics: { annualized_return: 0.05 },
    gate: { passed: true, checks: [{ key: "complete", passed: true, requirement: "verified evidence" }] },
    qualification_verified: true,
  };

  it.each(["report", "data"])("honors the native refusal over a nested %s qualification claim", (field) => {
    const envelope = field === "report"
      ? { kind: "calibration", report: completeReport, qualification_verified: false }
      : { ok: true, data: completeReport, qualification_verified: false };
    const report = parseResearchReport(envelope);
    expect(report.metrics?.annualized_return).toBe(0.05);
    expect(report.qualified).toBe(false);
  });

  it.each(["report", "data"])("treats a generic %s envelope as historical content rather than current native authority", (field) => {
    const envelope = field === "report"
      ? { kind: "calibration", report: completeReport, qualification_verified: true }
      : { ok: true, data: completeReport, qualification_verified: true };
    expect(parseResearchReport(envelope).qualified).toBe(false);
  });

  it("accepts a complete flat native qualification and retains explicit rejection", () => {
    expect(parseResearchReport(completeReport).qualified).toBe(true);
    expect(parseResearchReport({ ...completeReport, qualification_verified: false }).qualified).toBe(false);
    expect(parseResearchReport({ ...completeReport, data_quality: { passed: true, real_data: false } }).qualified).toBe(false);
  });

  it("never presents a synthetic report as a qualified strategy", () => {
    const report = parseResearchReport({
      run_id: "fixture-run", dataset_id: "fixture-dataset", data_fingerprint: "data",
      implementation_fingerprint: "core", research_fingerprint: "research", parameter_fingerprint: "parameters",
      parameters: { algorithm_version: "adaptive_swing_v2", parameter_set_id: "fixture-params", scoring_revision: "unit-rank-fix/v1", metric_contract_version: 2 },
      data_quality: { passed: true, real_data: false, issues: [] },
      metrics: { total_return: 0.1, annualized_return: 0.02, max_drawdown: 0.04 },
      gate: { passed: true, checks: [] }, qualification_verified: true,
    });
    expect(report.realData).toBe(false);
    expect(report.qualified).toBe(false);
    expect(report.metrics?.annualized_return).toBe(0.02);
  });

  it("shows legacy metrics but refuses qualification without version fingerprints", () => {
    const report = parseResearchReport({ run_id: "old", metrics: { total_return: 0.3 }, gate: { passed: true } });
    expect(report.legacy).toBe(true);
    expect(report.qualified).toBe(false);
    expect(report.metrics?.total_return).toBe(0.3);
  });

  it("does not turn nonfinite metrics or malformed status into a valid report", () => {
    expect(parseResearchReport({ metrics: { annualized_return: Infinity } }).metrics?.annualized_return).toBeUndefined();
    expect(() => parseResearchStatus({ datasets: "invalid", runs: [] })).toThrow();
  });

  it("reads an empty local registry and its explicit fixed parameter version", () => {
    const status = parseResearchStatus({ available: false, datasets: [], runs: [], active_parameter_set: { algorithm_version: "adaptive_swing_v1", parameter_set_id: "fixed-v1", scoring_revision: "unit-rank-fix/v1" } });
    expect(status.available).toBe(false);
    expect(status.active_parameter_set.parameter_set_id).toBe("fixed-v1");
  });
});
