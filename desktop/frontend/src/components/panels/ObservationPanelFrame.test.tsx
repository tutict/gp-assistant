import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it } from "vitest";
import { ObservationPanelFrame, type ObservationPanelFrameProps } from "./ObservationPanelFrame";

const limitedFixtureProps: Omit<ObservationPanelFrameProps, "children"> = {
  market: "hk",
  title: "港股观察",
  security: { market: "hk", code: "00700", kind: "ordinary_share" },
  provenance: [{
    source_id: "tencent",
    fetched_at: "2026-10-20T08:00:00Z",
    observed_at: "2026-10-20T08:00:00Z",
    disclosed_at: null,
    report_period: null,
    verification_status: "unverified",
    quality_flags: [],
    missing_reason: null,
  }],
  capability: {
    market: "hk",
    id: "observe",
    status: "limited",
    reason: "日 K 覆盖不足",
    coverage: { requested_count: 2, available_count: 1, missing_count: 1, start_date: "2026-10-16", end_date: "2026-10-17", missing_fields: ["daily_history"] },
  },
  issues: ["provider timeout"],
  actions: <button type="button">刷新行情</button>,
};

function textContent(value: unknown): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map(textContent).join("");
  if (typeof value === "object" && value !== null && "children" in value) return textContent(value.children);
  return "";
}

describe("ObservationPanelFrame", () => {
  it("shows source timestamps, limited coverage, issues, actions, and the observation body", async () => {
    let tree!: ReactTestRenderer;
    await act(async () => { tree = create(<ObservationPanelFrame {...limitedFixtureProps}><p>quote body</p></ObservationPanelFrame>); });
    const text = textContent(tree.toJSON());
    expect(text).toContain("部分可用");
    expect(text).toContain("日 K 覆盖不足");
    expect(text).toContain("tencent");
    expect(text).toContain("2026-10-20T08:00:00Z");
    expect(text).toContain("provider timeout");
    expect(text).toContain("刷新行情");
    expect(text).toContain("quote body");
  });

  it("keeps cached observation content visible while explaining an unavailable capability", async () => {
    let tree!: ReactTestRenderer;
    await act(async () => { tree = create(<ObservationPanelFrame
      {...limitedFixtureProps}
      capability={{ ...limitedFixtureProps.capability, status: "unavailable", reason: "普通股分类尚未核验" }}
    ><p>cached history</p></ObservationPanelFrame>); });
    const text = textContent(tree.toJSON());
    expect(text).toContain("不可用");
    expect(text).toContain("普通股分类尚未核验");
    expect(text).toContain("cached history");
  });

  it("states when the response does not report a fetch timestamp", async () => {
    let tree!: ReactTestRenderer;
    await act(async () => { tree = create(<ObservationPanelFrame
      {...limitedFixtureProps}
      provenance={[{ ...limitedFixtureProps.provenance[0], fetched_at: null }]}
    ><p>body</p></ObservationPanelFrame>); });
    expect(textContent(tree.toJSON())).toContain("抓取：未提供");
  });
});
