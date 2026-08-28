import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const tokensCss = readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8");
const researchCss = readFileSync(new URL("../styles/research.css", import.meta.url), "utf8");
const indexHtml = readFileSync(new URL("../../index.html", import.meta.url), "utf8");

function cssBlock(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`(?:^|\\n)[^\\n{}]*${escaped}\\s*\\{`).exec(source);
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

function declarations(block: string): Map<string, string> {
  return new Map(
    [...block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)]
      .map((match) => [match[1], match[2].trim()]),
  );
}

describe("Android light theme contract", () => {
  it("overrides every splash and Agent workspace color in the light theme", () => {
    const light = declarations(cssBlock(tokensCss, '[data-theme="light"]'));

    expect(Object.fromEntries([
      "--splash-bg",
      "--splash-text",
      "--agent-workspace-bg",
      "--agent-rail-bg",
      "--agent-stage-bg",
      "--agent-composer-bg",
      "--contrast-dark",
    ].map((token) => [token, light.get(token)]))).toEqual({
      "--splash-bg": "var(--bg)",
      "--splash-text": "var(--text)",
      "--agent-workspace-bg": "#f7f8fa",
      "--agent-rail-bg": "#ffffff",
      "--agent-stage-bg": "#f4f6f8",
      "--agent-composer-bg": "#eef1f5",
      "--contrast-dark": "#ffffff",
    });
  });

  it("renders the boot splash from theme variables without legacy dark colors", () => {
    expect(indexHtml).not.toContain("#120e0d");
    expect(indexHtml).not.toContain("#fff4ef");
    expect(indexHtml).toContain("background: var(--splash-bg)");
    expect(indexHtml).toContain("color: var(--splash-text)");
    expect(indexHtml).toContain('html[data-theme="light"]');
  });

  it("keeps the Android news summary, metadata, and composer compact", () => {
    const mobile = cssBlock(researchCss, "@media (max-width: 768px)");
    const brief = cssBlock(mobile, ".research-daily-brief");
    const sourceBadge = cssBlock(mobile, ".research-event-meta .research-badge");
    const composerRow = cssBlock(mobile, ".research-composer-row");
    const composerSend = cssBlock(mobile, ".research-composer-send");

    expect(brief).toContain("margin-top: var(--space-2)");
    expect(mobile).toContain(".research-brief-expanded");
    expect(sourceBadge).toContain("background: var(--surface-2)");
    expect(sourceBadge).toContain("color: var(--text-secondary)");
    expect(sourceBadge).not.toContain("--rise");
    expect(composerRow).toContain("grid-template-columns: minmax(0, 1fr) 40px");
    expect(composerSend).toContain("width: 40px");
    expect(mobile).toMatch(/\.research-composer-label[\s\S]*?display:\s*none/);
    expect(mobile).toMatch(/\.research-risk-boundary[\s\S]*?display:\s*none/);
  });

  it("uses one bottom-sheet language for the Android inbox and evidence inspector", () => {
    const mobile = cssBlock(researchCss, "@media (max-width: 768px)");
    const inbox = cssBlock(mobile, ".research-inbox");
    const evidence = cssBlock(mobile, ".research-evidence.has-selection");
    const handle = cssBlock(mobile, ".research-sheet-handle");

    expect(inbox).toContain("transform: translateY(calc(102% + var(--bottom-nav-height) + var(--safe-bottom)))");
    expect(inbox).toContain("border-radius: var(--radius-sheet) var(--radius-sheet) 0 0");
    expect(evidence).toContain("border-radius: var(--radius-sheet) var(--radius-sheet) 0 0");
    expect(evidence).toContain("animation: research-sheet-in var(--motion-standard) var(--ease-out)");
    expect(handle).toContain("width: 36px");
    expect(handle).toContain("height: 4px");
  });
});
