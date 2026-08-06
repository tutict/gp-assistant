import { act, create } from "react-test-renderer";
import type { ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DataStatus } from "../types";
import { FilterBar } from "./FilterBar";

vi.mock("../lib/tauri", () => ({
  getTauriInvoke: vi.fn(() => null),
  isMarketStatusStale: vi.fn(() => false),
  postJson: vi.fn(),
  refreshTauriMarketData: vi.fn(),
}));

describe("FilterBar", () => {
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("window", {
      setTimeout: vi.fn(() => 1),
      clearTimeout: vi.fn(),
    });
  });

  it("renders the controlled market status on mobile", async () => {
    const status: DataStatus = {
      universe_count: 5231,
      quote_trade_date: "20260804",
      current_trade_date: "20260804",
      cache_bytes: 1024,
    };

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FilterBar mobileRuntime status={status} />);
    });

    expect(renderer.root.findByProps({ "aria-label": "股票池状态" })).toBeTruthy();
  });

  it("keeps desktop data status in the header instead of duplicating it in the page toolbar", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FilterBar mobileRuntime={false} status={{ universe_count: 5231 }} />);
      await Promise.resolve();
    });

    expect(renderer.root.findAllByProps({ "aria-label": "股票池状态" })).toHaveLength(0);
  });
});
