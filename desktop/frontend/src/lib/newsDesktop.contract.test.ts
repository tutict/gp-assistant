import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);
const researchCss = readFileSync(new URL("../styles/research.css", import.meta.url), "utf8");
const newsRagPanel = readFileSync(new URL("../components/panels/NewsRagPanel.tsx", import.meta.url), "utf8");

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

function cssBlockLast(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const matches = [...source.matchAll(new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{`, "g"))];
  const match = matches.at(-1);
  if (!match || match.index == null) throw new Error(`Missing CSS selector: ${selector}`);
  const open = source.indexOf("{", match.index);
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

  it("renders a timeline rail and aligns desktop event dots to it", () => {
    const eventList = cssBlock(researchCss, ".research-event-list");
    expect(eventList).toContain("position: relative");
    expect(researchCss).toMatch(/\.research-event-list::before\s*\{[\s\S]*?position:\s*absolute[\s\S]*?width:\s*1px[\s\S]*?background:\s*var\(--line-soft\)/);

    const event = cssBlock(researchCss, ".research-event");
    expect(event).toContain("position: relative");
    expect(researchCss).toMatch(/\.research-event-dot-desktop\s*\{[\s\S]*?position:\s*relative/);
  });

  it("limits the desktop stream reading column", () => {
    const streamBody = cssBlock(researchCss, ".research-stream-body");
    expect(streamBody).toContain("max-width: 860px");
    expect(streamBody).toContain("margin-inline: auto");
  });

  it("keeps the reading column clear of an open evidence inspector", () => {
    const selectedColumns = cssBlock(
      researchCss,
      ".research-columns:has(.research-evidence.has-selection)",
    );
    expect(selectedColumns).toContain("grid-template-columns: clamp(196px, 15vw, 216px) minmax(0, 1fr) clamp(280px, 22vw, 320px)");
    const selectedEvidence = cssBlock(
      researchCss,
      ".research-columns:has(.research-evidence.has-selection) .research-evidence.has-selection",
    );
    expect(selectedEvidence).toContain("position: relative");
    expect(selectedEvidence).toContain("grid-column: 3");
  });

  it("uses a borderless brief stat row and a single-line zero state", () => {
    const stat = cssBlock(researchCss, ".research-stat");
    expect(stat).toContain("border: 0");
    expect(stat).toContain("background: transparent");

    expect(newsRagPanel).toMatch(/className="[^"]*research-brief-zero[^"]*"/);
    expect(newsRagPanel).toMatch(/ResearchBriefCounts[\s\S]*?hasBriefStats/);
    expect(newsRagPanel).toMatch(/function ResearchBriefExpanded[\s\S]*?const hasBriefStats[\s\S]*?if \(!hasBriefStats\)/);
    expect(cssBlock(researchCss, ".research-brief-zero")).toContain("white-space: nowrap");
  });

  it("keeps selected evidence readable and actionable on desktop", () => {
    const title = cssBlock(researchCss, ".research-evidence-card h2");
    expect(title).toContain("font-size: var(--fs-title)");

    const quote = cssBlockLast(researchCss, ".research-evidence-card blockquote");
    expect(quote).toContain("border-left: 2px solid var(--accent)");
    expect(quote).toContain("border: 0");
    expect(quote).toContain("max-height: 40%");
    expect(quote).toContain("overflow-y: auto");
    expect(quote).toContain("font-size: var(--fs-label)");
    expect(quote).toContain("background: var(--surface-2)");

    const sourceLink = cssBlock(researchCss, ".research-evidence-card > a");
    expect(sourceLink).toContain("min-height: var(--touch-dense)");
    expect(sourceLink).toContain("border: 1px solid var(--line)");
    expect(sourceLink).toContain("background: transparent");
  });

  it("uses typography tokens for research scope labels", () => {
    const scopeTag = cssBlock(researchCss, ".research-scope-tag");
    expect(scopeTag).toContain("font-size: var(--fs-caption)");
    expect(scopeTag).not.toMatch(/font-size:\s*\d+(?:\.\d+)?rem/);
  });
});
