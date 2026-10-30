import { describe, expect, it } from "vitest";
const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);
const css = readFileSync(new URL("../styles/research.css", import.meta.url), "utf8");
const panel = readFileSync(new URL("../components/panels/NewsRagPanel.tsx", import.meta.url), "utf8");
function lastRule(selector: string) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const rules = [...css.matchAll(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, "g"))];
  expect(rules.length, selector).toBeGreaterThan(0);
  return rules.map((rule) => rule[1]).join("\n");
}
describe("desktop research reading contract", () => {
  it("keeps a 200px stock rail and the reading column when evidence opens", () => {
    expect(lastRule(".research-workspace .research-columns")).toContain("grid-template-columns: 200px minmax(0, 1fr)");
    expect(lastRule(".research-workspace .research-columns:has(.research-evidence.has-selection)")).toContain("grid-template-columns: 200px minmax(0, 1fr)");
  });
  it("limits and centers the reading region at 960px", () => {
    expect(lastRule(".research-workspace .research-stream-body")).toContain("max-width: 960px");
    expect(lastRule(".research-workspace .research-stream-body")).toContain("margin-inline: auto");
  });
  it("uses a 420px right detail sheet with full phone height", () => {
    expect(css).toContain("width: min(420px, 100%)");
    expect(lastRule(".research-surface-sheet")).toContain("height: 100dvh");
    expect(lastRule(".research-surface-sheet")).toContain("var(--safe-top)");
    expect(lastRule(".research-surface-sheet")).toContain("var(--safe-bottom)");
  });
  it("keeps evidence inside the research sheet", () => {
    expect(lastRule(".research-surface-sheet .research-evidence.has-selection")).toContain("position: static");
    expect(panel).toContain('label={citation ? "引用证据检查器" : "研究问答"}');
    expect(panel).toContain('aria-label="关闭证据检查器"');
  });
  it("keeps reference history buttons at the dense touch target", () => {
    expect(lastRule(".research-citation-history button")).toContain("width: var(--touch-dense)");
    expect(lastRule(".research-citation-history button")).toContain("height: var(--touch-dense)");
  });
  it("keeps composer and answers in the question surface", () => {
    const surface = panel.indexOf('className="research-question-surface"');
    const composer = panel.indexOf('<form className="research-composer"');
    expect(composer).toBeGreaterThan(surface);
    expect(lastRule(".research-question-surface .research-composer")).toContain("position: sticky");
    expect(lastRule(".research-question-surface[hidden]")).toContain("display: none");
  });
  it("shows at most two summary lines in the message preview", () => {
    expect(lastRule(".research-event-summary")).toContain("-webkit-line-clamp: 2");
    expect(lastRule(".research-event-summary")).toContain("overflow: hidden");
  });
  it("groups dates without a decorative sentiment rail", () => {
    expect(lastRule(".research-workspace .research-event-list::before")).toContain("content: none");
    expect(panel).toContain('label: date || "日期未提供"');
    expect(panel).not.toContain('label: "利好线索"');
  });
  it("keeps count details folded and hides a completely empty brief", () => {
    expect(lastRule(".research-stat")).toContain("border: 0");
    expect(lastRule(".research-stat")).toContain("background: transparent");
    expect(panel).toContain('visibleMessages.length > 0 && <details className="research-daily-brief"');
    expect(panel).not.toContain("props.documents");
  });
  it("keeps full evidence excerpts readable without a tiny nested scroll region", () => {
    expect(lastRule(".research-surface-sheet .research-evidence-card blockquote")).toContain("max-height: none");
    expect(lastRule(".research-surface-sheet .research-evidence-card blockquote")).toContain("font-size: var(--research-body)");
    expect(lastRule(".research-scope-tag")).toContain("font-size: var(--fs-caption)");
  });
});
