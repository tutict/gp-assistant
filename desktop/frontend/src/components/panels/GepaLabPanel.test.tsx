import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
const api = vi.hoisted(() => ({ gepaLabAvailable: vi.fn(()=>true), getGepaStatus: vi.fn(), startGepaRun: vi.fn(), applyGepaRun: vi.fn(), cancelGepaRun: vi.fn(), getGepaReport: vi.fn(), listenGepaEvents: vi.fn() }));
vi.mock('../../lib/gepaLab', () => api);
import { GepaLabPanel } from './GepaLabPanel';
let renderer: ReactTestRenderer;
beforeEach(() => { vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true); vi.clearAllMocks(); api.getGepaStatus.mockResolvedValue({enabled:true}); });
afterEach(async () => { if(renderer) await act(async()=>renderer.unmount()); vi.unstubAllGlobals(); });
it('allows a model backed by credential_ref without a plaintext API key or custom URL', async () => {
  await act(async()=>{renderer=create(<GepaLabPanel open llm={{model:'test-model',credential_ref:'provider-test'}} onClose={()=>{}} onAvailabilityChange={()=>{}} />);});
  const start=renderer.root.findAllByType('button').find(button => button.props.className === 'action-btn' && button.props.children.some?.((child: unknown)=>typeof child==='string' && child.includes('开始实验')));
  expect(start, 'visible start control').toBeDefined();
  expect(start!.props.disabled).toBe(false);
});

