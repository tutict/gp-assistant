import { invoke } from '@tauri-apps/api/core';
import { useEffect, useId, useRef, useState } from 'react';

const EVENTS = ['started', 'clean_shutdown', 'previous_unclean_exit', 'storage_check_failed', 'storage_recovery_completed', 'offline_operation_completed', 'task_completed', 'task_failed', 'task_cancelled', 'background_request', 'gepa_setting_changed', 'gepa_blocked'] as const;
type OperationalEvent = typeof EVENTS[number];
interface FeatureStatus {
  gepa_requested_enabled: boolean;
  gepa_effective_enabled: boolean;
  gepa_compiled: boolean;
  safe_start: boolean;
}
interface Preview {
  schema_version: 1;
  previous_exit: 'first_run' | 'clean' | 'unclean';
  settings: FeatureStatus;
  events: OperationalEvent[];
  counters: Partial<Record<OperationalEvent, number>>;
  dropped_events: number;
}
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw Error('invalid diagnostics');
  return value as Record<string, unknown>;
}
function keys(value: Record<string, unknown>, allowed: string[]) {
  if (Object.keys(value).length !== allowed.length || Object.keys(value).some(key => !allowed.includes(key))) throw Error('invalid diagnostics');
}
function parseSettings(raw: unknown): FeatureStatus {
  const value=object(raw);
  keys(value, ['gepa_requested_enabled', 'gepa_effective_enabled', 'gepa_compiled', 'safe_start']);
  if (Object.values(value).some(item => typeof item !== 'boolean') || (value.gepa_effective_enabled && (!value.gepa_requested_enabled || !value.gepa_compiled || value.safe_start))) throw Error('invalid diagnostics');
  return { gepa_requested_enabled: value.gepa_requested_enabled as boolean,
    gepa_effective_enabled: value.gepa_effective_enabled as boolean, gepa_compiled: value.gepa_compiled as boolean, safe_start: value.safe_start as boolean };
}
function counter(value: unknown): value is number { return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 1_000_000; }
function event(value: unknown): value is OperationalEvent { return typeof value === 'string' && EVENTS.some(item => item === value); }
/** Defense in depth: reject, rather than render/stringify, unexpected IPC data. */
function parsePreview(raw: unknown): Preview {
  const value=object(raw);
  keys(value, ['schema_version','previous_exit','settings','events','counters','dropped_events']);
  if (value.schema_version !== 1 || !['first_run','clean','unclean'].includes(value.previous_exit as string) || !Array.isArray(value.events) || value.events.length > 64 || !value.events.every(event) || !counter(value.dropped_events)) throw Error('invalid diagnostics');
  const counters=object(value.counters);
  if (Object.entries(counters).some(([key, count]) => !event(key) || !counter(count))) throw Error('invalid diagnostics');
  return { schema_version:1, previous_exit:value.previous_exit as Preview['previous_exit'], settings:parseSettings(value.settings),
    events:[...value.events] as OperationalEvent[], counters:{...counters} as Preview['counters'], dropped_events:value.dropped_events };
}

/** No remote transport, free-text entry, device identity or automatic downloads. */
export function ReliabilityPanel({ onGepaPreferenceChange }: { onGepaPreferenceChange?: () => void } = {}) {
  const headingId=useId();
  const [settings,setSettings]=useState<FeatureStatus | null>(null);
  const [preview,setPreview]=useState<Preview | null>(null);
  const [confirmed,setConfirmed]=useState(false);
  const [busy,setBusy]=useState(false);
  const [message,setMessage]=useState('');
  const inFlight=useRef(false);
  useEffect(() => {
    let active=true;
    void invoke('api_diagnostics_status').then(raw => { if (active) setSettings(parseSettings(raw)); })
      .catch(() => { if (active) setMessage('本地可靠性控制不可用，请检查启动状态。'); });
    return () => { active=false; };
  }, []);
  async function run(operation: () => Promise<void>) {
    if (inFlight.current) return;
    inFlight.current=true; setBusy(true); setMessage(''); setConfirmed(false); setPreview(null);
    try { await operation(); }
    catch { setMessage('本地操作未完成；未导出数据，设置以已保存状态为准。'); }
    finally { inFlight.current=false; setBusy(false); }
  }
  function download() {
    if (!confirmed || !preview || busy || inFlight.current) return;
    // Only this synchronous user gesture creates a file, and only from the visible snapshot.
    const bytes=JSON.stringify(preview,null,2);
    let url: string | undefined;
    let anchor: HTMLAnchorElement | undefined;
    try {
      url=URL.createObjectURL(new Blob([bytes],{type:'application/json'}));
      anchor=document.createElement('a'); anchor.href=url; anchor.download='gp-local-diagnostics.json';
      document.body.appendChild(anchor); anchor.click();
      setConfirmed(false); setMessage('已请求本地下载，请检查下载目录；未上传任何数据。');
    } catch { setMessage('本地下载不可用；未上传数据。可手动复制已预览内容。'); }
    finally {
      anchor?.remove();
      if (url) { const localUrl=url; setTimeout(() => URL.revokeObjectURL(localUrl),1000); }
    }
  }
  return (
    <section className="settings-item-copy" style={{ minWidth:0, overflowWrap:'anywhere' }} aria-labelledby={headingId} aria-busy={busy}>
      <h3 id={headingId}>本地诊断与安全控制</h3>
      <p>只保留本次运行的固定操作类别与计数，最多 64 条；不记录问题、股票代码、网址、路径、密钥或业务内容。没有自动上传。</p>
      <div>
        <label>GEPA 实验（默认关闭） </label>
        <button type="button" className="settings-toggle" role="switch" aria-label="启用 GEPA 实验"
          aria-checked={settings?.gepa_requested_enabled ?? false} disabled={busy || !settings || !settings.gepa_compiled}
          onClick={() => run(async () => {
            if (!settings) return;
            setSettings(parseSettings(await invoke('api_diagnostics_set_gepa',{payload:{enabled:!settings.gepa_requested_enabled}})));
            onGepaPreferenceChange?.();
          })}><span aria-hidden="true" /></button>
      </div>
      <p>{settings?.safe_start ? '安全启动：本次 GEPA 已禁用，保存的偏好不变。' : settings?.gepa_effective_enabled ? 'GEPA 当前已启用。' : 'GEPA 当前未启用。'}完整性检查始终保留。</p>
      {settings && !settings.gepa_compiled ? <p>此构建未包含 GEPA 实验功能。</p> : null}
      <p>安全启动：以 --safe-start 启动，或启动前设置 GP_ASSISTANT_SAFE_START=1。仅限制 GEPA，不是远程控制。</p>
      <button type="button" className="clear-btn" aria-label="预览本地诊断" disabled={busy || !settings}
        onClick={() => run(async () => { const next=parsePreview(await invoke('api_diagnostics_preview')); setPreview(next); setSettings(next.settings); })}>预览本地诊断</button>
      {preview ? <>
        <p>{preview.previous_exit === 'unclean' ? '上次未正常退出：不等于已验证崩溃，也可能是强制停止或断电。' : preview.previous_exit === 'clean' ? '上次已记录正常退出。' : '尚无上次退出记录。'}</p>
        <pre aria-label="诊断导出预览" style={{ whiteSpace:'pre-wrap', overflowWrap:'anywhere', maxHeight:'16rem', overflowY:'auto' }}>{JSON.stringify(preview,null,2)}</pre>
      </> : null}
      <label>
        <input type="checkbox" aria-label="确认诊断导出" checked={confirmed} disabled={!preview || busy}
          onChange={event => setConfirmed(event.currentTarget.checked)} />
        我已检查上述内容，并确认仅导出到本机。
      </label>
      <button type="button" className="save-btn" aria-label="导出已预览诊断" disabled={!preview || !confirmed || busy} onClick={download}>导出已预览诊断</button>
      {busy ? <p role="status">正在处理本地操作…</p> : null}
      {message ? <p role="status">{message}</p> : null}
    </section>
  );
}
