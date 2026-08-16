import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useFontScale } from "./useFontScale";

function FontScaleProbe() {
  const { fontScale, setFontScale } = useFontScale();
  return (
    <button type="button" onClick={() => setFontScale("large")}>
      {fontScale}
    </button>
  );
}

describe("useFontScale", () => {
  const values = new Map<string, string>();

  beforeEach(() => {
    values.clear();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("localStorage", {
      getItem: vi.fn((key: string) => values.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => values.set(key, value)),
    });
    vi.stubGlobal("window", { location: { search: "" } });
    vi.stubGlobal("document", { documentElement: { dataset: {} } });
  });

  it("defaults to standard and persists a large selection", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FontScaleProbe />);
    });

    expect(renderer.root.findByType("button").children).toEqual(["standard"]);
    await act(async () => renderer.root.findByType("button").props.onClick());

    expect(values.get("stock-optimizer-font-scale")).toBe("large");
    expect(document.documentElement.dataset.fontScale).toBe("large");
  });

  it("uses the fontScale query parameter before local storage", async () => {
    values.set("stock-optimizer-font-scale", "large");
    vi.stubGlobal("window", { location: { search: "?fontScale=small" } });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<FontScaleProbe />);
    });

    expect(renderer.root.findByType("button").children).toEqual(["small"]);
  });
});
