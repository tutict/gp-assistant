import { describe, expect, it } from "vitest";
import { createSettingsRegistry } from "./settingsRegistry";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);
const header = readFileSync(new URL("../components/Header.tsx", import.meta.url), "utf8");
const shell = readFileSync(new URL("../styles/shell.css", import.meta.url), "utf8");
const tokens = readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8");
const responsive = readFileSync(new URL("../styles/responsive.css", import.meta.url), "utf8");
const screenshotHarness = readFileSync(new URL("../../../../scripts/ui-screenshot.mjs", import.meta.url), "utf8");

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

  it("centers the desktop stock search independently of surrounding controls", () => {
    const search = ruleBody(shell, ".header-search");
    expect(search).toMatch(/position\s*:\s*absolute/);
    expect(search).toMatch(/left\s*:\s*50%/);
    expect(search).toMatch(/transform\s*:\s*translateX\(-50%\)/);
    expect(search).not.toMatch(/margin-left/);
  });

  it("uses centered soft separators and tabular status values", () => {
    expect(ruleBody(shell, ".header-status > span")).not.toMatch(/border-left/);
    expect(shell).toMatch(/\.header-status > span::before\s*\{[^}]*height:\s*20px[^}]*background:\s*var\(--line-soft\)/s);
    expect(ruleBody(shell, ".header-status em")).toMatch(/line-height\s*:\s*1\.2/);
    expect(ruleBody(shell, ".header-status strong")).toMatch(/font-variant-numeric\s*:\s*tabular-nums/);
  });

  it("keeps font scale overrides token-only and registers all initial settings", () => {
    for (const scale of ["small", "large"]) {
      const body = ruleBody(tokens, `:root[data-font-scale="${scale}"]`);
      const declarations = body.split(";").map((item) => item.trim()).filter(Boolean);
      expect(declarations).toHaveLength(5);
      expect(declarations.every((declaration) => declaration.startsWith("--fs-"))).toBe(true);
    }

    const settings = createSettingsRegistry({
      density: "comfortable",
      setDensity: () => undefined,
      theme: "dark",
      setTheme: () => undefined,
      fontScale: "standard",
      setFontScale: () => undefined,
    });
    expect(settings.map((setting) => setting.key)).toEqual(["density", "theme", "fontScale", "market"]);
    expect(settings.find((setting) => setting.key === "market")?.disabled).toBe(true);
  });

  it("defines the mobile bottom sheet and the requested screenshot matrix", () => {
    expect(responsive).toMatch(/\.settings-sheet\s*\{[^}]*max-height:\s*78dvh[^}]*padding-bottom:\s*var\(--safe-bottom\)/s);
    for (const device of ["desktop-1440", "desktop-1920", "phone-390"]) {
      expect(screenshotHarness).toContain(`name: "${device}-light"`);
    }
    expect(screenshotHarness).toContain('"header-default.png"');
    expect(screenshotHarness).toContain('"settings-open.png"');
  });
});
