import { useEffect } from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DataStatus } from "../types";
import type { MarketId } from "../lib/marketDomain";

const { getJsonMock } = vi.hoisted(() => ({ getJsonMock: vi.fn() }));
vi.mock("../lib/tauri", () => ({ getJson: getJsonMock }));
import { useMarketDataStatus } from "./useMarketDataStatus";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => { resolve = complete; });
  return { promise, resolve };
}

function StatusProbe({ market, capture }: { market: MarketId; capture?: (commit: (status: DataStatus | null) => void) => void }) {
  const { marketStatus, setMarketStatus, refreshMarketStatus } = useMarketDataStatus(market);
  useEffect(() => { capture?.(setMarketStatus); }, [capture, setMarketStatus]);
  return <>
    <output>{marketStatus?.universe_count ?? "--"} · {marketStatus?.quote_trade_date ?? "--"}</output>
    <button onClick={() => void refreshMarketStatus()}>刷新状态</button>
  </>;
}

describe("market data status", () => {
  let renderer: ReactTestRenderer | undefined;
  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    getJsonMock.mockReset();
  });
  afterEach(async () => {
    if (renderer) await act(async () => renderer?.unmount());
    renderer = undefined;
    vi.unstubAllGlobals();
  });
  const renderedStatus = () => renderer!.root.findByType("output").children.join("");
  const hkStatus: DataStatus = { market: "hk", universe_count: 2756, quote_trade_date: "20261106", stale: false };

  it("loads HK cache status for the HK workspace", async () => {
    getJsonMock.mockImplementation(async (url: string) => {
      if (url !== "/api/hk/market-status") throw new Error("wrong market endpoint");
      return hkStatus;
    });
    await act(async () => { renderer = create(<StatusProbe market="hk" />); });
    expect(renderedStatus()).toBe("2756 · 20261106");
  });

  it("keeps a late A-share response out of a switched HK workspace", async () => {
    const pendingCn = deferred<DataStatus>();
    getJsonMock.mockImplementation((url: string) => url === "/api/data-sources/status" ? pendingCn.promise : Promise.resolve(hkStatus));
    await act(async () => { renderer = create(<StatusProbe market="cn" />); });
    await act(async () => { renderer!.update(<StatusProbe market="hk" />); });
    expect(renderedStatus()).toBe("2756 · 20261106");
    await act(async () => { pendingCn.resolve({ universe_count: 5930, quote_trade_date: "20261105" }); });
    expect(renderedStatus()).toBe("2756 · 20261106");
  });

  it("keeps the newer manual refresh when the startup read finishes late", async () => {
    const pendingStartup = deferred<DataStatus>();
    getJsonMock.mockReturnValueOnce(pendingStartup.promise).mockResolvedValueOnce(hkStatus);
    await act(async () => { renderer = create(<StatusProbe market="hk" />); });
    await act(async () => { renderer!.root.findByType("button").props.onClick(); });
    expect(renderedStatus()).toBe("2756 · 20261106");
    await act(async () => { pendingStartup.resolve({ ...hkStatus, universe_count: 2700, quote_trade_date: "20261105" }); });
    expect(renderedStatus()).toBe("2756 · 20261106");
  });

  it("preserves known counts and quote dates when a status reread fails", async () => {
    getJsonMock.mockResolvedValueOnce(hkStatus).mockRejectedValueOnce(new Error("temporary cache read failure"));
    await act(async () => { renderer = create(<StatusProbe market="hk" />); });
    await act(async () => { renderer!.root.findByType("button").props.onClick(); });
    expect(renderedStatus()).toBe("2756 · 20261106");
  });

  it("ignores a status callback retained by the previous market", async () => {
    let commitCn!: (status: DataStatus | null) => void;
    getJsonMock.mockImplementation(async (url: string) => url === "/api/hk/market-status" ? hkStatus : { universe_count: 5930 });
    await act(async () => { renderer = create(<StatusProbe market="cn" capture={(commit) => { commitCn = commit; }} />); });
    await act(async () => { renderer!.update(<StatusProbe market="hk" />); });
    await act(async () => { commitCn({ universe_count: 6000, quote_trade_date: "20261105" }); });
    expect(renderedStatus()).toBe("2756 · 20261106");
  });
});
