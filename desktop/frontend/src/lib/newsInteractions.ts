import { useCallback, useState } from "react";
import type { ResearchCitation, ResearchMessage, ResearchOverview } from "../types";

export interface MarkReadResult {
  messages: ResearchMessage[];
  overview: ResearchOverview | null;
}

export function applyMarkRead(
  messages: ResearchMessage[],
  overview: ResearchOverview | null,
  ids: string[],
): MarkReadResult {
  const targetIds = new Set(ids);
  const changedByStock = new Map<string, number>();
  let changed = 0;
  const nextMessages = messages.map((message) => {
    if (!targetIds.has(message.id) || !message.unread) return message;
    changed += 1;
    if (message.stock_code) {
      changedByStock.set(message.stock_code, (changedByStock.get(message.stock_code) || 0) + 1);
    }
    return { ...message, unread: false };
  });
  if (!overview || changed === 0) return { messages: nextMessages, overview };
  const unreadByStock = { ...(overview.unread_by_stock || {}) };
  for (const [stockCode, count] of changedByStock) {
    unreadByStock[stockCode] = Math.max(0, (unreadByStock[stockCode] || 0) - count);
  }
  return {
    messages: nextMessages,
    overview: {
      ...overview,
      unread_count: Math.max(0, overview.unread_count - changed),
      unread_by_stock: unreadByStock,
    },
  };
}

export function pushCitation(
  stack: ResearchCitation[],
  pointer: number,
  next: ResearchCitation,
): { stack: ResearchCitation[]; pointer: number } {
  if (stack[pointer]?.citation_id === next.citation_id) return { stack, pointer };
  const nextStack = [...stack.slice(0, pointer + 1), next];
  return { stack: nextStack, pointer: nextStack.length - 1 };
}

export function useEventSelection(initialId: string | null = null) {
  const [selectedId, setSelectedId] = useState<string | null>(initialId);
  const toggle = useCallback((id: string) => {
    setSelectedId((current) => current === id ? null : id);
  }, []);
  const close = useCallback(() => setSelectedId(null), []);
  return { selectedId, toggle, close };
}
