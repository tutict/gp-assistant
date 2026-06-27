import { useCallback } from "react";
import type { WatchlistItem } from "../../types";
import { formatPrice, formatPercent } from "../../lib/format";

interface WatchlistPanelProps {
  items: WatchlistItem[];
  onChange: (items: WatchlistItem[]) => void;
}

export function WatchlistPanel({ items, onChange }: WatchlistPanelProps) {
  const remove = useCallback((code: string) => {
    onChange(items.filter((w) => w.code !== code));
  }, [items, onChange]);

  const clear = useCallback(() => {
    if (confirm("确认清空自选股列表？")) {
      onChange([]);
    }
  }, [onChange]);

  return (
    <div className="watchlist-panel">
      <div className="watchlist-header">
        <h3>自选股 <span className="watchlist-count">({items.length})</span></h3>
        {items.length > 0 && (
          <button type="button" className="clear-btn" onClick={clear}>清空</button>
        )}
      </div>

      {items.length === 0 ? (
        <div className="watchlist-empty">
          <span>暂无关注股票</span>
        </div>
      ) : (
        <ul className="watchlist-items">
          {items.map((item) => (
            <li key={item.code} className="watchlist-item">
              <span className="watchlist-code">{item.code}</span>
              <span className="watchlist-name">{item.name || "—"}</span>
              <button
                type="button"
                className="watchlist-remove"
                onClick={() => remove(item.code)}
                aria-label="移除"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <line x1="6" y1="6" x2="18" y2="18" />
                  <line x1="18" y1="6" x2="6" y2="18" />
                </svg>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
