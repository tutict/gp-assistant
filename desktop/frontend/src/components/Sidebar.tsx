interface SidebarProps {
  view: string;
  onNavigate: (view: "screen" | "observe" | "backtest" | "news" | "agent") => void;
  open: boolean;
  onClose: () => void;
}

const NAV_ITEMS = [
  { key: "screen", label: "选股", icon: "screen", href: "#sectionScreen" },
  { key: "observe", label: "观察", icon: "observe", href: "#sectionObserve" },
  { key: "backtest", label: "回测", icon: "backtest", href: "#sectionBacktest" },
  { key: "news", label: "消息", icon: "news", href: "#sectionNewsRag" },
  { key: "agent", label: "Agent", icon: "agent", href: "#sectionAgent" },
] as const;

const ICONS: Record<string, string> = {
  screen: "M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z",
  observe: "M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2zm0 18a8 8 0 1 1 8-8 8 8 0 0 1-8 8zM12 8v4l3 3",
  backtest: "M3 17l6-6 4 4 8-8M17 7h4v4",
  news: "M4 4h16v16H4zM8 8h8M8 12h8M8 16h5",
  agent: "M12 2a3 3 0 0 1 3 3v1a3 3 0 0 1-3 3 3 3 0 0 1-3-3V5a3 3 0 0 1 3-3zM5 21v-2a7 7 0 0 1 14 0v2",
};

export function Sidebar({ view, onNavigate, open, onClose }: SidebarProps) {
  return (
    <>
      {open && <div className="mobile-nav-overlay visible" onClick={onClose} />}
      <nav
        className={`sidebar ${open ? "open" : ""}`}
      >
        <button className="mobile-nav-close" type="button" onClick={onClose} aria-label="关闭导航">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="6" y1="6" x2="18" y2="18" />
            <line x1="18" y1="6" x2="6" y2="18" />
          </svg>
        </button>
        <ul className="nav-items">
          {NAV_ITEMS.map((item) => (
            <li key={item.key}>
              <a
                href={item.href}
                className={`nav-link ${view === item.key ? "active" : ""}`}
                data-view-link={item.key}
                onClick={(e) => {
                  e.preventDefault();
                  onNavigate(item.key);
                }}
              >
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d={ICONS[item.icon]} />
                </svg>
                <span>{item.label}</span>
              </a>
            </li>
          ))}
        </ul>
      </nav>
    </>
  );
}
