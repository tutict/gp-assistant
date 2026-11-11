import { describe, expect, it } from "vitest";
import { isMarketCapability, marketCapabilityLabel, marketCapabilityStatusLabel, parseSecurityId } from "./marketDomain";
import fixture from "../../../src-tauri/testdata/market-domain-contract.json";

describe("market domain wire contract", () => {
  it("validates optional quote currency scope without counting it as missing data", () => {
    const capability = { ...fixture.capability, quote_scope: { currency: "HKD", excluded_count: 24 } };
    expect(isMarketCapability(capability)).toBe(true);
    expect(isMarketCapability({ ...capability, quote_scope: { currency: "HKD", excluded_count: -1 } })).toBe(false);
    expect(isMarketCapability({ ...capability, quote_scope: { currency: "Hong Kong", excluded_count: 24 } })).toBe(false);
  });
  it("uses one market capability label vocabulary across panels", () => {
    expect(marketCapabilityLabel("security_directory")).toBe("证券目录");
    expect(marketCapabilityLabel("backtest")).toBe("回测");
    expect(marketCapabilityStatusLabel("unavailable")).toBe("不可用");
    expect(marketCapabilityStatusLabel("unavailable", "未开放")).toBe("未开放");
    expect(marketCapabilityStatusLabel("limited")).toBe("部分可用");
  });

  it("accepts the Rust market-domain fixture", () => {
    expect(fixture.market).toBe("hk");
    expect(fixture.security).toEqual({
      market: "hk",
      code: "00700",
      kind: "ordinary_share",
    });
    expect(fixture.money).toEqual({ amount: 12.5, currency: "HKD" });
    expect(fixture.quantity).toEqual({ amount: 100, unit: "share" });
    expect(isMarketCapability(fixture.capability)).toBe(true);

    const capability = fixture.capability as {
      id: string;
      status: string;
      coverage: { requested_count: number; available_count: number; missing_count: number };
    };
    expect(capability.id).toBe("observe");
    expect(capability.status).toBe("limited");
    expect(capability.coverage).toMatchObject({
      requested_count: 2,
      available_count: 1,
      missing_count: 1,
    });
  });

  it("accepts old capability responses and validates additive source evidence", () => {
    const legacy = { ...fixture.capability } as Record<string, unknown>;
    delete legacy.sources;
    expect(isMarketCapability(legacy)).toBe(true);

    const capability = {
      ...fixture.capability,
      sources: [{
        source_id: "tencent",
        domain: "quote",
        market: "hk",
        fetched_at: "2026-10-27T08:00:00Z",
        observed_at: "2026-10-27",
        disclosed_at: null,
        report_period: null,
        freshness: "fresh",
        verification_status: "unverified",
        quality_flags: ["provider_rights_unverified"],
        coverage: null,
      }],
    };
    expect(isMarketCapability(capability)).toBe(true);
    expect(isMarketCapability({ ...capability, sources: [{ ...capability.sources[0], freshness: "probably-fresh" }] })).toBe(false);
    expect(isMarketCapability({ ...capability, sources: [{ ...capability.sources[0], market: "cn" }] })).toBe(false);
  });

  it("rejects malformed capability values", () => {
    expect(
      isMarketCapability({
        market: "hk",
        id: "observe",
        status: "limited",
        reason: "missing data",
        coverage: {
          requested_count: 1,
          available_count: 1,
          missing_count: -1,
          start_date: null,
          end_date: null,
          missing_fields: [],
        },
      }),
    ).toBe(false);
  });

  it("preserves canonical HK leading zeroes and rejects noncanonical codes", () => {
    expect(parseSecurityId("hk", "00700", "ordinary_share")).toEqual({
      market: "hk",
      code: "00700",
      kind: "ordinary_share",
    });
    expect(parseSecurityId("hk", "700", "ordinary_share")).toBeNull();
    expect(parseSecurityId("hk", "00700.HK", "ordinary_share")).toBeNull();
  });

  it("represents an HKEX equity listing without claiming its ordinary or preference share class", () => {
    expect(parseSecurityId("hk", "00700", "listed_equity")).toEqual({
      market: "hk",
      code: "00700",
      kind: "listed_equity",
    });
    expect(parseSecurityId("cn", "600000", "listed_equity")).toBeNull();
  });
});
