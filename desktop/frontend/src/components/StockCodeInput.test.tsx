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
});
