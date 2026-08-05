import { act, create } from "react-test-renderer";
import type { ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DataStatus } from "../types";
import { getJson } from "../lib/tauri";
import { FilterBar } from "./FilterBar";

vi.mock("../lib/tauri", () => ({
  getJson: vi.fn(),
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

  it("publishes loaded market status to the application shell", async () => {
    const status: DataStatus = {
      universe_count: 5231,
      quote_trade_date: "20260804",
      current_trade_date: "20260804",
      cache_bytes: 1024,
    };
    vi.mocked(getJson).mockResolvedValue(status);
    const onStatusChange = vi.fn();

    await act(async () => {
      create(<FilterBar mobileRuntime={false} onStatusChange={onStatusChange} />);
      await Promise.resolve();
    });

    expect(onStatusChange).toHaveBeenCalledWith(status);
  });

  it("keeps desktop data status in the header instead of duplicating it in the page toolbar", async () => {
    vi.mocked(getJson).mockResolvedValue({ universe_count: 5231 });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FilterBar mobileRuntime={false} />);
      await Promise.resolve();
    });

    expect(renderer.root.findAllByProps({ "aria-label": "股票池状态" })).toHaveLength(0);
  });
});
