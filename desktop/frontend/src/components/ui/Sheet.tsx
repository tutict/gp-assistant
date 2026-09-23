import { useEffect, useRef, type MouseEvent, type ReactNode } from "react";

const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  "[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

interface SheetProps {
  open: boolean;
  onClose: () => void;
  label: string;
  children: ReactNode;
  className?: string;
  backdropClassName?: string;
}


function eventTargetsNestedDialog(event: KeyboardEvent, panel: HTMLElement | null): boolean {
  const target = event.target;
  if (typeof Node === "undefined" || !(target instanceof Node) || !panel || panel.contains(target)) return false;
  const element = target instanceof Element ? target : target.parentElement;
  return Boolean(element?.closest?.("[role='dialog']"));
}

function canFocus(value: unknown): value is HTMLElement {
  return typeof (value as HTMLElement | null)?.focus === "function";
}

export function Sheet({
  open,
  onClose,
  label,
  children,
  className = "",
  backdropClassName = "",
}: SheetProps) {
  const panelRef = useRef<HTMLElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    previouslyFocusedRef.current = canFocus(document.activeElement) ? document.activeElement : null;
    const panel = panelRef.current;
    const getFocusable = () => panel ? Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
      .filter((element) => !element.closest?.('[hidden], [inert]')
        && (typeof element.getClientRects !== "function" || element.getClientRects().length > 0)) : [];
    const focusable = getFocusable();
    const initialFocusTarget = focusable[0] ?? (canFocus(panel) ? panel : null);
    initialFocusTarget?.focus();
    const body = document.body;
    const previousOverflow = body?.style.overflow;
    if (body) body.style.overflow = "hidden";
    // Disable siblings along the ancestor path, leaving only this overlay interactive.
    const inertSiblings: Array<{ element: HTMLElement; previous: boolean }> = [];
    let branch = panel?.parentElement;
    while (branch?.parentElement && branch !== body) {
      for (const sibling of Array.from(branch.parentElement.children)) {
        if (sibling === branch || !(sibling instanceof HTMLElement)) continue;
        inertSiblings.push({ element: sibling, previous: sibling.inert });
        sibling.inert = true;
      }
      branch = branch.parentElement;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (eventTargetsNestedDialog(event, panel)) return;
        event.preventDefault();
        event.stopPropagation?.();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      if (eventTargetsNestedDialog(event, panel)) return;
      const current = getFocusable();
      if (current.length === 0) {
        event.preventDefault();
        panel?.focus();
        return;
      }
      const first = current[0];
      const last = current[current.length - 1];
      if (panel && !panel.contains(document.activeElement)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      if (body) body.style.overflow = previousOverflow ?? "";
      for (const { element, previous } of inertSiblings) element.inert = previous;
      previouslyFocusedRef.current?.focus();
      previouslyFocusedRef.current = null;
    };
  }, [open]);

  if (!open) return null;

  const handleBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };

  return (
    <div
      className={`sheet-backdrop ${backdropClassName}`.trim()}
      onMouseDown={handleBackdropMouseDown}
    >
      <section
        ref={panelRef}
        className={`sheet-panel ${className}`.trim()}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
      >
        {children}
      </section>
    </div>
  );
}
