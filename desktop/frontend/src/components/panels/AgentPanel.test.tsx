import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

let AgentPanel: typeof import("./AgentPanel").AgentPanel;

beforeAll(async () => {
  vi.stubGlobal("window", { location: { href: "http://localhost/" } });
  ({ AgentPanel } = await import("./AgentPanel"));
});

afterAll(() => {
  vi.unstubAllGlobals();
});

const baseProps = {
  onLlmSettingsChange: vi.fn(),
  watchlist: [],
  onWatchlistChange: vi.fn(),
};

describe("AgentPanel empty state", () => {
  it("renders a compact mode selector and model setup hint", () => {
    const html = renderToStaticMarkup(<AgentPanel {...baseProps} llmSettings={null} />);

    expect(html).toContain("<h2>开始对话</h2>");
    expect(html).toContain('role="group"');
    expect(html).toContain('aria-pressed="true"');
    expect(html).toContain("请先配置模型");
    expect(html).toContain("游资早期框架：环境、主线、情绪周期与失效条件");
    expect(html).toContain("价值复利框架：企业质量、资本配置与估值");
    expect(html).not.toContain("快速模式能力");
  });

  it("hides the setup hint when a model is configured", () => {
    const html = renderToStaticMarkup(
      <AgentPanel
        {...baseProps}
        llmSettings={{
          active_provider_id: "test",
          providers: [{ id: "test", name: "Test", provider: "openai-compatible", model: "test-model" }],
        }}
      />,
    );

    expect(html).not.toContain("请先配置模型");
  });
});
