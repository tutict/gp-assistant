import { describe, expect, it } from "vitest";

describe("Android research workspace density", () => {
  it("keeps the research feed compact even above the generic mobile breakpoint", async () => {
    const nodeFs = "node:fs";
    const { readFileSync } = await import(nodeFs);
    const responsiveCss = readFileSync(new URL("../styles/responsive.css", import.meta.url), "utf8");

    expect(responsiveCss).toMatch(
      /html\.android-phone \.research-workspace\s*\{[^}]*--research-header:\s*48px;[^}]*border:\s*0;/s,
    );
    expect(responsiveCss).toMatch(
      /html\.android-phone \.research-actions button\s*\{[^}]*width:\s*36px;[^}]*font-size:\s*0;/s,
    );
    expect(responsiveCss).toMatch(
      /html\.android-phone \.research-event-content > strong\s*\{[^}]*font-size:\s*11px;/s,
    );
    expect(responsiveCss).toMatch(
      /html\.android-phone \.research-composer > button\s*\{[^}]*width:\s*36px;[^}]*height:\s*36px;/s,
    );
    expect(responsiveCss).toMatch(
      /html\.android-phone \.research-inbox\s*\{[^}]*position:\s*fixed;[^}]*transform:\s*translateX\(-102%\);/s,
    );
    expect(responsiveCss).toMatch(
      /html\.android-phone \.research-mobile-overlay\s*\{[^}]*position:\s*fixed;[^}]*display:\s*block;/s,
    );
  });
});
