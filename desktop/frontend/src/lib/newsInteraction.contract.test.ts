import { describe, expect, it } from "vitest";

const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const panel = readFileSync(new URL("../components/panels/NewsRagPanel.tsx", import.meta.url), "utf8");
const styles = readFileSync(new URL("../styles/research.css", import.meta.url), "utf8");
const styleDirectory = new URL("../styles/", import.meta.url);

describe("news interaction contract", () => {
  it("uses in-app deletion confirmation and shared interaction helpers", () => {
    expect(panel).not.toContain("window.confirm");
    expect(panel).toContain("applyMarkRead");
    expect(panel).toContain("pushCitation");
    expect(panel).toContain("useEventSelection");
  });

  it("keeps the research primitives tokenized", () => {
    for (const selector of [".research-dot", ".research-stat", ".research-badge", ".research-pill"]) {
      expect(styles).toContain(`${selector} {`);
    }
    const badge = styles.match(/\.research-badge\s*\{([^}]*)\}/)?.[1] || "";
    expect(badge).not.toMatch(/--(?:rise|fall)/);
  });

  it("removes legacy rag-prefixed selectors from every stylesheet", async () => {
    const { readdirSync } = await import(nodeFs);
    for (const file of readdirSync(styleDirectory)) {
      if (!file.endsWith(".css")) continue;
      const source = readFileSync(new URL(file, styleDirectory), "utf8");
      expect(source).not.toMatch(/\.rag-/);
    }
  });
});
