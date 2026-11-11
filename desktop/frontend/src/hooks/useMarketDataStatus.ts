import { useCallback, useEffect, useRef, useState } from "react";
import type { MarketId } from "../lib/marketDomain";
import { getJson } from "../lib/tauri";
import type { DataStatus } from "../types";

export function useMarketDataStatus(market: MarketId) {
  const [snapshot, setSnapshot] = useState<{ market: MarketId; status: DataStatus } | null>(null);
  const marketRef = useRef(market);
  const requestVersionRef = useRef(0);
  marketRef.current = market;

  const setMarketStatus = useCallback((status: DataStatus | null) => {
    if (marketRef.current !== market) return;
    ++requestVersionRef.current;
    setSnapshot(status ? { market, status } : null);
  }, [market]);

  const refreshMarketStatus = useCallback(async () => {
    if (marketRef.current !== market) return;
    const version = ++requestVersionRef.current;
    try {
      const status = await getJson<DataStatus>(market === "hk" ? "/api/hk/market-status" : "/api/data-sources/status");
      if (marketRef.current !== market || version !== requestVersionRef.current) return;
      setSnapshot({ market, status });
    } catch {
      // A failed reread does not erase a known snapshot or another market's state.
    }
  }, [market]);

  useEffect(() => {
    void refreshMarketStatus();
    return () => { ++requestVersionRef.current; };
  }, [refreshMarketStatus]);

  return {
    marketStatus: snapshot?.market === market ? snapshot.status : null,
    setMarketStatus,
    refreshMarketStatus,
  };
}
