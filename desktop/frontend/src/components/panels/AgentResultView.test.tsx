import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import type { AgentResult } from "../../types";

let AgentResultView: typeof import("./AgentResultView").AgentResultView;

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ AgentResultView } = await import("./AgentResultView"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

describe("AgentResultView", () => {
  it("renders structured agent details with the legacy result fallback", () => {
    const result: AgentResult = {
      action: "data_status",
      tool_calls: [{
        id: "status-check",
        label: "Check local market data",
        status: "ok",
        output_summary: "Three datasets are current",
      }],
      evidence_summary: [{
        level: "primary",
        title: "Daily bars are current",
        summary: "Latest trade date is available",
        source: "Local data status",
      }],
      answer_sections: [{
        title: "Data readiness",
        bullets: ["All required datasets are ready."],
      }],
      warnings: ["Intraday quotes are delayed."],
      data: { status: "ready" },
    };

    const html = renderToStaticMarkup(
      <AgentResultView result={result} watchlist={[]} onToggleWatchlist={vi.fn()} />,
    );

    expect(html).toContain("Check local market data");
    expect(html).toContain("Three datasets are current");
    expect(html).toContain("Daily bars are current");
    expect(html).toContain("Latest trade date is available");
    expect(html).toContain("Data readiness");
    expect(html).toContain("All required datasets are ready.");
    expect(html).toContain("Intraday quotes are delayed.");
    expect(html).toContain("原始 JSON");
  });
});
