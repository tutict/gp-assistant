import { Bot, ChartNoAxesCombined, Eye, LayoutGrid, Newspaper, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { IconButton } from "./ui/IconButton";

interface SidebarProps {
  view: string;
  onNavigate: (view: "screen" | "observe" | "backtest" | "news" | "agent") => void;
  open: boolean;
  onClose: () => void;
}

type ViewKey = "screen" | "observe" | "backtest" | "news" | "agent";

const NAV_ITEMS: Array<{ key: ViewKey; label: string; icon: LucideIcon; href: string }> = [
  { key: "screen", label: "选股", icon: LayoutGrid, href: "#sectionScreen" },
  { key: "observe", label: "观察", icon: Eye, href: "#sectionObserve" },
  { key: "backtest", label: "回测", icon: ChartNoAxesCombined, href: "#sectionBacktest" },
  { key: "news", label: "消息", icon: Newspaper, href: "#sectionNewsRag" },
  { key: "agent", label: "Agent", icon: Bot, href: "#sectionAgent" },
];

export function Sidebar({ view, onNavigate, open, onClose }: SidebarProps) {
  return (
    <>
      {open && <div className="mobile-nav-overlay visible" onClick={onClose} />}
      <nav className={`sidebar ${open ? "open" : ""}`} aria-label="主导航">
        <IconButton
          className="mobile-nav-close"
          onClick={onClose}
          label="关闭导航"
          icon={<X size={20} aria-hidden="true" />}
        />
        <ul className="nav-items">
          {NAV_ITEMS.map((item) => {
            const NavIcon = item.icon;
            return (
              <li key={item.key}>
                <a
                  href={item.href}
                  className={`nav-link ${view === item.key ? "active" : ""}`}
                  data-view-link={item.key}
                  aria-current={view === item.key ? "page" : undefined}
                  onClick={(event) => {
                    event.preventDefault();
                    onNavigate(item.key);
                  }}
                >
                  <NavIcon size={20} strokeWidth={1.8} aria-hidden="true" />
                  <span>{item.label}</span>
                </a>
              </li>
            );
          })}
        </ul>
      </nav>
    </>
  );
}