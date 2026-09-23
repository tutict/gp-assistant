import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../panels/LlmSettingsPanel", () => ({
  LlmSettingsPanel: (props: { presentation?: string }) => (
    <button type="button" data-presentation={props.presentation}>模型连接</button>
  ),
}));

import { SettingsSheet } from "./SettingsSheet";

describe("SettingsSheet", () => {
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", {
      activeElement: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
  });

  it("renders an empty settings dialog and closes from its backdrop", async () => {
    const onClose = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SettingsSheet open onClose={onClose} settings={[]} />,
        { createNodeMock: () => ({ querySelectorAll: () => [] }) },
      );
    });

    expect(renderer.root.findByProps({ role: "dialog", "aria-label": "设置" })).toBeTruthy();
    const backdrop = renderer.root.findByProps({ className: "sheet-backdrop settings-sheet-backdrop" });
    await act(async () => {
      backdrop.props.onMouseDown({ currentTarget: backdrop, target: backdrop });
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("renders segmented settings from descriptors", async () => {
    const setDensity = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SettingsSheet
          open
          onClose={vi.fn()}
          settings={[{
            key: "density",
            title: "信息密度",
            type: "segmented",
            options: [
              { value: "comfortable", label: "舒适" },
              { value: "compact", label: "紧凑" },
            ],
            get: () => "comfortable",
            set: setDensity,
          }]}
        />,
        { createNodeMock: () => ({ querySelectorAll: () => [] }) },
      );
    });

    expect(renderer.root.findByProps({ className: "settings-item-title" }).children).toEqual(["信息密度"]);
    const compact = renderer.root.findByProps({ "aria-label": "信息密度：紧凑" });
    await act(async () => compact.props.onClick());
    expect(setDensity).toHaveBeenCalledWith("compact");
  });

  it("keeps model connection as the existing dialog inside settings", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SettingsSheet open onClose={vi.fn()} settings={[]} llmSettings={null} onLlmSettingsChange={vi.fn()} />,
        { createNodeMock: () => ({ querySelectorAll: () => [] }) },
      );
    });

    const model = renderer.root.findByProps({ "data-presentation": "dialog" });
    expect(model.children).toContain("模型连接");
    expect(renderer.root.findAllByProps({ "aria-label": "模型连接配置" })).toHaveLength(0);
  });
});
