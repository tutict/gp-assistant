import { useState } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const persistence = vi.hoisted(() => ({
  state: { status: "saving", pendingCount: 1, error: null as string | null, storage: "sqlite" },
  retry: vi.fn(),
  listeners: new Set<() => void>(),
}));
vi.mock("../../lib/watchlistStore", () => ({
  subscribeWatchlistPersistence: (listener: () => void) => {
    persistence.listeners.add(listener);
    return () => persistence.listeners.delete(listener);
  },
  getWatchlistPersistenceSnapshot: () => persistence.state,
  retryWatchlistPersistence: persistence.retry,
}));
beforeEach(() => {
  persistence.state = { status: "saving", pendingCount: 1, error: null, storage: "sqlite" };
  persistence.retry.mockClear();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
});

import type { WatchlistItem } from "../../types";
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

  it("starts collapsed in a narrow viewport and can expand the stock list", async () => {
    vi.stubGlobal("window", {
      matchMedia: vi.fn().mockReturnValue({ matches: true }),
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
    await act(async () => renderer.unmount());
  });
});


it("shows saving until acknowledged and exposes an explicit failure retry", async () => {
  let renderer!: ReactTestRenderer;
  await act(async () => { renderer = create(<WatchlistPanel items={[]} onChange={vi.fn()} />); });
  expect(JSON.stringify(renderer.toJSON())).toContain("保存中");
  expect(JSON.stringify(renderer.toJSON())).not.toContain("已保存");
  await act(async () => {
    persistence.state = { status: "error", pendingCount: 1, error: "disk full", storage: "sqlite" };
    persistence.listeners.forEach((listener) => listener());
  });
  expect(JSON.stringify(renderer.toJSON())).toContain("未保存");
  await act(async () => { renderer.root.findByProps({ "aria-label": "重试保存自选股" }).props.onClick(); });
  expect(persistence.retry).toHaveBeenCalledOnce();
  await act(async () => {
    persistence.state = { status: "saved", pendingCount: 0, error: null, storage: "sqlite" };
    persistence.listeners.forEach((listener) => listener());
  });
  expect(JSON.stringify(renderer.toJSON())).toContain("已保存");
  await act(async () => renderer.unmount());
});

it("undoes each removed item independently and retains its metadata", async () => {
  const a = { code: "000001.SZ", name: "A", added_at: "2026-01-01", source: "screen", screenCriteriaSummary: "quality" };
  const b = { code: "000002.SZ", name: "B", added_at: "2026-01-02" };
  const changed = vi.fn();
  function Harness() {
    const [items, setItems] = useState<WatchlistItem[]>([a, b]);
    return <WatchlistPanel items={items} onChange={(next) => { changed(next); setItems(next); }} />;
  }
  let renderer!: ReactTestRenderer;
  await act(async () => { renderer = create(<Harness />); });
  await act(async () => { renderer.root.findByProps({ "aria-label": "移除 A" }).props.onClick(); });
  await act(async () => { renderer.root.findByProps({ "aria-label": "移除 B" }).props.onClick(); });
  expect(changed).toHaveBeenLastCalledWith([]);
  await act(async () => { renderer.root.findByProps({ "aria-label": "撤销移除 A" }).props.onClick(); });
  expect(changed).toHaveBeenLastCalledWith([a]);
  await act(async () => { renderer.root.findByProps({ "aria-label": "撤销移除 B" }).props.onClick(); });
  expect(changed.mock.lastCall?.[0]).toEqual(expect.arrayContaining([a, b]));
  await act(async () => renderer.unmount());
});

it("still confirms before clearing the watchlist", async () => {
  const confirm = vi.fn().mockReturnValueOnce(false).mockReturnValueOnce(true);
  vi.stubGlobal("confirm", confirm);
  const changed = vi.fn();
  let renderer!: ReactTestRenderer;
  await act(async () => { renderer = create(<WatchlistPanel items={[{ code: "000001.SZ" }]} onChange={changed} />); });
  await act(async () => { renderer.root.findByProps({ "aria-label": "清空自选股" }).props.onClick(); });
  expect(changed).not.toHaveBeenCalled();
  await act(async () => { renderer.root.findByProps({ "aria-label": "清空自选股" }).props.onClick(); });
  expect(changed).toHaveBeenLastCalledWith([]);
  await act(async () => renderer.unmount());
});
