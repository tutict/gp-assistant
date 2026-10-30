import { useEffect, useId, useRef, useState } from "react";
import { Database, MoreHorizontal, Settings, Activity } from "lucide-react";

export function ResearchMore({ onKnowledge, onModel, onStatus, modelDisabled = false }: {
  onKnowledge: () => void; onModel: () => void; onStatus: () => void; modelDisabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = () => { setOpen(false); trigger.current?.focus(); };
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLButtonElement>("[role='menuitem']")?.focus();
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) close();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); close(); }
    };
    document.addEventListener?.("pointerdown", outside);
    document.addEventListener?.("keydown", escape);
    return () => { document.removeEventListener?.("pointerdown", outside); document.removeEventListener?.("keydown", escape); };
  }, [open]);
  return <div className="research-more" ref={root}>
    <button type="button" className="sentiment-icon-action research-more-trigger" ref={trigger}
      aria-label="更多研究操作" aria-expanded={open} aria-haspopup="menu" aria-controls={id}
      onClick={() => open ? close() : setOpen(true)}><MoreHorizontal size={16} aria-hidden="true" /><span>更多</span></button>
    {open && <div className="research-more-menu" id={id} role="menu" aria-label="研究管理" onKeyDown={(event) => {
      const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("[role='menuitem']:not([disabled])"));
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "ArrowDown" ? (index + 1) % items.length : event.key === "ArrowUp" ? (index - 1 + items.length) % items.length : event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : -1;
      if (next >= 0) { event.preventDefault(); items[next]?.focus(); }
    }}>
      <button type="button" role="menuitem" onClick={() => { close(); onKnowledge(); }}><Database size={16} aria-hidden="true" />知识库管理</button>
      <button type="button" role="menuitem" disabled={modelDisabled} onClick={() => { close(); onModel(); }}><Settings size={16} aria-hidden="true" />模型设置</button>
      <button type="button" role="menuitem" onClick={() => { close(); onStatus(); }}><Activity size={16} aria-hidden="true" />数据状态</button>
    </div>}
  </div>;
}
