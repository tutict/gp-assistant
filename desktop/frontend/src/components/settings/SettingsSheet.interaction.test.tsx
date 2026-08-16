import { useState } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDensity } from "../../hooks/useDensity";
import { SettingsSheet } from "./SettingsSheet";

function SettingsHarness() {
  const [open, setOpen] = useState(false);
  const { density, setDensity } = useDensity();
  return (
    <>
      <button type="button" className="settings-harness-trigger" onClick={() => setOpen(true)}>
        设置
      </button>
      <SettingsSheet
        open={open}
        onClose={() => setOpen(false)}
        settings={[{
          key: "density",
          title: "信息密度",
          type: "segmented",
          options: [
            { value: "comfortable", label: "舒适" },
            { value: "compact", label: "紧凑" },
          ],
          get: () => density,
          set: (value) => {
            if (value === "comfortable" || value === "compact") setDensity(value);
          },
        }]}
      />
    </>
  );
}

describe("SettingsSheet interaction", () => {
  const values = new Map<string, string>();
  const listeners = new Map<string, EventListener>();
  const triggerNode = { focus: vi.fn() };
  const firstControl = { focus: vi.fn() };

  beforeEach(() => {
    values.clear();
    listeners.clear();
    triggerNode.focus.mockClear();
    firstControl.focus.mockClear();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("localStorage", {
      getItem: vi.fn((key: string) => values.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    });
    vi.stubGlobal("window", { location: { search: "" } });
    vi.stubGlobal("document", {
      activeElement: triggerNode,
      documentElement: { dataset: {} },
      addEventListener: vi.fn((type: string, listener: EventListener) => listeners.set(type, listener)),
      removeEventListener: vi.fn((type: string) => listeners.delete(type)),
    });
  });

  it("opens, persists density, and restores focus after Escape", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<SettingsHarness />, {
        createNodeMock: (element) => {
          if (element.type === "section") {
            return { querySelectorAll: () => [firstControl], focus: vi.fn() };
          }
          const props = element.props as { className?: string };
          if (props.className === "settings-harness-trigger") return triggerNode;
          return {};
        },
      });
    });

    await act(async () => {
      renderer.root.findByProps({ className: "settings-harness-trigger" }).props.onClick();
    });
    expect(renderer.root.findByProps({ role: "dialog", "aria-label": "设置" })).toBeTruthy();

    await act(async () => {
      renderer.root.findByProps({ "aria-label": "信息密度：紧凑" }).props.onClick();
    });
    expect(values.get("stock-optimizer-density")).toBe("compact");

    await act(async () => {
      listeners.get("keydown")?.({ key: "Escape", preventDefault: vi.fn() } as unknown as Event);
    });
    expect(renderer.root.findAllByProps({ role: "dialog", "aria-label": "设置" })).toHaveLength(0);
    expect(triggerNode.focus).toHaveBeenCalledOnce();
  });
});
