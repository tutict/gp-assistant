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

  it("commits a stock from the global search", async () => {
    const onSearchCommit = vi.fn();
    let renderer!: ReactTestRenderer;

    await act(async () => {
      renderer = create(
        <Header
          theme="dark"
          density="comfortable"
          searchCode="600519.SH"
          searchInputRef={createRef<HTMLInputElement>()}
          watchlistCount={3}
          dataStatus={null}
          shortcutHelpOpen={false}
          onSearchCodeChange={vi.fn()}
          onSearchCommit={onSearchCommit}
          onToggleDensity={vi.fn()}
          onToggleHelp={vi.fn()}
          onToggleTheme={vi.fn()}
          onToggleMobileNav={vi.fn()}
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
          theme="light"
          density="compact"
          searchCode=""
          searchInputRef={createRef<HTMLInputElement>()}
          watchlistCount={5}
          dataStatus={{
            universe_count: 5231,
            cache_bytes: 64 * 1024 * 1024,
            quote_trade_date: "20260804",
            current_trade_date: "20260804",
            stale: false,
          }}
          shortcutHelpOpen
          onSearchCodeChange={vi.fn()}
          onSearchCommit={vi.fn()}
          onToggleDensity={vi.fn()}
          onToggleHelp={vi.fn()}
          onToggleTheme={vi.fn()}
          onToggleMobileNav={vi.fn()}
        />,
      );
    });

    expect(renderer.root.findByProps({ "aria-label": "全局数据状态" })).toBeTruthy();
    expect(renderer.root.findAllByType("strong").map((node) => node.children.join(""))).toEqual(
      expect.arrayContaining(["5", "5,231", "2026/08/04", "最新"]),
    );
    expect(renderer.root.findByProps({ role: "dialog", "aria-label": "快捷键帮助" })).toBeTruthy();
    expect(renderer.root.findByProps({ "aria-label": "切换到舒适密度" })).toBeTruthy();
  });
});
