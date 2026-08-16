import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);
const header = readFileSync(new URL("../components/Header.tsx", import.meta.url), "utf8");
const shell = readFileSync(new URL("../styles/shell.css", import.meta.url), "utf8");

function ruleBody(source: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return source.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

describe("header settings contract", () => {
  it("uses the settings trigger without the legacy density control or search label", () => {
    expect(header).toContain('className="settings-trigger"');
    expect(header).not.toContain("density-control");
    expect(header).not.toContain("header-search-label");
  });

  it("keeps actions adjacent to status and aligns icon controls", () => {
    expect(ruleBody(shell, ".header-actions")).not.toMatch(/margin-left\s*:\s*auto/);
    expect(ruleBody(shell, ".theme-toggle")).toMatch(/min-height\s*:\s*36px/);
    expect(ruleBody(shell, ".theme-toggle-label")).toMatch(/display\s*:\s*none/);
    expect(shell).toMatch(/\.settings-trigger,\s*\.shortcut-help-trigger\s*\{[^}]*min-height:\s*36px/s);
  });

  it("uses centered soft separators and tabular status values", () => {
    expect(ruleBody(shell, ".header-status > span")).not.toMatch(/border-left/);
    expect(shell).toMatch(/\.header-status > span::before\s*\{[^}]*height:\s*20px[^}]*background:\s*var\(--line-soft\)/s);
    expect(ruleBody(shell, ".header-status em")).toMatch(/line-height\s*:\s*1\.2/);
    expect(ruleBody(shell, ".header-status strong")).toMatch(/font-variant-numeric\s*:\s*tabular-nums/);
  });
});
