import { describe, expect, it, vi } from "vitest";
import { createSettingsRegistry } from "./settingsRegistry";

describe("createSettingsRegistry", () => {
  it("exposes density and theme through injected state setters", () => {
    const setDensity = vi.fn();
    const setTheme = vi.fn();
    const setFontScale = vi.fn();
    const settings = createSettingsRegistry({
      density: "comfortable",
      setDensity,
      theme: "dark",
      setTheme,
      fontScale: "standard",
      setFontScale,
    });

    expect(settings.map((setting) => setting.key)).toEqual(["density", "theme", "fontScale"]);
    expect(settings[0].get()).toBe("comfortable");
    expect(settings[1].get()).toBe("dark");
    expect(settings[2].get()).toBe("standard");

    settings[0].set("compact");
    settings[1].set("light");
    settings[2].set("large");
    expect(setDensity).toHaveBeenCalledWith("compact");
    expect(setTheme).toHaveBeenCalledWith("light");
    expect(setFontScale).toHaveBeenCalledWith("large");
  });
});
