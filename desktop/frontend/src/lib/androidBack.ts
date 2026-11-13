declare global {
  interface Window {
    __gpHandleAndroidBack?: () => boolean;
  }
}

/** Give the focused overlay first refusal before Android performs its default Back action. */
export function handleAndroidBack(root: Document = document): boolean {
  const hasOverlay = Array.from(root.querySelectorAll<HTMLElement>(
    '[role="dialog"], [role="menu"], .stock-suggest, .market-confirm, .research-more-menu',
  )).some(element => element.getClientRects().length > 0
    && !element.closest('[hidden], [inert], [aria-hidden="true"]'));
  if (!hasOverlay) return false;
  const event = new KeyboardEvent("keydown", {
    key: "Escape",
    code: "Escape",
    bubbles: true,
    cancelable: true,
  });
  (root.activeElement || root).dispatchEvent(event);
  return event.defaultPrevented;
}
