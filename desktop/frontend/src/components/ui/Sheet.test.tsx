import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Sheet } from "./Sheet";

describe("Sheet", () => {
  const listeners = new Map<string, EventListener>();
  const trigger = { focus: vi.fn() };
  const firstControl = { focus: vi.fn() };

  beforeEach(() => {
    listeners.clear();
    trigger.focus.mockClear();
    firstControl.focus.mockClear();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", {
      activeElement: trigger,
      addEventListener: vi.fn((type: string, listener: EventListener) => listeners.set(type, listener)),
      removeEventListener: vi.fn((type: string) => listeners.delete(type)),
    });
  });

  it("focuses the first control and restores the trigger after Escape closes it", async () => {
    const onClose = vi.fn();
    let renderer!: ReactTestRenderer;
    const renderSheet = (open: boolean) => (
      <Sheet open={open} onClose={onClose} label="设置" className="settings-sheet">
        <button type="button">首个控件</button>
      </Sheet>
    );

    await act(async () => {
      renderer = create(renderSheet(true), {
        createNodeMock: (element) => element.type === "section"
          ? { querySelectorAll: () => [firstControl], contains: () => true }
          : {},
      });
    });

    expect(firstControl.focus).toHaveBeenCalledOnce();
    await act(async () => {
      listeners.get("keydown")?.({ key: "Escape", preventDefault: vi.fn() } as unknown as Event);
    });
    expect(onClose).toHaveBeenCalledOnce();

    await act(async () => {
      renderer.update(renderSheet(false));
    });
    expect(trigger.focus).toHaveBeenCalledOnce();
    expect(renderer.toJSON()).toBeNull();
  });
});
