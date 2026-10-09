import { useCallback, useEffect, useState } from "react";
import {
  deleteEvolutionRule,
  editEvolutionRule,
  getEvolutionProfile,
  resetEvolutionProfile,
  setEvolutionRuleStatus,
  suppressEvolutionKind,
  unsuppressEvolutionKind,
  updateEvolutionSettings,
  type EvolutionProfile,
  type EvolutionRule,
} from "../../lib/evolution";

export function EvolutionProfilePanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [profile, setProfile] = useState<EvolutionProfile>();
  const [editing, setEditing] = useState<string>();
  const [editText, setEditText] = useState("");
  const [error, setError] = useState<string>();

  const reload = useCallback(() => {
    let cancelled = false;
    void getEvolutionProfile()
      .then((value) => { if (!cancelled) setProfile(value); })
      .catch((cause) => { if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause)); });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => { if (!open) return undefined; return reload(); }, [open, reload]);
  if (!open) return null;

  const settings = profile?.settings;
  const toggle = async (enabled: boolean) => {
    try {
      const next = await updateEvolutionSettings({ enabled, coach_mode: enabled });
      setProfile((previous) => previous ? { ...previous, settings: next } : previous);
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const reset = async () => {
    try { setProfile(await resetEvolutionProfile()); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const startEdit = (rule: EvolutionRule) => { setEditing(rule.rule_id); setEditText(rule.statement); };
  const saveEdit = async (rule: EvolutionRule) => {
    try {
      setProfile(await editEvolutionRule({ rule_id: rule.rule_id, statement: editText, evidence_refs: rule.evidence_refs }));
      setEditing(undefined);
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };
  const changeStatus = async (ruleId: string, status: string) => {
    try { setProfile(await setEvolutionRuleStatus(ruleId, status)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
  };

  return <div className="evolution-modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="evolution-modal" role="dialog" aria-modal="true" aria-label="我的研究画像">
      <header><div><span className="evolution-kicker">PERSONAL PROFILE</span><h2>我的研究画像</h2></div><button type="button" onClick={onClose} aria-label="关闭画像">×</button></header>
      <p className="evolution-note">默认本地优先，不保存完整原始对话。个人画像不会发送给远程 Agent，也不会修改全局 GEPA。</p>
      <label className="evolution-switch"><input type="checkbox" checked={settings?.enabled === true} onChange={(event) => void toggle(event.target.checked)} /> 开启研究教练</label>
      <p>画像版本：{profile?.profile_version ?? "--"}</p>
      <div className="evolution-rule-list">
        {profile?.rules.length ? profile.rules.map((rule) => <article className={`evolution-rule status-${rule.status}`} key={rule.rule_id}>
          {editing === rule.rule_id ? <textarea value={editText} onChange={(event) => setEditText(event.target.value)} aria-label={`编辑规则 ${rule.rule_id}`} /> : <p>{rule.statement}</p>}
          <small>来源复盘：{rule.source_review_id} · 证据：{rule.evidence_refs.join(", ") || "无"} · 状态：{rule.status}</small>
          <div className="evolution-rule-actions">
            {editing === rule.rule_id ? <><button type="button" onClick={() => void saveEdit(rule)}>保存编辑</button><button type="button" onClick={() => setEditing(undefined)}>取消</button></> : <button type="button" onClick={() => startEdit(rule)}>编辑</button>}
            {rule.status === "active" && <button type="button" onClick={() => void changeStatus(rule.rule_id, "paused")}>暂停</button>}
            {rule.status === "paused" && <button type="button" onClick={() => void changeStatus(rule.rule_id, "active")}>恢复</button>}
            {rule.status !== "never_suggested" && <button type="button" onClick={() => void suppressEvolutionKind({ kind: rule.kind, statement: rule.statement, source_review_id: rule.source_review_id }).then(() => changeStatus(rule.rule_id, "never_suggested")).catch((cause) => setError(cause instanceof Error ? cause.message : String(cause)))}>以后不再建议此类</button>}
            <button type="button" onClick={() => void deleteEvolutionRule(rule.rule_id).then(setProfile).catch((cause) => setError(cause instanceof Error ? cause.message : String(cause)))}>删除</button>
          </div>
        </article>) : <p>还没有确认的研究规则。</p>}
      </div>
      {!!profile?.suppressed_kinds?.length && <section className="evolution-suppressed"><h3>已禁用的建议类型</h3>{profile.suppressed_kinds.map((kind) => <div key={kind}><span>{kind}</span><button type="button" onClick={() => void unsuppressEvolutionKind(kind).then(setProfile).catch((cause) => setError(cause instanceof Error ? cause.message : String(cause)))}>恢复建议</button></div>)}</section>}
      {error && <p className="evolution-error" role="alert">{error}</p>}
      <footer><button type="button" className="danger-btn" onClick={() => void reset()}>清空画像</button><button type="button" className="action-btn" onClick={onClose}>完成</button></footer>
    </section>
  </div>;
}
