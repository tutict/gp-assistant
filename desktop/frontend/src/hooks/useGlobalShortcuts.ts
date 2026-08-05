import { useEffect } from "react";

export type ShortcutView = "screen" | "observe" | "backtest" | "news" | "agent";
export type GlobalShortcutAction =
  | { type: "focus-search" }
  | { type: "navigate"; view: ShortcutView }
  | { type: "toggle-help" }
  | { type: "escape" };

export interface ShortcutEventLike {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  isComposing?: boolean;
  target?: EventTarget | { tagName?: string; isContentEditable?: boolean } | null;
  preventDefault?: () => void;
}

const VIEW_BY_NUMBER: Record<string, ShortcutView> = {
  "1": "screen",
  "2": "observe",
  "3": "backtest",
  "4": "news",
  "5": "agent",
};

export function isEditableTarget(target: ShortcutEventLike["target"]): boolean {
  const element = target as { tagName?: string; isContentEditable?: boolean } | null | undefined;
  const tagName = element?.tagName?.toLowerCase();
  return tagName === "input"
    || tagName === "textarea"
    || tagName === "select"
    || element?.isContentEditable === true;
}

export function resolveGlobalShortcut(event: ShortcutEventLike): GlobalShortcutAction | null {
  if (event.isComposing) return null;
  if (event.key === "Escape") return { type: "escape" };
  if (event.key.toLowerCase() === "k" && (event.ctrlKey || event.metaKey)) {
    return { type: "focus-search" };
  }
  if (isEditableTarget(event.target)) return null;
  if (!event.altKey && !event.ctrlKey && !event.metaKey && event.key === "/") {
    return { type: "focus-search" };
  }
  if (!event.altKey && !event.ctrlKey && !event.metaKey && VIEW_BY_NUMBER[event.key]) {
    return { type: "navigate", view: VIEW_BY_NUMBER[event.key] };
  }
  if (!event.altKey && !event.ctrlKey && !event.metaKey && event.key === "?") {
    return { type: "toggle-help" };
  }
  return null;
}

interface GlobalShortcutHandlers {
  onFocusSearch: () => void;
  onNavigate: (view: ShortcutView) => void;
  onToggleHelp: () => void;
  onEscape: () => void;
}

export function useGlobalShortcuts({
  onFocusSearch,
  onNavigate,
  onToggleHelp,
  onEscape,
}: GlobalShortcutHandlers): void {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const action = resolveGlobalShortcut(event);
      if (!action) return;
      event.preventDefault();
      if (action.type === "focus-search") onFocusSearch();
      else if (action.type === "navigate") onNavigate(action.view);
      else if (action.type === "toggle-help") onToggleHelp();
      else onEscape();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onEscape, onFocusSearch, onNavigate, onToggleHelp]);
}
