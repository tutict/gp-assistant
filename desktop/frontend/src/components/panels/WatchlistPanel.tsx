import { ChartNoAxesCombined, ChevronRight, Newspaper, Trash2, X } from "lucide-react";
import { useCallback, useId, useState, useSyncExternalStore } from "react";
import { getWatchlistPersistenceSnapshot, retryWatchlistPersistence, subscribeWatchlistPersistence } from "../../lib/watchlistStore";
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

function defaultWatchlistExpanded(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return true;
  return !window.matchMedia("(max-width: 768px)").matches;
}

export function WatchlistPanel({ items, onChange, onObserve, onNews, onBacktest }: WatchlistPanelProps) {
  const [expanded, setExpanded] = useState(defaultWatchlistExpanded);
  const bodyId = useId();
  const persistence = useSyncExternalStore(subscribeWatchlistPersistence, getWatchlistPersistenceSnapshot, getWatchlistPersistenceSnapshot);
  const [removed, setRemoved] = useState<Array<{ item: WatchlistItem; index: number }>>([]);
  const remove = useCallback((code: string) => {
    const index = items.findIndex((item) => item.code === code);
    if (index < 0) return;
    const item = items[index];
    setRemoved((previous) => [...previous.filter((entry) => entry.item.code !== code), { item, index }]);
    onChange(items.filter((item) => item.code !== code));
  }, [items, onChange]);

  const undo = (entry: { item: WatchlistItem; index: number }) => {
    // Restore only this item; never roll the whole list back over later edits.
    if (!items.some((item) => item.code === entry.item.code)) {
      const next = [...items];
      next.splice(Math.min(entry.index, next.length), 0, entry.item);
      onChange(next);
    }
    setRemoved((previous) => previous.filter((item) => item.item.code !== entry.item.code));
  };

  const clear = useCallback(() => {
    if (confirm("确认清空自选股列表？")) {
      onChange([]);
    }
  }, [onChange]);

  return (
    <section className="watchlist-panel" aria-label="自选股">
      <header className="watchlist-header">
        <div className="watchlist-heading">
          <h3>自选股</h3>
          <span className="watchlist-count"><strong>{items.length}</strong> 只</span>
          {items.length > 0 && (
            <IconButton
              className="watchlist-collapse"
              onClick={() => setExpanded((current) => !current)}
              label={expanded ? "收起自选股" : "展开自选股"}
              aria-controls={bodyId}
              aria-expanded={expanded}
              icon={<ChevronRight size={15} aria-hidden="true" />}
            />
          )}
        </div>
        {items.length > 0 && (
          <div className="watchlist-header-actions">
            {onBacktest && (
              <button
                type="button"
                className="watchlist-backtest"
                onClick={onBacktest}
                aria-label={`用 ${items.length} 只自选股回测`}
                title="用自选股组合回测"
              >
                <ChartNoAxesCombined size={15} aria-hidden="true" />
                <span>组合回测</span>
              </button>
            )}
            <IconButton
              className="watchlist-clear"
              onClick={clear}
              label="清空自选股"
              icon={<Trash2 size={15} aria-hidden="true" />}
            />
          </div>
        )}
      </header>

      <div className="watchlist-header watchlist-count" role="status" aria-live="polite" aria-atomic="true">
        <span>
          {persistence.status === "saving" ? "保存中…" : persistence.status === "error" ? "未保存，待保存操作已保留。" : persistence.storage === "local" ? "已保存到浏览器（非桌面数据库）" : "已保存"}
        </span>
        {persistence.status === "error" && (
          <button type="button" className="watchlist-backtest" aria-label="重试保存自选股" title={persistence.error || undefined} onClick={() => { void retryWatchlistPersistence(); }}>
            重试保存
          </button>
        )}
      </div>
      {removed.length > 0 && (
        <ul className="watchlist-items" aria-label="最近移除的自选股">
          {removed.map((entry) => (
            <li className="watchlist-item" key={entry.item.code}>
              <span>{entry.item.name || entry.item.code} 已移除</span>
              <button type="button" className="watchlist-backtest" aria-label={`撤销移除 ${entry.item.name || entry.item.code}`} onClick={() => undo(entry)}>
                撤销
              </button>
            </li>
          ))}
        </ul>
      )}

      <div
        id={bodyId}
        className="watchlist-body"
        hidden={items.length > 0 && !expanded}
      >
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
                  aria-label={`观察 ${item.name || item.code}`}
                >
                  <span className="watchlist-code">{item.code}</span>
                  <span className={`watchlist-name ${item.name ? "" : "missing"}`.trim()}>{item.name || "名称待同步"}</span>
                </button>
                {onNews && (
                  <IconButton
                    className="watchlist-news-action"
                    onClick={() => onNews(item.code)}
                    label={`查看 ${item.name || item.code} 的消息`}
                    icon={<Newspaper size={15} aria-hidden="true" />}
                  />
                )}
                <IconButton
                  className="watchlist-remove"
                  onClick={() => remove(item.code)}
                  label={`移除 ${item.name || item.code}`}
                  icon={<X size={16} aria-hidden="true" />}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
