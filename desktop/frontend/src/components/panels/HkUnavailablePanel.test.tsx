import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import { HkUnavailablePanel } from "./HkUnavailablePanel";

describe("HkUnavailablePanel", () => {
  it("renders the registry reason, evidence gap, and market-switch action", async () => {
    const onSwitchMarket = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<HkUnavailablePanel
      feature="港股回测"
      capability={{
        market: "hk",
        id: "backtest",
        status: "unavailable",
        reason: "point-in-time daily history and corporate actions are not registered",
        coverage: { requested_count: 2, available_count: 0, missing_count: 2, start_date: null, end_date: null, missing_fields: ["daily_history", "corporate_actions"] },
      }}
      onSwitchMarket={onSwitchMarket}
    />); });

    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("港股回测");
    expect(text).toContain("未开放");
    expect(text).toContain("point-in-time daily history and corporate actions are not registered");
    expect(text).toContain("corporate_actions");
    await act(async () => { renderer.root.findByProps({ children: "切换到 A 股" }).props.onClick(); });
    expect(onSwitchMarket).toHaveBeenCalledTimes(1);
    expect(text).not.toContain("MVP");
  });
});
