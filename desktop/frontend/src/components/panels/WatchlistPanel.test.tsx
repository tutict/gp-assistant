import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WatchlistPanel } from "./WatchlistPanel";

describe("WatchlistPanel", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders a clear portfolio backtest action and readable missing-name state", () => {
    const html = renderToStaticMarkup(
      <WatchlistPanel
        items={[{ code: "002432.SZ" }]}
        onChange={vi.fn()}
        onObserve={vi.fn()}
        onNews={vi.fn()}
        onBacktest={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="用 1 只自选股回测"');
    expect(html).toContain("组合回测");
    expect(html).toContain("名称待同步");
    expect(html).toContain('aria-label="查看 002432.SZ 的消息"');
    expect(html).toContain('aria-label="移除 002432.SZ"');
  });

  it("starts collapsed on Android and can expand the stock list", async () => {
    vi.stubGlobal("document", {
      documentElement: {
        classList: { contains: (name: string) => name === "android-phone" },
      },
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <WatchlistPanel
          items={[{ code: "002432.SZ", name: "九安医疗" }]}
          onChange={vi.fn()}
        />,
      );
    });

    const body = () => renderer.root.find((node) => node.props.className === "watchlist-body");
    const toggle = () => renderer.root.find((node) => node.props["aria-controls"] === body().props.id);
    expect(body().props.hidden).toBe(true);
    expect(toggle().props["aria-expanded"]).toBe(false);

    await act(async () => {
      toggle().props.onClick();
    });

    expect(body().props.hidden).toBe(false);
    expect(toggle().props["aria-expanded"]).toBe(true);
  });
});
