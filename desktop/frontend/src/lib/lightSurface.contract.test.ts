import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

function read(name: string): string {
  return readFileSync(new URL(`../styles/${name}`, import.meta.url), "utf8");
}

describe("light surface edges", () => {
  it("gives pale content blocks a solid edge", () => {
    const sentiment = read("sentiment.css");
    const pages = read("pages.css");
    const screen = read("screen.css");
    const components = read("components.css");
    for (const rule of [".sentiment-empty", ".sentiment-notice, .sentiment-error", ".sentiment-progress", ".sentiment-quality"]) {
      expect(sentiment).toContain(`${rule} {`);
    }
    expect(sentiment).toMatch(/\.sentiment-empty \{[^}]*border: 1px solid var\(--line\)/);
    expect(sentiment).toMatch(/\.sentiment-notice, \.sentiment-error \{[^}]*border: 1px solid var\(--line\)/);
    expect(sentiment).toMatch(/\.sentiment-progress \{[^}]*border: 1px solid var\(--line\)/);
    expect(sentiment).toMatch(/\.sentiment-quality \{[^}]*border: 1px solid var\(--line\)[^}]*border-left: 2px solid var\(--control-line\)/);
    expect(pages).toMatch(/\.watchlist-item \{[\s\S]*?border: 1px solid var\(--line\)/);
    expect(pages).toMatch(/\.raw-result \{[\s\S]*?border: 1px solid var\(--line\)/);
    expect(pages).toContain(".checklist span { border: 1px solid var(--line-soft); }");
    expect(pages).toContain(".pack-state { border: 1px solid var(--line-soft); }");
    expect(pages).toMatch(/\.observe-result \.state-pill\.neutral \{[\s\S]*?border: 1px solid var\(--line-soft\)/);
    expect(screen).toMatch(/\.stock-details \{[^}]*border: 1px solid var\(--line\)/);
    expect(components).toMatch(/\.group-label \{[^}]*border-bottom: 1px solid var\(--line\)/);
  });

  it("uses the theme elevation shadow instead of a light-mode white shadow", () => {
    const settings = read("components.css");
    const shell = read("shell.css");
    const pages = read("pages.css");
    const research = read("research.css");
    expect(settings).toMatch(/\.settings-sheet \{[\s\S]*?box-shadow: 0 24px 60px var\(--elev-float\)/);
    expect(shell).toMatch(/\.shortcut-help \{[\s\S]*?box-shadow: 0 24px 60px var\(--elev-float\)/);
    expect(pages).toMatch(/\.stock-suggest,\s*\.market-confirm \{[\s\S]*?box-shadow: 0 12px 28px var\(--elev-float\)/);
    expect(pages).toMatch(/\.screen-refresh-maintenance-panel \{[\s\S]*?box-shadow: 0 12px 28px var\(--elev-float\)/);
    expect(pages).toMatch(/\.equity-chart-tooltip \{[\s\S]*?box-shadow: 0 5px 18px var\(--elev-float\)/);
    expect(pages).toMatch(/\.agent-run-drawer \{[\s\S]*?box-shadow: -12px 0 28px var\(--elev-float\)/);
    expect(research).toMatch(/\.research-error \.panel-feedback \{[\s\S]*?box-shadow: 0 12px 30px var\(--elev-float\)/);
    expect(research).toMatch(/\.research-evidence\.has-selection \{[\s\S]*?box-shadow: 0 16px 50px var\(--elev-float\)/);
    expect(research).not.toContain("rgb(21 31 42 / 18%)");
    const settingsRule = settings.slice(settings.indexOf(".settings-sheet {"), settings.indexOf(".settings-sheet {") + 700);
    const shortcutRule = shell.slice(shell.indexOf(".shortcut-help {"), shell.indexOf(".shortcut-help {") + 500);
    expect(settingsRule).not.toContain("contrast-dark");
    expect(shortcutRule).not.toContain("contrast-dark");
  });
});
