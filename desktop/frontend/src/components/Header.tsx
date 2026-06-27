interface HeaderProps {
  theme: "dark" | "light";
  onToggleTheme: () => void;
  onToggleMobileNav: () => void;
}

export function Header({ theme, onToggleTheme, onToggleMobileNav }: HeaderProps) {
  return (
    <header className="app-header">
      <button
        className="mobile-nav-toggle"
        type="button"
        aria-label="打开导航"
        onClick={onToggleMobileNav}
      >
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <line x1="3" y1="6" x2="21" y2="6" />
          <line x1="3" y1="12" x2="21" y2="12" />
          <line x1="3" y1="18" x2="21" y2="18" />
        </svg>
      </button>

      <div className="app-brand">
        <img src="/assets/logo-mark-v2.png" alt="股选优" className="app-logo" />
        <span className="app-title">股选优</span>
      </div>

      <div className="header-actions">
        <label className="theme-toggle">
          <input
            type="checkbox"
            checked={theme === "dark"}
            onChange={onToggleTheme}
          />
          <span className="theme-toggle-track">
            <span className="theme-toggle-thumb" />
          </span>
          <span className="theme-toggle-label">
            {theme === "dark" ? "暗色模式" : "亮色模式"}
          </span>
        </label>
      </div>
    </header>
  );
}
