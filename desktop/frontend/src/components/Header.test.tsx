import { createRef } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/tauri", () => ({ getJson: vi.fn() }));

import { Header } from "./Header";

describe("Header", () => {
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("document", {
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal("window", {
      setTimeout: vi.fn(() => 1),
      clearTimeout: vi.fn(),
    });
  });

  const commonProps = {
    searchInputRef: createRef<HTMLInputElement>(),
    settingsOpen: false,
    settings: [],
    onSearchCodeChange: vi.fn(),
    onToggleHelp: vi.fn(),
    onToggleSettings: vi.fn(),
    onToggleTheme: vi.fn(),
    onToggleMobileNav: vi.fn(),
  };

  it("commits a stock from the global search", async () => {
    const onSearchCommit = vi.fn();
    let renderer!: ReactTestRenderer;

    await act(async () => {
      renderer = create(
        <Header
          {...commonProps}
          theme="dark"
          searchCode="600519.SH"
          watchlistCount={3}
          dataStatus={null}
          shortcutHelpOpen={false}
          onSearchCommit={onSearchCommit}
        />,
      );
    });

    const search = renderer.root.find((node) => node.props["aria-label"] === "搜索股票");
    await act(async () => {
      search.props.onKeyDown({ key: "Enter", preventDefault: vi.fn() });
    });

    expect(onSearchCommit).toHaveBeenCalledWith("600519.SH");
  });

  it("shows desktop data status and the shortcut help dialog", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <Header
          {...commonProps}
          theme="light"
          searchCode=""
          watchlistCount={5}
          dataStatus={{
            universe_count: 5231,
            cache_bytes: 64 * 1024 * 1024,
            quote_trade_date: "20260804",
            current_trade_date: "20260804",
            stale: false,
          }}
          shortcutHelpOpen
          onSearchCommit={vi.fn()}
        />,
      );
    });

    expect(renderer.root.findByProps({ "aria-label": "全局数据状态" })).toBeTruthy();
    expect(renderer.root.findAllByType("strong").map((node) => node.children.join(""))).toEqual(
      expect.arrayContaining(["5", "5,231", "2026/08/04", "最新"]),
    );
    expect(renderer.root.findByProps({ role: "dialog", "aria-label": "快捷键帮助" })).toBeTruthy();
    expect(renderer.root.findByProps({ "aria-label": "设置" })).toBeTruthy();
  });

  it("does not claim fresh data without freshness evidence", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <Header
          {...commonProps}
          theme="dark"
          searchCode=""
          watchlistCount={0}
          dataStatus={{ universe_count: 5231 }}
          shortcutHelpOpen={false}
          onSearchCommit={vi.fn()}
        />,
      );
    });

    const values = renderer.root.findAllByType("strong").map((node) => node.children.join(""));
    expect(values).toContain("\u5f85\u68c0\u67e5");
    expect(values).not.toContain("\u6700\u65b0");
  });

  it("replaces the density control with a settings trigger", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <Header
          {...commonProps}
          theme="dark"
          searchCode=""
          watchlistCount={0}
          dataStatus={null}
          shortcutHelpOpen={false}
          onSearchCommit={vi.fn()}
        />,
      );
    });

    expect(renderer.root.findByProps({ className: "icon-button settings-trigger" })).toBeTruthy();
    expect(renderer.root.findAllByProps({ className: "density-control" })).toHaveLength(0);
  });
});
