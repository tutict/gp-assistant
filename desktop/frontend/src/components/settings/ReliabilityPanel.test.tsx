import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ipc);
import { ReliabilityPanel } from './ReliabilityPanel';
const settings = { gepa_requested_enabled: false, gepa_effective_enabled: false, gepa_compiled: true, safe_start: false };
const snapshot = { schema_version: 1, previous_exit: 'unclean', settings, events: ['started', 'previous_unclean_exit'], counters: { started: 1, previous_unclean_exit: 1 }, dropped_events: 0 };
let renderer: ReactTestRenderer;
let click: ReturnType<typeof vi.fn>;
let downloaded: Blob | undefined;
const find = (label: string) => renderer.root.findByProps({ 'aria-label': label });
async function mount() { await act(async () => { renderer = create(<ReliabilityPanel />); }); }
async function preview() { await act(async () => find('预览本地诊断').props.onClick()); }
beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.clearAllMocks(); downloaded = undefined;
  ipc.invoke.mockImplementation(async (name: string) => {
    if (name === 'api_diagnostics_status') return settings;
    if (name === 'api_diagnostics_preview') return snapshot;
    if (name === 'api_diagnostics_set_gepa') return { ...settings, gepa_requested_enabled: true, gepa_effective_enabled: true };
    throw Error('unexpected IPC');
  });
  click=vi.fn();
  vi.stubGlobal('URL', { createObjectURL: vi.fn((blob: Blob) => { downloaded=blob; return 'blob:local-test'; }), revokeObjectURL: vi.fn() });
  vi.stubGlobal('document', { createElement: () => ({ click, remove: vi.fn(), href: '', download: '' }), body: { appendChild: vi.fn() } });
});
afterEach(async () => { if (renderer) await act(async () => renderer.unmount()); vi.unstubAllGlobals(); });
describe('ReliabilityPanel', () => {
  it('does not preview, upload or download until the user asks; exports precisely the confirmed preview', async () => {
    await mount(); expect(ipc.invoke.mock.calls.map(c=>c[0])).toEqual(['api_diagnostics_status']);
    expect(find('导出已预览诊断').props.disabled).toBe(true); expect(click).not.toHaveBeenCalled();
    await preview(); expect(JSON.stringify(renderer.toJSON())).toContain('不等于已验证崩溃');
    expect(find('导出已预览诊断').props.disabled).toBe(true);
    await act(async () => find('确认诊断导出').props.onChange({ currentTarget: { checked: true } }));
    await act(async () => find('导出已预览诊断').props.onClick());
    expect(click).toHaveBeenCalledTimes(1);
    expect(JSON.parse(await downloaded!.text())).toEqual(snapshot);
    expect(ipc.invoke.mock.calls.map(c=>c[0])).toEqual(['api_diagnostics_status','api_diagnostics_preview']);
  });
  it('persists GEPA only after an explicit toggle and invalidates any stale export', async () => {
    await mount(); await preview();
    expect(find('启用 GEPA 实验').props['aria-checked']).toBe(false);
    await act(async () => find('启用 GEPA 实验').props.onClick());
    expect(ipc.invoke).toHaveBeenLastCalledWith('api_diagnostics_set_gepa',{payload:{enabled:true}});
    expect(find('启用 GEPA 实验').props['aria-checked']).toBe(true);
    expect(find('导出已预览诊断').props.disabled).toBe(true);
  });
  it('safe start shows the stored preference but clearly reports the effective override', async () => {
    ipc.invoke.mockResolvedValueOnce({...settings,gepa_requested_enabled:true,safe_start:true});
    await mount(); expect(find('启用 GEPA 实验').props['aria-checked']).toBe(true);
    expect(JSON.stringify(renderer.toJSON())).toContain('安全启动：本次 GEPA 已禁用');
    expect(JSON.stringify(renderer.toJSON())).toContain('完整性检查始终保留');
  });
  it('rejects extra fields, unknown events, excessive quota and unsafe error strings without exporting', async () => {
    await mount();
    for (const bad of [{...snapshot,url:'https://secret'}, {...snapshot,events:['600000']}, {...snapshot,counters:{question:1}}, {...snapshot,counters:{started:1000001}}, {...snapshot,events:Array(65).fill('started')}, {...snapshot,settings:{...settings,key:'secret'}}]) {
      ipc.invoke.mockResolvedValueOnce(bad); await preview();
      expect(find('导出已预览诊断').props.disabled).toBe(true);
      expect(JSON.stringify(renderer.toJSON())).not.toContain('https://secret');
    }
    ipc.invoke.mockRejectedValueOnce(Error('secret-path-secret-key')); await preview();
    expect(JSON.stringify(renderer.toJSON())).not.toContain('secret-path-secret-key');
    expect(click).not.toHaveBeenCalled();
  });
  it('failed save retains old switch state and never displays raw backend errors', async () => {
    await mount(); ipc.invoke.mockRejectedValueOnce(Error('private filesystem path'));
    await act(async () => find('启用 GEPA 实验').props.onClick());
    expect(find('启用 GEPA 实验').props['aria-checked']).toBe(false);
    expect(JSON.stringify(renderer.toJSON())).not.toContain('private filesystem path');
  });
});

it('notifies the parent only after a successful persisted GEPA preference change', async () => {
  const changed=vi.fn();
  await act(async () => { renderer=create(<ReliabilityPanel onGepaPreferenceChange={changed} />); });
  ipc.invoke.mockRejectedValueOnce(Error('failed write'));
  await act(async () => find('启用 GEPA 实验').props.onClick());
  expect(changed).not.toHaveBeenCalled();
  await act(async () => find('启用 GEPA 实验').props.onClick());
  expect(changed).toHaveBeenCalledTimes(1);
});
