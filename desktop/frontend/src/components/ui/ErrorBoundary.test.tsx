import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";
let renderer: ReactTestRenderer;
afterEach(async () => { if (renderer) await act(async () => renderer.unmount()); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
function Broken(): never { throw new Error("render failed"); }
it("isolates a failed panel without clearing storage and can retry", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true); vi.spyOn(console, "error").mockImplementation(() => {});
  const clear = vi.fn(); vi.stubGlobal("localStorage", { clear });
  await act(async () => { renderer = create(<><div>other panel</div><ErrorBoundary><Broken /></ErrorBoundary></>); });
  expect(JSON.stringify(renderer.toJSON())).toContain("other panel"); expect(JSON.stringify(renderer.toJSON())).toContain("本地数据不会被清除");
  await act(async () => { renderer.update(<ErrorBoundary resetKey="new"><div>recovered</div></ErrorBoundary>); });
  expect(JSON.stringify(renderer.toJSON())).toContain("recovered"); expect(clear).not.toHaveBeenCalled();
});
it("does not reload while a failed save would lose in-memory edits", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true); vi.spyOn(console, "error").mockImplementation(() => {});
  const reload = vi.fn(); const beforeReload = vi.fn(async () => false);
  await act(async () => { renderer = create(<ErrorBoundary reload={reload} beforeReload={beforeReload}><Broken /></ErrorBoundary>); });
  await act(async () => renderer.root.findAllByType("button")[1].props.onClick());
  expect(beforeReload).toHaveBeenCalledOnce(); expect(reload).not.toHaveBeenCalled(); expect(JSON.stringify(renderer.toJSON())).toContain("已阻止重新加载");
});
