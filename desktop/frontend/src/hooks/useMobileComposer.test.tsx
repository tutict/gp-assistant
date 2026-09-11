import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";
import { useMobileComposer } from "./useMobileComposer";

describe("composer input method protection", () => {
  it("does not submit during Chinese composition or the browser composition-end Enter", async () => {
    const submit = vi.fn();
    function Probe() {
      const composer = useMobileComposer("");
      return <textarea onCompositionStart={composer.onCompositionStart} onCompositionEnd={composer.onCompositionEnd}
        onKeyDown={event => { if (event.key === "Enter" && !composer.isComposing(event)) submit(); }} />;
    }
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<Probe />); });
    const input = renderer.root.findByType("textarea");
    await act(async () => {
      input.props.onCompositionStart();
      input.props.onKeyDown({ key: "Enter", nativeEvent: {} });
      input.props.onCompositionEnd();
      input.props.onKeyDown({ key: "Enter", nativeEvent: { isComposing: true } });
      input.props.onKeyDown({ key: "Enter", keyCode: 229, nativeEvent: {} });
    });
    expect(submit).not.toHaveBeenCalled();
    await act(async () => input.props.onKeyDown({key:"Enter",nativeEvent:{}}));
    expect(submit).toHaveBeenCalledOnce();
    await act(async () => renderer.unmount());
    vi.unstubAllGlobals();
  });
});
