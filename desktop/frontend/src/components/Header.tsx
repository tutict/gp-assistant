import { Menu, Moon, Sun } from "lucide-react";
import { IconButton } from "./ui/IconButton";

interface HeaderProps {
  theme: "dark" | "light";
  onToggleTheme: () => void;
  onToggleMobileNav: () => void;
}

export function Header({ theme, onToggleTheme, onToggleMobileNav }: HeaderProps) {
  return (
    <header className="app-header">
      <IconButton
        className="mobile-nav-toggle"
        label="打开导航"
        icon={<Menu size={21} aria-hidden="true" />}
        onClick={onToggleMobileNav}
      />

      <div className="app-brand">
        <img src="/assets/logo-mark-v2.png" alt="股选优" className="app-logo" />
        <span className="app-title">股选优</span>
      </div>

      <div className="header-actions">
        <button
          type="button"
          className="theme-toggle"
          aria-pressed={theme === "dark"}
          aria-label={theme === "dark" ? "切换到亮色模式" : "切换到暗色模式"}
          title={theme === "dark" ? "切换到亮色模式" : "切换到暗色模式"}
          onClick={onToggleTheme}
        >
          {theme === "dark" ? <Moon size={16} aria-hidden="true" /> : <Sun size={16} aria-hidden="true" />}
          <span className="theme-toggle-label">
            {theme === "dark" ? "暗色模式" : "亮色模式"}
          </span>
        </button>
      </div>
    </header>
  );
}