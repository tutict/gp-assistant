import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);
const researchCss = readFileSync(new URL("../styles/research.css", import.meta.url), "utf8");

function cssBlock(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{`).exec(source);
  const start = match?.index ?? -1;
  if (start < 0) throw new Error(`Missing CSS selector: ${selector}`);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return source.slice(open + 1, index);
  }
  throw new Error(`Unclosed CSS block: ${selector}`);
}

describe("desktop research layout contract", () => {
  it("uses two columns until the evidence inspector is selected", () => {
    const columns = cssBlock(researchCss, ".research-columns");
    expect(columns).toContain("grid-template-columns: clamp(196px, 15vw, 216px) minmax(0, 1fr)");

    expect(researchCss).toMatch(/\.research-evidence\s*\{[\s\S]*?position:\s*absolute/);
    expect(researchCss).toMatch(/\.research-evidence\s*\{[\s\S]*?transform:\s*translateX\(calc\(100% \+ var\(--space-3\)\)\)/);
    expect(researchCss).toMatch(/\.research-evidence\s*\{[\s\S]*?pointer-events:\s*none/);

    const selectedEvidence = cssBlock(researchCss, ".research-evidence.has-selection");
    expect(selectedEvidence).toContain("transform: translateX(0)");
    expect(selectedEvidence).toContain("pointer-events: auto");
  });

  it("keeps the desktop evidence close action available", () => {
    const close = cssBlock(researchCss, ".research-evidence.has-selection .research-evidence-close");
    expect(close).toContain("display: inline-grid");
  });

  it("keeps repeated controls at the dense touch target", () => {
    const historyButton = cssBlock(researchCss, ".research-citation-history button");
    expect(historyButton).toContain("width: var(--touch-dense)");
    expect(historyButton).toContain("height: var(--touch-dense)");

    const markRead = cssBlock(researchCss, ".research-stock-mark-read");
    expect(markRead).toContain("min-height: var(--touch-dense)");
  });

  it("keeps the desktop composer as a compact single row", () => {
    const composer = cssBlock(researchCss, ".research-composer");
    expect(composer).toContain("height: calc(var(--touch-dense) + var(--space-1))");
    expect(composer).toContain("padding: 0");

    const row = cssBlock(researchCss, ".research-composer-row");
    expect(row).toContain("align-items: center");

    const label = cssBlock(researchCss, ".research-composer-label");
    expect(label).toContain("display: none");

    const riskBoundary = cssBlock(researchCss, ".research-risk-boundary");
    expect(riskBoundary).toContain("position: static");
  });
});
