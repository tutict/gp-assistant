import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it } from "vitest";
import { RawJson } from "./RawJson";

describe("RawJson", () => {
  it("hides the raw payload when a non-development panel disables it", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<RawJson result={{ secret: "value" }} enabled={false} />);
    });
    expect(renderer.toJSON()).toBeNull();
  });

  it("keeps the raw payload available when explicitly enabled", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<RawJson result={{ marker: "visible" }} enabled />);
    });
    const markup = JSON.stringify(renderer.toJSON());
    expect(markup).toContain("原始 JSON");
    expect(markup).toContain("visible");
  });
});
