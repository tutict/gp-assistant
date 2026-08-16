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

import { postJson } from "../lib/tauri";

const postJsonMock = vi.mocked(postJson);

describe("FilterBar", () => {
  let nextTimerId: number;
  let timers: Map<number, () => void>;

  beforeEach(() => {
    nextTimerId = 1;
    timers = new Map();
    postJsonMock.mockReset();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("window", {
      setTimeout: vi.fn((callback: () => void) => {
        const timerId = nextTimerId;
        nextTimerId += 1;
        timers.set(timerId, callback);
        return timerId;
      }),
      clearTimeout: vi.fn((timerId: number) => timers.delete(timerId)),
    });
    vi.stubGlobal("document", undefined);
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

  it("renders the desktop data status as the compact toolbar summary", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FilterBar mobileRuntime={false} status={{ universe_count: 5231, quote_trade_date: "20260804", current_trade_date: "20260804", stale: false }} />);
      await Promise.resolve();
    });

    const status = renderer.root.findByProps({ "aria-label": "股票池状态" });
    expect(status.props.className).toContain("fresh");
    expect(status.findByType("strong").children.join("")).toContain("已同步 5,231 只");
  });

  it("pauses automatic log collapse while the desktop log shell is hovered", async () => {
    postJsonMock.mockResolvedValue({
      status: { universe_count: 5432, quote_trade_date: "20260814", current_trade_date: "20260814", stale: false },
      refreshed: true,
      notes: ["刷新完成。"],
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FilterBar mobileRuntime={false} status={{ universe_count: 5231 }} />);
    });

    await act(async () => {
      renderer.root.findByProps({ className: "screen-toolbar-refresh-btn" }).props.onClick();
      await Promise.resolve();
    });

    let shell = renderer.root.findByProps({ id: "refresh-log-panel" });
    expect(shell.props.className).toContain("open");

    await act(async () => {
      shell.props.onMouseEnter();
    });

    await act(async () => {
      for (const [timerId, callback] of [...timers]) {
        timers.delete(timerId);
        callback();
      }
    });
    shell = renderer.root.findByProps({ id: "refresh-log-panel" });
    expect(shell.props.className).toContain("open");

    await act(async () => {
      shell.props.onMouseLeave();
    });
    const collapseTimer = Math.max(...timers.keys());
    await act(async () => {
      timers.get(collapseTimer)?.();
      timers.delete(collapseTimer);
    });

    expect(renderer.root.findByProps({ id: "refresh-log-panel" }).props.className).toContain("collapsed");
  });

  it("closes the desktop maintenance menu on Escape and outside mousedown", async () => {
    const listeners = new Map<string, EventListener>();
    const insideNode = { nodeName: "SUMMARY" };
    const maintenanceNode = { contains: vi.fn((target: unknown) => target === insideNode) };
    vi.stubGlobal("document", {
      addEventListener: vi.fn((type: string, listener: EventListener) => listeners.set(type, listener)),
      removeEventListener: vi.fn((type: string) => listeners.delete(type)),
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FilterBar mobileRuntime={false} status={{ universe_count: 5231 }} />, {
        createNodeMock: (element) => element.type === "details" ? maintenanceNode : {},
      });
    });

    const openSummary = () => renderer.root.findByProps({ "aria-label": "股票池维护设置" });
    await act(async () => {
      openSummary().props.onClick({ preventDefault: vi.fn() });
    });
    expect(openSummary().props["aria-expanded"]).toBe(true);

    await act(async () => {
      listeners.get("keydown")?.({ key: "Escape", preventDefault: vi.fn() } as unknown as Event);
    });
    expect(openSummary().props["aria-expanded"]).toBe(false);

    await act(async () => {
      openSummary().props.onClick({ preventDefault: vi.fn() });
    });
    expect(openSummary().props["aria-expanded"]).toBe(true);

    await act(async () => {
      listeners.get("mousedown")?.({ target: { nodeName: "BODY" } } as unknown as Event);
    });
    expect(openSummary().props["aria-expanded"]).toBe(false);
  });
});
