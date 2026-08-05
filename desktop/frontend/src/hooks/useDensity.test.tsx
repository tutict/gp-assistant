import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDensity } from "./useDensity";

function DensityProbe() {
  const { density, toggleDensity } = useDensity();
  return <button type="button" onClick={toggleDensity}>{density}</button>;
}

describe("useDensity", () => {
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

  it("defaults to comfortable and persists a compact toggle", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DensityProbe />);
    });

    expect(renderer.root.findByType("button").children).toEqual(["comfortable"]);
    await act(async () => {
      renderer.root.findByType("button").props.onClick();
    });

    expect(renderer.root.findByType("button").children).toEqual(["compact"]);
    expect(values.get("stock-optimizer-density")).toBe("compact");
    expect(document.documentElement.dataset.density).toBe("compact");
  });

  it("uses the density query parameter before local storage", async () => {
    values.set("stock-optimizer-density", "comfortable");
    vi.stubGlobal("window", { location: { search: "?density=compact" } });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<DensityProbe />);
    });

    expect(renderer.root.findByType("button").children).toEqual(["compact"]);
  });
});
