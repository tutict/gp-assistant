import { CircleHelp, Menu, Moon, Search, Sun, X } from "lucide-react";
import { useEffect, useRef } from "react";
import type { Ref } from "react";
import type { DataStatus } from "../types";
import type { Density } from "../hooks/useDensity";
import { formatBytes, formatMarketRefreshDate } from "../lib/format";
import { StockCodeInput } from "./StockCodeInput";
import { IconButton } from "./ui/IconButton";

interface HeaderProps {
  theme: "dark" | "light";
  density: Density;
  searchCode: string;
  searchInputRef: Ref<HTMLInputElement>;
  watchlistCount: number;
  dataStatus: DataStatus | null;
  shortcutHelpOpen: boolean;
  onSearchCodeChange: (value: string) => void;
  onSearchCommit: (value: string) => void;
  onToggleDensity: () => void;
  onToggleHelp: () => void;
  onToggleTheme: () => void;
  onToggleMobileNav: () => void;
}

const SHORTCUTS = [
  ["Ctrl+K / /", "聚焦搜股"],
  ["1 - 5", "切换工作区"],
  ["?", "显示快捷键"],
  ["Esc", "关闭浮层"],
] as const;

function formatCount(value: unknown): string {
  const count = Number(value);
  return Number.isFinite(count) ? Math.max(0, Math.round(count)).toLocaleString("zh-CN") : "--";
}

function freshnessLabel(status: DataStatus | null): string {
  if (!status) return "待检查";
  if (status.stale === true) return "待更新";
  if (status.quote_trade_date && status.current_trade_date) {
    return status.quote_trade_date === status.current_trade_date ? "最新" : "待更新";
  }
  return status.stale === false ? "最新" : "待检查";
}

export function Header({
  theme,
  density,
  searchCode,
  searchInputRef,
  watchlistCount,
  dataStatus,
  shortcutHelpOpen,
  onSearchCodeChange,
  onSearchCommit,
  onToggleDensity,
  onToggleHelp,
  onToggleTheme,
  onToggleMobileNav,
}: HeaderProps) {
  const refreshDate = dataStatus?.quote_trade_date
    ?? dataStatus?.quote_generated_at
    ?? dataStatus?.generated_at
    ?? dataStatus?.universe_updated_at;
  const helpRef = useRef<HTMLElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!shortcutHelpOpen) {
      previouslyFocusedRef.current?.focus();
      previouslyFocusedRef.current = null;
      return;
    }
    const activeElement = document.activeElement;
    previouslyFocusedRef.current = activeElement && typeof (activeElement as HTMLElement).focus === "function"
      ? activeElement as HTMLElement
      : null;
    const timer = window.setTimeout(() => {
      helpRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [shortcutHelpOpen]);

  return (
    <>
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

        <div className="header-search" role="search">
          <label className="header-search-label" htmlFor="global-stock-search">搜股</label>
          <div className="header-search-field">
            <Search size={16} aria-hidden="true" />
            <StockCodeInput
              id="global-stock-search"
              value={searchCode}
              onChange={onSearchCodeChange}
              onCommit={onSearchCommit}
              inputRef={searchInputRef}
              inputAriaLabel="搜索股票"
              placeholder="代码 / 名称"
              resolveBareCode
            />
          </div>
        </div>

        <div className="header-status" aria-label="全局数据状态">
          <span><em>自选</em><strong>{formatCount(watchlistCount)}</strong></span>
          <span><em>股票池</em><strong>{formatCount(dataStatus?.universe_count)}</strong></span>
          <span><em>数据</em><strong>{formatMarketRefreshDate(refreshDate)}</strong></span>
          <span title={dataStatus?.cache_bytes == null ? undefined : "缓存 " + formatBytes(dataStatus.cache_bytes)}>
            <em>缓存</em>
            <strong className={freshnessLabel(dataStatus) === "待更新" ? "status-warning" : ""}>
              {freshnessLabel(dataStatus)}
            </strong>
          </span>
        </div>

        <div className="header-actions">
          <div className="density-control" aria-label="信息密度">
            <button
              type="button"
              className={density === "comfortable" ? "active" : ""}
              aria-pressed={density === "comfortable"}
              aria-label="切换到舒适密度"
              onClick={density === "comfortable" ? undefined : onToggleDensity}
            >
              舒适
            </button>
            <button
              type="button"
              className={density === "compact" ? "active" : ""}
              aria-pressed={density === "compact"}
              aria-label="切换到紧凑密度"
              onClick={density === "compact" ? undefined : onToggleDensity}
            >
              紧凑
            </button>
          </div>
          <IconButton
            className="shortcut-help-trigger"
            label="快捷键帮助"
            icon={<CircleHelp size={17} aria-hidden="true" />}
            onClick={onToggleHelp}
          />
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

      {shortcutHelpOpen ? (
        <div className="shortcut-help-backdrop" onMouseDown={onToggleHelp}>
          <section
            ref={helpRef}
            className="shortcut-help"
            role="dialog"
            aria-modal="true"
            aria-label="快捷键帮助"
            onMouseDown={(event) => event.stopPropagation()}
            onKeyDown={(event) => {
              if (event.key !== "Tab") return;
              event.preventDefault();
              helpRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
            }}
          >
            <header>
              <div><span>桌面快捷键</span><strong>快速定位工作区</strong></div>
              <IconButton
                label="关闭快捷键帮助"
                icon={<X size={18} aria-hidden="true" />}
                onClick={onToggleHelp}
              />
            </header>
            <dl>
              {SHORTCUTS.map(([keys, action]) => (
                <div key={keys}><dt>{keys}</dt><dd>{action}</dd></div>
              ))}
            </dl>
          </section>
        </div>
      ) : null}
    </>
  );
}
