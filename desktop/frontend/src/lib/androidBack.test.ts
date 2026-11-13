import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { handleAndroidBack } from "./androidBack";

class BackKeyEvent extends Event {
  readonly key: string;
  constructor(type: string, options: KeyboardEventInit) {
    super(type, options);
    this.key = options.key || "";
  }
}

beforeEach(() => vi.stubGlobal("KeyboardEvent", BackKeyEvent));
afterEach(() => vi.unstubAllGlobals());

function backPage(activeElement: EventTarget | null, visible = true) {
  return Object.assign(new EventTarget(), {
    activeElement,
    querySelectorAll: () => [{ getClientRects: () => visible ? [{}] : [], closest: () => null }],
  });
}

it("lets the focused overlay consume Back without sending it to the page underneath", () => {
  const control = new EventTarget();
  const page = backPage(control);
  const pageHandler = vi.fn();
  const closeOverlay = vi.fn();
  page.addEventListener("keydown", pageHandler);
  control.addEventListener("keydown", (event) => {
    expect((event as BackKeyEvent).key).toBe("Escape");
    expect(event.bubbles).toBe(true);
    event.preventDefault();
    closeOverlay();
  });
  expect(handleAndroidBack(page as unknown as Document)).toBe(true);
  expect(closeOverlay).toHaveBeenCalledOnce();
  expect(pageHandler).not.toHaveBeenCalled();
});

it("falls back to the document when no element is focused", () => {
  const page = backPage(null);
  page.addEventListener("keydown", (event) => event.preventDefault());
  expect(handleAndroidBack(page as unknown as Document)).toBe(true);
});

it("allows native default Back when the page does not consume Escape", () => {
  const page = backPage(new EventTarget());
  expect(handleAndroidBack(page as unknown as Document)).toBe(false);
});

it("does not trap native Back on an input that consumes Escape when no popup is visible", () => {
  const control = new EventTarget();
  const inputEscape = vi.fn((event: Event) => event.preventDefault());
  control.addEventListener("keydown", inputEscape);
  const page = backPage(control, false);
  expect(handleAndroidBack(page as unknown as Document)).toBe(false);
  expect(inputEscape).not.toHaveBeenCalled();
});
