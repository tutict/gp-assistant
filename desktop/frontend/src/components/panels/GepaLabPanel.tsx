import { Play, Square, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { LlmClientConfig } from "../../types";
import { applyGepaRun, cancelGepaRun, gepaLabAvailable, getGepaReport, getGepaStatus, listenGepaEvents, startGepaRun, type GepaEvent, type GepaRunReport } from "../../lib/gepaLab";

const PROFILES = [
  { id: "hot_money_early_v1", label: "游资早期研究" },
  { id: "value_compounder_v1", label: "价值复利研究" },
] as const;
const BUDGETS = [30, 60, 120] as const;

export interface GepaLabPanelProps {
  llm?: LlmClientConfig;
  open: boolean;
  onClose: () => void;
  onAvailabilityChange: (enabled: boolean) => void;
  onApplied?: () => void;
}

export function GepaLabPanel({ llm, open, onClose, onAvailabilityChange, onApplied }: GepaLabPanelProps) {
  const [enabled, setEnabled] = useState(false);
  const [profileId, setProfileId] = useState<(typeof PROFILES)[number]["id"]>(PROFILES[0].id);
  const [budget, setBudget] = useState<number>(60);
  const [runId, setRunId] = useState<string>();
  const [baseVersion, setBaseVersion] = useState<string>();
  const [status, setStatus] = useState("idle");
  const [message, setMessage] = useState("");
  const [report, setReport] = useState<GepaRunReport>();
  const [error, setError] = useState<string>();
  const running = status === "running";
  const canStart = Boolean(enabled && llm?.model && (llm.api_key || llm.base_url) && !running);
  const profileLabel = useMemo(() => PROFILES.find((item) => item.id === profileId)?.label || profileId, [profileId]);

  useEffect(() => {
    if (!gepaLabAvailable()) {
      setEnabled(false);
      onAvailabilityChange(false);
      return;
    }
    let disposed = false;
    void getGepaStatus().then((result) => {
      if (disposed) return;
      const available = result.enabled === true;
      setEnabled(available);
      onAvailabilityChange(available);
    }).catch(() => {
      if (disposed) return;
      setEnabled(false);
      onAvailabilityChange(false);
    });
    return () => { disposed = true; };
  }, [onAvailabilityChange]);

  useEffect(() => {
    if (!runId) return;
    let disposed = false;
    let poll: number | undefined;
    let unlisten: (() => void) | undefined;
    let stopped = false;
    const stop = () => {
      stopped = true;
      if (poll !== undefined) window.clearInterval(poll);
      unlisten?.();
    };
    void listenGepaEvents((event: GepaEvent) => {
      if (disposed || event.run_id !== runId) return;
      if (event.message) setMessage(event.message);
      if (event.type === "complete" || event.type === "failed") {
        setStatus(event.type === "failed" ? "failed" : event.report?.status || "completed");
        if (event.report) setReport(event.report);
        if (event.message) setError(event.message);
        stop();
      }
    }).then((cleanup) => { if (stopped) cleanup?.(); else unlisten = cleanup; });
    poll = window.setInterval(() => {
      void getGepaReport(runId).then((next) => {
        if (disposed) return;
        setReport(next);
        setStatus(next.status);
        if (next.error) setError(next.error);
        if (next.status !== "running") stop();
      }).catch(() => undefined);
    }, 1_500);
    return () => { disposed = true; stop(); };
  }, [runId]);

  if (!gepaLabAvailable() || !enabled || !open) return null;

  async function start() {
    if (!llm) return;
    setError(undefined); setReport(undefined); setMessage("正在启动 GEPA…"); setStatus("running");
    try {
      const result = await startGepaRun(llm, profileId, budget);
      setRunId(result.run_id); setBaseVersion(result.base_prompt_version);
    } catch (cause) {
      setStatus("failed"); setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  async function cancel() {
    if (!runId) return;
    try { await cancelGepaRun(runId); setMessage("正在取消…"); } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  }

  async function apply() {
    if (!runId || !baseVersion) return;
    try { await applyGepaRun(runId, baseVersion); setMessage("候选已应用为本机提示词 overlay。"); onApplied?.(); } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  }

  return (
    <>
      {open && (
        <div className="agent-gepa-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
          <section className="agent-gepa-panel" role="dialog" aria-modal="true" aria-label="GEPA 提示词实验">
            <header className="agent-gepa-header">
              <div><span className="agent-gepa-kicker">PROMPT LAB</span><h2>GEPA 自进化实验</h2></div>
              <button type="button" className="icon-button" onClick={onClose} aria-label="关闭 GEPA 实验"><X size={17} /></button>
            </header>
            <p className="agent-gepa-description">只改写当前研究方法卡；固定安全规则、工具权限和证据校验不会进入优化对象。结果先评测，再由你手动应用。</p>
            <div className="agent-gepa-controls">
              <label>Profile<select value={profileId} disabled={running} onChange={(event) => setProfileId(event.target.value as typeof profileId)}>{PROFILES.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select></label>
              <label>Metric-call 预算<select value={budget} disabled={running} onChange={(event) => setBudget(Number(event.target.value))}>{BUDGETS.map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
            </div>
            <div className="agent-gepa-actions">
              {!running ? <button type="button" className="action-btn" disabled={!canStart} onClick={() => void start()}><Play size={15} />开始实验</button> : <button type="button" className="action-btn danger" onClick={() => void cancel()}><Square size={15} />取消运行</button>}
              <span className="agent-gepa-status" role="status">{message || (status === "idle" ? `将优化 ${profileLabel}` : status)}</span>
            </div>
            {error && <p className="agent-gepa-error" role="alert">{error}</p>}
            {report && <div className="agent-gepa-report">
              <div className="agent-gepa-score-grid"><div><span>基线验证</span><strong>{formatScore(report.baseline_validation_score)}</strong></div><div><span>基线 holdout</span><strong>{formatScore(report.baseline_holdout_score)}</strong></div><div><span>候选验证</span><strong>{formatScore(report.candidate_validation_score)}</strong></div><div><span>候选 holdout</span><strong>{formatScore(report.candidate_holdout_score)}</strong></div></div>
              <p className="agent-gepa-meta">{report.status} · {report.engine_version} · 数据集 {report.dataset_sha256.slice(0, 12)}…</p>
              <div className="agent-gepa-samples"><strong>候选验证样例</strong>{(report.candidate_validation || []).map((item) => <div key={item.id}><span>{item.id}</span><b>{formatScore(item.score)}</b>{item.feedback?.[0] && <small>{item.feedback[0]}</small>}</div>)}</div><div className="agent-gepa-samples"><strong>候选 holdout 样例</strong>{(report.candidate_holdout || []).map((item) => <div key={item.id}><span>{item.id}</span><b>{formatScore(item.score)}</b>{item.feedback?.[0] && <small>{item.feedback[0]}</small>}</div>)}</div>
              <button type="button" className="action-btn" disabled={report.status !== "completed" || !report.candidate_body} onClick={() => void apply()}>应用候选到本机 overlay</button>
            </div>}
            {!llm?.model && <p className="agent-gepa-hint">请先配置当前 Agent 模型连接。</p>}
          </section>
        </div>
      )}
    </>
  );
}

function formatScore(value?: number | null): string { return typeof value === "number" ? `${Math.round(value * 100)}%` : "--"; }
