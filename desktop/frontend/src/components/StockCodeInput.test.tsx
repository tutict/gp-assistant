import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";

const { getJsonMock } = vi.hoisted(() => ({ getJsonMock: vi.fn() }));

vi.mock("../lib/tauri", () => ({ getJson: getJsonMock }));

import { StockCodeInput } from "./StockCodeInput";

function StockCodeInputHarness() {
  const [value, setValue] = useState("");
  return (
    <StockCodeInput
      id="stockCode"
      value={value}
      onChange={setValue}
      resolveBareCode
    />
  );
}

function PopulatedStockCodeInputHarness() {
  const [value, setValue] = useState("600519.SH");
  return (
    <StockCodeInput
      id="stockCode"
      value={value}
      onChange={setValue}
      resolveBareCode
    />
  );
}

describe("StockCodeInput", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("resolves a six-digit desktop code without prompting for a market", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal("window", {
      setTimeout: vi.fn(() => 1),
      clearTimeout: vi.fn(),
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<StockCodeInputHarness />);
    });

    await act(async () => {
      renderer.root.findByType("input").props.onChange({ target: { value: "000100" } });
    });

    expect(renderer.root.findByType("input").props.value).toBe("000100.SZ");
    expect(renderer.root.findAll((node) => node.props["aria-label"] === "选择市场")).toHaveLength(0);
  });

  it("allows deleting through a previously resolved stock code", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal("window", {
      setTimeout: vi.fn(() => 1),
      clearTimeout: vi.fn(),
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<PopulatedStockCodeInputHarness />);
    });

    await act(async () => {
      renderer.root.findByType("input").props.onChange({ target: { value: "600519" } });
    });

    expect(renderer.root.findByType("input").props.value).toBe("600519");
  });

  it("commits a complete code when Enter is pressed", async () => {
    const onCommit = vi.fn();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal("window", {
      setTimeout: vi.fn(() => 1),
      clearTimeout: vi.fn(),
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <StockCodeInput
          id="stockCode"
          value="600519.SH"
          onChange={vi.fn()}
          onCommit={onCommit}
          resolveBareCode
        />,
      );
    });

    await act(async () => {
      renderer.root.findByType("input").props.onKeyDown({
        key: "Enter",
        preventDefault: vi.fn(),
      });
    });

    expect(onCommit).toHaveBeenCalledWith("600519.SH");
  });

  it.each(["loading", "market", "option", "nested"])("consumes Escape for the %s lookup popup and restores input focus", async (state) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", { addEventListener: vi.fn(), removeEventListener: vi.fn(), activeElement: null });
    let search!: () => Promise<void>;
    vi.stubGlobal("window", { setTimeout: vi.fn((callback) => { search = callback; return 1; }), clearTimeout: vi.fn() });
    getJsonMock.mockReset().mockResolvedValue(state === "market" ? [] : [{ code: "600519.SH", name: "贵州茅台" }]);
    let renderer!: ReactTestRenderer;
    const focus = vi.fn(() => renderer.root.findByType("input").props.onFocus());
    await act(async () => {
      renderer = create(<StockCodeInput id="popupCode" value={state === "market" ? "600519" : "贵州"} onChange={vi.fn()} />, {
        createNodeMock: element => element.type === "div" && (element.props as { className?: string }).className === "stock-code-input"
          ? { querySelector: () => ({ focus }) } : null,
      });
    });
    await act(async () => renderer.root.findByType("input").props.onFocus());
    if (state !== "loading") await act(async () => { await search(); });
    expect(renderer.root.findAllByProps({ className: state === "market" ? "market-confirm" : "stock-suggest" })).toHaveLength(1);
    const preventDefault = vi.fn(), stopPropagation = vi.fn();
    const wrapper = renderer.root.findByProps({ className: "stock-code-input" });
    await act(async () => wrapper.props.onKeyDown({ key: "Escape", preventDefault, stopPropagation,
      target: { tagName: state === "option" || state === "market" ? "BUTTON" : "INPUT" } }));
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(focus).toHaveBeenCalledOnce();
    expect(renderer.root.findAllByProps({ className: "stock-suggest" })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ className: "market-confirm" })).toHaveLength(0);
    if (state === "loading") {
      await act(async () => { await search(); });
      expect(renderer.root.findAllByProps({ className: "stock-suggest" })).toHaveLength(0);
    }
    await act(async () => renderer.unmount());
  });
});
