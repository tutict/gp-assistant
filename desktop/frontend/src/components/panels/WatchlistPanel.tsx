import { Trash2 } from "lucide-react";
import { useCallback } from "react";
import type { WatchlistItem } from "../../types";
import { IconButton } from "../ui/IconButton";
import { PanelFeedback } from "../ui/PanelFeedback";

interface WatchlistPanelProps {
  items: WatchlistItem[];
  onChange: (items: WatchlistItem[]) => void;
  onObserve?: (code: string) => void;
  onNews?: (code: string) => void;
  onBacktest?: () => void;
}

export function WatchlistPanel({ items, onChange, onObserve, onNews, onBacktest }: WatchlistPanelProps) {
  const remove = useCallback((code: string) => {
    onChange(items.filter((item) => item.code !== code));
  }, [items, onChange]);

  const clear = useCallback(() => {
    if (confirm("确认清空自选股列表？")) {
      onChange([]);
    }
  }, [onChange]);

  return (
    <section className="watchlist-panel" aria-label="自选股">
      <header className="watchlist-header">
        <div>
          <h3>自选股</h3>
          <span className="watchlist-count">{items.length} 只</span>
        </div>
        {items.length > 0 && (
          <div className="watchlist-header-actions">
            {onBacktest && <button type="button" className="clear-btn watchlist-backtest" onClick={onBacktest}>回测</button>}
            <button type="button" className="clear-btn" onClick={clear}>清空</button>
          </div>
        )}
      </header>

      {items.length === 0 ? (
        <PanelFeedback kind="empty" description="收藏股票后会显示在这里。" />
      ) : (
        <ul className="watchlist-items">
          {items.map((item) => (
            <li key={item.code} className="watchlist-item">
              <button
                type="button"
                className="watchlist-observe"
                onClick={() => onObserve?.(item.code)}
                disabled={!onObserve}
                title={`观察 ${item.name || item.code}`}
              >
                <span className="watchlist-code">{item.code}</span>
                <span className="watchlist-name">{item.name || "暂无名称"}</span>
              </button>
              {onNews && (
                <button type="button" className="watchlist-news" onClick={() => onNews(item.code)}>消息</button>
              )}
              <IconButton
                className="watchlist-remove"
                onClick={() => remove(item.code)}
                label={`移除 ${item.name || item.code}`}
                icon={<Trash2 size={15} aria-hidden="true" />}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}