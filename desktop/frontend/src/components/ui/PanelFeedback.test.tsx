import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { PanelFeedback } from "./PanelFeedback";

describe("PanelFeedback", () => {
  it("renders errors as assertive alerts", () => {
    const html = renderToStaticMarkup(
      <PanelFeedback kind="error" title="查询失败" description="网络不可用" />,
    );

    expect(html).toContain('role="alert"');
    expect(html).toContain('aria-live="assertive"');
    expect(html).toContain("查询失败");
    expect(html).toContain("网络不可用");
  });

  it("renders loading feedback as a polite status", () => {
    const html = renderToStaticMarkup(
      <PanelFeedback kind="loading" description="正在加载" />,
    );

    expect(html).toContain('role="status"');
    expect(html).toContain('aria-live="polite"');
    expect(html).toContain("panel-feedback-loading");
    expect(html).toContain("panel-feedback-skeleton");
    expect(html).toContain("skeleton-line");
    expect(html).toContain("正在加载");
  });
});
