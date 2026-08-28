import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const tokensCss = readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8");
const indexHtml = readFileSync(new URL("../../index.html", import.meta.url), "utf8");

function cssBlock(source: string, selector: string): string {
  const start = source.indexOf(selector);
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
});
