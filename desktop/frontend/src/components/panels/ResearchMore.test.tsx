import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, expect, it, vi } from "vitest";
import { ResearchMore } from "./ResearchMore";
let renderer: ReactTestRenderer | undefined;
afterEach(async () => { await act(async () => renderer?.unmount()); renderer = undefined; vi.unstubAllGlobals(); });
it("closes More on Escape and outside pointer events and restores its trigger", async () => {
  class Target {}
  vi.stubGlobal("Node", Target);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const listeners = new Map<string, (event: unknown) => void>();
  const trigger = { focus: vi.fn() };
  const first = { focus: vi.fn() };
  const inside = new Target();
  vi.stubGlobal("document", { addEventListener: (type: string, handler: (event: unknown) => void) => listeners.set(type, handler), removeEventListener: (type: string) => listeners.delete(type) });
  await act(async () => { renderer = create(<ResearchMore onKnowledge={vi.fn()} onModel={vi.fn()} onStatus={vi.fn()} />, { createNodeMock: (node) => node.type === "button" ? trigger : { querySelector: () => first, contains: (target: unknown) => target === inside } }); });
  const button = renderer!.root.findByProps({ "aria-label": "更多研究操作" });
  await act(async () => button.props.onClick());
  expect(first.focus).toHaveBeenCalledTimes(1);
  await act(async () => listeners.get("pointerdown")?.({ target: inside }));
  expect(button.props["aria-expanded"]).toBe(true);
  await act(async () => listeners.get("keydown")?.({ key: "Escape", preventDefault: vi.fn() }));
  expect(button.props["aria-expanded"]).toBe(false);
  expect(trigger.focus).toHaveBeenCalledTimes(1);
  await act(async () => button.props.onClick());
  await act(async () => listeners.get("pointerdown")?.({ target: new Target() }));
  expect(button.props["aria-expanded"]).toBe(false);
  expect(trigger.focus).toHaveBeenCalledTimes(2);
  expect(listeners.size).toBe(0);
});
