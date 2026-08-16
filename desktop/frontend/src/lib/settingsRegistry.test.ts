import { describe, expect, it, vi } from "vitest";
import { createSettingsRegistry } from "./settingsRegistry";

describe("createSettingsRegistry", () => {
  it("exposes density and theme through injected state setters", () => {
    const setDensity = vi.fn();
    const setTheme = vi.fn();
    const settings = createSettingsRegistry({
      density: "comfortable",
      setDensity,
      theme: "dark",
      setTheme,
    });

    expect(settings.map((setting) => setting.key)).toEqual(["density", "theme"]);
    expect(settings[0].get()).toBe("comfortable");
    expect(settings[1].get()).toBe("dark");

    settings[0].set("compact");
    settings[1].set("light");
    expect(setDensity).toHaveBeenCalledWith("compact");
    expect(setTheme).toHaveBeenCalledWith("light");
  });
});
