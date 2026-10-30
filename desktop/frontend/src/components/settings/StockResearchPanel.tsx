import { useEffect, useState } from "react";
import { getJson } from "../../lib/tauri";
import { parseResearchReport, parseResearchStatus } from "../../lib/stockResearch";
import type { ResearchReportView, ResearchStatus } from "../../lib/stockResearch";
import "../../styles/stock-research.css";

function metricPercent(value: number | undefined): string {
  return value === undefined ? "—" : `${(value * 100).toFixed(2)}%`;
}

export function StockResearchPanel() {
  const [status, setStatus] = useState<ResearchStatus | null>(null);
  const [report, setReport] = useState<ResearchReportView | null>(null);
  const [loading, setLoading] = useState(true);
  const [reportLoading, setReportLoading] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let cancelled = false;
    void getJson("/api/stock-research/status").then((value) => {
      if (!cancelled) setStatus(parseResearchStatus(value));
    }).catch(() => { if (!cancelled) setError("研究记录暂不可读，请检查本地研究工具输出后重试。"); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, []);

  async function refresh() {
    setLoading(true); setError("");
    try { setStatus(parseResearchStatus(await getJson("/api/stock-research/status"))); }
    catch { setError("研究记录暂不可读，请检查本地研究工具输出后重试。"); }
    finally { setLoading(false); }
  }

  async function loadReport(runId: string) {
    setReportLoading(true); setError("");
    try { setReport(parseResearchReport(await getJson(`/api/stock-research/report?run_id=${encodeURIComponent(runId)}`))); }
    catch { setError("这份研究报告暂不可读，请重新生成或选择其他记录。"); }
    finally { setReportLoading(false); }
  }

  return <div className="stock-research-panel" aria-label="A 股选股研究与验证">
    <div className="stock-research-header"><strong>选股研究与验证</strong><button type="button" className="action-btn" disabled={loading} onClick={() => void refresh()}>刷新记录</button></div>
    <p className="stock-research-description">PIT 数据导入、回测与权重校准由本地命令行完成。这里查看数据覆盖和冻结的验证结果。</p>
    {loading ? <p role="status">读取本地研究记录…</p> : null}
    {error ? <p role="alert">{error}</p> : null}
    {status ? <>
      <dl className="stock-research-facts"><div><dt>当前算法</dt><dd>{status.active_parameter_set.algorithm_version || "未记录"}</dd></div><div><dt>参数版本</dt><dd>{status.active_parameter_set.parameter_set_id || "未记录"}</dd></div><div><dt>评分修订</dt><dd>{status.active_parameter_set.scoring_revision || "旧版本未记录"}</dd></div></dl>
      {status.active_dataset_id ? <p className="stock-research-description">当前冻结数据集：{status.active_dataset_id}</p> : null}
      {status.app_data_dir ? <details><summary>研究数据目录</summary><code className="stock-research-path">{status.app_data_dir}</code></details> : null}
      {status.datasets.length === 0 ? <p className="stock-research-description">暂无 PIT 数据。准备完整历史数据并通过校验后，才能取得真实策略效果证据。</p> : <div className="stock-research-datasets">{status.datasets.map((dataset) => <div key={dataset.dataset_id}><strong>{dataset.name || dataset.dataset_id}</strong><span>{dataset.real_data ? "真实数据声明" : "合成样例"} · {dataset.start_date} 至 {dataset.end_date}</span><span>{dataset.source}</span></div>)}</div>}
      {status.runs.length ? <ul className="stock-research-runs">{status.runs.map((run) => <li key={run.run_id}><span>{run.kind === "calibration" ? "权重校准" : "严格回测"} · {run.status}</span><button type="button" className="action-btn" aria-label={`查看研究报告 ${run.run_id}`} disabled={reportLoading} onClick={() => loadReport(run.run_id)}>查看报告</button></li>)}</ul> : <p className="stock-research-description">暂无验证报告。</p>}
    </> : null}
    {report ? <section className="stock-research-report" aria-label="研究验证报告">
      <h4>验证报告</h4>
      <dl className="stock-research-facts"><div><dt>报告标识</dt><dd>{report.runId || "旧报告未记录"}</dd></div><div><dt>数据集</dt><dd>{report.datasetId || "旧报告未记录"}</dd></div><div><dt>本报告算法</dt><dd>{report.parameters.algorithm_version || "旧报告未记录"}</dd></div><div><dt>本报告参数版本</dt><dd>{report.parameters.parameter_set_id || "旧报告未记录"}</dd></div></dl>
      <p>{report.realData ? "真实数据研究" : "合成样例 · 不提供真实收益证据"} · {report.qualityPassed ? "数据校验满足" : "数据校验未满足"}</p>
      <p>{report.qualified ? "门槛满足；启用由命令行显式操作" : "未取得启用资格"}</p>
      {report.legacy ? <p className="stock-research-description">旧报告缺少数据或参数版本，不能作为当前启用依据。</p> : null}
      <dl className="stock-research-facts"><div><dt>净总收益</dt><dd>{metricPercent(report.metrics?.total_return)}</dd></div><div><dt>净年化收益</dt><dd>{metricPercent(report.metrics?.annualized_return)}</dd></div><div><dt>最大回撤</dt><dd>{metricPercent(report.metrics?.max_drawdown)}</dd></div><div><dt>固定权重年化</dt><dd>{metricPercent(report.baseline?.annualized_return)}</dd></div></dl>
      {report.issues.length ? <ul>{report.issues.map((issue, index) => <li key={`${index}:${issue}`}>{issue}</li>)}</ul> : null}
      {report.checks.length ? <div className="stock-research-table"><table><thead><tr><th scope="col">验证门槛</th><th scope="col">结果</th></tr></thead><tbody>{report.checks.map((check, index) => <tr key={`${index}:${check.key}`}><td>{check.requirement || check.key.replaceAll("_", " ")}</td><td>{check.passed ? "满足" : "未满足"}</td></tr>)}</tbody></table></div> : null}
    </section> : null}
    <p className="stock-research-description">仅供研究，不构成投资建议。</p>
  </div>;
}
