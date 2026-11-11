import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../panels/LlmSettingsPanel", () => ({
  LlmSettingsPanel: (props: { presentation?: string }) => (
    <button type="button" data-presentation={props.presentation}>模型连接</button>
  ),
}));
vi.mock("../../lib/tauri", () => ({ getJson: vi.fn() }));

import { SettingsSheet } from "./SettingsSheet";
import { getJson } from "../../lib/tauri";
const getJsonMock = vi.mocked(getJson);

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

  it("includes the network proxy controls in settings", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <SettingsSheet open onClose={vi.fn()} settings={[]} />,
        { createNodeMock: () => ({ querySelectorAll: () => [] }) },
      );
    });

    const proxyDisclosure = renderer.root.findAllByType("summary").find((node) =>
      node.findAllByType("strong").some((heading) => heading.children.includes("网络代理")),
    );
    expect(proxyDisclosure).toBeTruthy();
    expect(renderer.root.findAllByProps({ "aria-label": "代理模式" })).toHaveLength(1);
  });

  it("saves a manual proxy and reports provider connectivity from the probe response", async () => {
    getJsonMock.mockReset();
    const values = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    });
    getJsonMock.mockResolvedValue({
      market: "hk",
      code: "00700",
      proxy_mode: "manual",
      proxy_configured: true,
      sources: [
        { source_id: "tencent", ok: true, error: "" },
        { source_id: "tencent_history", ok: false, error: "Tencent history blocked" },
        { source_id: "sina_hk_financial", ok: false, error: "连接超时" },
        { source_id: "eastmoney_hk", ok: true, error: "" },
      ],
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<SettingsSheet open onClose={vi.fn()} settings={[]} />, {
        createNodeMock: () => ({ querySelectorAll: () => [] }),
      });
    });
    const disclosure = renderer.root.findAllByType("summary").find((node) =>
      node.findAllByType("strong").some((heading) => heading.children.includes("网络代理")),
    );
    expect(disclosure).toBeTruthy();

    const mode = renderer.root.findByProps({ "aria-label": "代理模式" });
    await act(async () => mode.props.onChange({ currentTarget: { value: "manual" } }));
    const address = renderer.root.findByProps({ "aria-label": "手动代理地址" });
    await act(async () => address.props.onChange({ currentTarget: { value: "http://127.0.0.1:7890" } }));
    await act(async () => renderer.root.findByProps({ children: "保存代理设置" }).props.onClick());
    expect(values.get("stock-optimizer-network-proxy-mode")).toBe("manual");
    expect(values.get("stock-optimizer-network-proxy-url")).toBe("http://127.0.0.1:7890");

    await act(async () => renderer.root.findByProps({ children: "测试港股数据源" }).props.onClick());
    expect(getJsonMock).toHaveBeenCalledWith("/api/hk/network-probe?code=00700.HK");
    expect(JSON.stringify(renderer.toJSON())).toContain("连接超时");
    expect(JSON.stringify(renderer.toJSON())).toContain("新浪财务");
    expect(JSON.stringify(renderer.toJSON())).toContain("腾讯日 K");
    expect(JSON.stringify(renderer.toJSON())).toContain("Tencent history blocked");
  });

  it("rejects manual proxy credentials before persisting or testing", async () => {
    const values = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    });
    getJsonMock.mockReset();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<SettingsSheet open onClose={vi.fn()} settings={[]} />, {
        createNodeMock: () => ({ querySelectorAll: () => [] }),
      });
    });
    const mode = renderer.root.findByProps({ "aria-label": "代理模式" });
    await act(async () => mode.props.onChange({ currentTarget: { value: "manual" } }));
    const address = renderer.root.findByProps({ "aria-label": "手动代理地址" });
    await act(async () => address.props.onChange({ currentTarget: { value: "http://user:password@127.0.0.1:7890" } }));
    await act(async () => renderer.root.findByProps({ children: "保存代理设置" }).props.onClick());

    expect(values.has("stock-optimizer-network-proxy-mode")).toBe(false);
    expect(values.has("stock-optimizer-network-proxy-url")).toBe(false);
    expect(renderer.root.findByProps({ "aria-label": "手动代理地址" }).props["aria-invalid"]).toBe(true);
    expect(JSON.stringify(renderer.toJSON())).toContain("不能包含账号或密码");
    expect(getJsonMock).not.toHaveBeenCalled();
  });
});
