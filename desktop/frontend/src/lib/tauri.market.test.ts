import { describe, expect, it, vi } from "vitest";
import { MARKET_CAPABILITY_IDS } from "./marketCapabilities";
import { TAURI_GET_ROUTES } from "./tauri";

describe("market capability Tauri route", () => {
  it("routes market capability reads to the native registry", async () => {
    const capabilities = MARKET_CAPABILITY_IDS.map((id) => ({
      market: "hk",
      id,
      status: "unavailable",
      reason: "adapter missing",
      coverage: null,
    }));
    const invoke = vi.fn().mockResolvedValue(capabilities);
    const result = await TAURI_GET_ROUTES["/api/market/capabilities"]!({
      invoke,
      parsed: new URL("https://app.test/api/market/capabilities?market=hk"),
      path: "/api/market/capabilities",
    });

    expect(invoke).toHaveBeenCalledWith("api_market_capabilities", { market: "hk" });
    expect(result).toEqual(capabilities);
  });

  it("rejects unsupported markets before invoking native code", async () => {
    const invoke = vi.fn();

    await expect(
      TAURI_GET_ROUTES["/api/market/capabilities"]!({
        invoke,
        parsed: new URL("https://app.test/api/market/capabilities?market=global"),
        path: "/api/market/capabilities",
      }),
    ).rejects.toThrow("unsupported market");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects a capability snapshot returned for a different market", async () => {
    const response = MARKET_CAPABILITY_IDS.map((id) => ({
      market: "cn",
      id,
      status: "available",
      reason: null,
      coverage: null,
    }));
    const invoke = vi.fn().mockResolvedValue(response);

    await expect(
      TAURI_GET_ROUTES["/api/market/capabilities"]!({
        invoke,
        parsed: new URL("https://app.test/api/market/capabilities?market=hk"),
        path: "/api/market/capabilities",
      }),
    ).rejects.toThrow("invalid response");
  });
});
