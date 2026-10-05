import { useState } from "react";
import type { LlmClientConfig } from "../../types";
import type { EvolutionReview } from "../../lib/evolution";
import { confirmEvolutionRule, enhanceEvolutionReview, suppressEvolutionKind } from "../../lib/evolution";

export function EvolutionReviewCard({ review: initialReview, llm, onClose }: { review: EvolutionReview; llm?: LlmClientConfig; onClose: () => void }) {
  const [review, setReview] = useState(initialReview);
  const [confirmed, setConfirmed] = useState<Set<string>>(new Set());
  const [suppressed, setSuppressed] = useState<Set<string>>(new Set());
  const [enhancing, setEnhancing] = useState(false);
  const [error, setError] = useState<string>();
  const confirm = async (candidate: EvolutionReview["blind_spot_candidates"][number], index: number) => {
    const ruleId = `${review.review_id}-${index}`;
    try {
      await confirmEvolutionRule({ rule_id: ruleId, kind: candidate.kind, statement: candidate.statement, source_review_id: review.review_id, evidence_refs: candidate.evidence_refs });
      setConfirmed((previous) => new Set(previous).add(ruleId));
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const suppress = async (candidate: EvolutionReview["blind_spot_candidates"][number]) => {
    try {
      await suppressEvolutionKind({ kind: candidate.kind, statement: candidate.statement, source_review_id: review.review_id });
      setSuppressed((previous) => new Set(previous).add(candidate.kind));
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const enhance = async () => {
    if (!llm) return;
    setEnhancing(true); setError(undefined);
    try { setReview(await enhanceEvolutionReview(review, llm as unknown as Record<string, unknown>)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
    finally { setEnhancing(false); }
  };
  return (
    <div className="evolution-modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="evolution-modal" role="dialog" aria-modal="true" aria-label="研究复盘">
        <header><div><span className="evolution-kicker">RESEARCH COACH</span><h2>本次研究复盘</h2></div><button type="button" onClick={onClose} aria-label="关闭复盘">×</button></header>
        <p className="evolution-note">复盘只分析研究过程，不做人格诊断，也不会修改全局 GEPA 方法卡。模型增强仅允许使用 localhost 模型。</p>
        {review.model_feedback && <section><h3>模型增强反馈</h3><p>{review.model_feedback}</p></section>}
        <section><h3>研究目标</h3><p>{review.research_goal || "未读取到研究目标"}</p></section>
        <section><h3>证据与过程</h3><p>证据引用 {review.evidence_and_process.evidence_ids.length} 条 · 工具步骤 {review.evidence_and_process.tool_calls.length} 步</p></section>
        <section><h3>结论质量</h3><p>{review.conclusion_quality.has_answer ? review.conclusion_quality.answer : "没有可复盘的回答正文。"}</p></section>
        <section><h3>研究习惯候选</h3>{review.blind_spot_candidates.length === 0 ? <p>本次没有发现可确认的研究过程改进项。</p> : review.blind_spot_candidates.map((candidate, index) => { const id = `${review.review_id}-${index}`; const done = confirmed.has(id); const muted = suppressed.has(candidate.kind); return <article key={id} className="evolution-candidate"><p>{candidate.statement}</p><small>证据引用：{candidate.evidence_refs.length ? candidate.evidence_refs.join(", ") : "无"}</small><div>{!muted && <button type="button" disabled={done || candidate.evidence_refs.length === 0} onClick={() => void confirm(candidate, index)}>{done ? "已加入研究画像" : candidate.evidence_refs.length ? "确认加入画像" : "缺少证据，不能确认"}</button>}{!muted && <button type="button" onClick={() => void suppress(candidate)}>以后不再建议</button>}{muted && <small>已标记为以后不再建议</small>}</div></article>; })}</section>
        {error && <p className="evolution-error" role="alert">{error}</p>}
        <footer><button type="button" className="btn" disabled={!llm || enhancing} onClick={() => void enhance()}>{enhancing ? "本地模型增强中…" : "本地模型增强复盘"}</button><button type="button" className="action-btn" onClick={onClose}>完成</button></footer>
      </section>
    </div>
  );
}
