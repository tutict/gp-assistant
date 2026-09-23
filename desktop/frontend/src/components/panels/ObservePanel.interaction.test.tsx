import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { getJsonMock } = vi.hoisted(() => ({ getJsonMock: vi.fn() }));

vi.mock("../../lib/tauri", () => ({ getJson: getJsonMock }));

import { ObservePanel } from "./ObservePanel";

function textContent(renderer: ReactTestRenderer): string {
  return renderer.root.findAll((node) => typeof node.children[0] === "string")
    .flatMap((node) => node.children)
    .filter((child): child is string => typeof child === "string")
    .join(" ");
}

function observeCalls() {
  return getJsonMock.mock.calls.filter((call) => String(call[0]).includes("/api/observe/"));
}

describe("ObservePanel requests", () => {
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("window", {
      location: { href: "http://localhost/" },
      setTimeout: () => 1,
      clearTimeout: () => undefined,
    });
    vi.stubGlobal("document", {
      activeElement: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    getJsonMock.mockImplementation((url: string) => {
      const target = String(url);
      if (target.includes("/api/observe/600519.SH")) {
        return Promise.resolve({ stock: { code: "600519.SH", name: "贵州茅台", industry: "白酒" }, notes: [] });
      }
      if (target.includes("/api/observe/000001.SZ")) {
        return Promise.resolve({ stock: { code: "000001.SZ", name: "平安银行" }, notes: [] });
      }
      return Promise.resolve({ stock: { code: "000000.SZ", name: "未知" }, notes: [] });
    });
  });

  afterEach(() => {
    getJsonMock.mockReset();
    vi.unstubAllGlobals();
  });

  it("runs once for an incoming stock and does not rerun when the code is edited", async () => {
    const onOpenNews = vi.fn();
    const onAskAgent = vi.fn();
    let renderer!: ReactTestRenderer;
    const props = {
      watchlist: [],
      onWatchlistChange: () => undefined,
      initialCode: "600519.SH",
      initialCodeRequestId: 1,
      onOpenNews,
      onAskAgent,
    };
    await act(async () => {
      renderer = create(<ObservePanel {...props} />);
    });
    await act(async () => { await Promise.resolve(); });

    expect(observeCalls()).toHaveLength(1);
    expect(String(observeCalls()[0][0])).toContain("/api/observe/600519.SH");
    expect(textContent(renderer)).toContain("贵州茅台");

    await act(async () => {
      renderer.update(<ObservePanel {...props} />);
    });
    expect(observeCalls()).toHaveLength(1);

    const input = renderer.root.find((node) => node.type === "input" && node.props.id === "observeCode");
    await act(async () => {
      input.props.onChange({ target: { value: "000001.SZ" } });
    });
    expect(input.props.value).toBe("000001.SZ");
    expect(observeCalls()).toHaveLength(1);

    const run = renderer.root.find((node) => node.type === "button" && node.children.includes("开始观察"));
    await act(async () => { await run.props.onClick(); });
    expect(observeCalls()).toHaveLength(2);
    expect(String(observeCalls()[1][0])).toContain("/api/observe/000001.SZ");

    await act(async () => {
      renderer.update(<ObservePanel {...props} initialCodeRequestId={2} />);
    });
    await act(async () => { await Promise.resolve(); });
    expect(observeCalls()).toHaveLength(3);
    expect(textContent(renderer)).toContain("贵州茅台");

    const news = renderer.root.find((node) => node.type === "button" && node.children.includes("消息"));
    const handoff = renderer.root.find((node) => node.type === "button" && node.children.includes("交给 Agent"));
    await act(async () => { news.props.onClick(); });
    await act(async () => { handoff.props.onClick(); });
    expect(onOpenNews).toHaveBeenCalledWith("600519.SH");
    expect(onAskAgent).toHaveBeenCalledTimes(1);
    expect(onAskAgent.mock.calls[0][0]).toContain("600519.SH");
    expect(onAskAgent.mock.calls[0][0]).toContain("贵州茅台");
    await act(async () => renderer.unmount());
  });


  it("drops an in-flight observation when the code is edited before it returns", async () => {
    let resolveRequest: (value: unknown) => void = () => undefined;
    getJsonMock.mockImplementation((url: string) => {
      if (String(url).includes("/api/observe/600519.SH")) {
        return new Promise((resolve) => { resolveRequest = resolve; });
      }
      return Promise.resolve({ stock: { code: "000001.SZ", name: "平安银行" }, notes: [] });
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="600519.SH"
          initialCodeRequestId={1}
        />,
      );
    });
    const input = renderer.root.find((node) => node.type === "input" && node.props.id === "observeCode");
    await act(async () => {
      input.props.onChange({ target: { value: "000001.SZ" } });
    });
    await act(async () => {
      resolveRequest({ stock: { code: "600519.SH", name: "贵州茅台" }, notes: [] });
      await Promise.resolve();
    });
    expect(textContent(renderer)).not.toContain("贵州茅台");
    expect(renderer.root.findByProps({ id: "observeCode" }).props.value).toBe("000001.SZ");
    await act(async () => renderer.unmount());
  });

  it("reports an invalid incoming code without requesting observation", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <ObservePanel
          watchlist={[]}
          onWatchlistChange={() => undefined}
          initialCode="not-a-code"
          initialCodeRequestId={1}
        />,
      );
    });
    expect(textContent(renderer)).toContain("请输入有效股票代码。");
    expect(observeCalls()).toHaveLength(0);
    await act(async () => renderer.unmount());
  });
});
