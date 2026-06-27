import type { StockRowView, WatchlistItem } from "../types";
import { formatNumber, formatPercent, formatPrice } from "../lib/format";

interface StockListProps {
  items: StockRowView[];
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
}

export function StockList({ items, watchlist, onToggleWatchlist, onObserveStock }: StockListProps) {
  if (!items.length) return <div className="empty-list">No matching stocks</div>;

  return (
    <div className="quote-table">
      <div className="quote-table-head">
        <span>Name</span>
        <span>Score</span>
        <span>PE</span>
        <span>PB</span>
        <span>ROE</span>
        <span>Actions</span>
      </div>
      <div className="stock-list">
        {items.map((item, i) => {
          const inWatchlist = watchlist.some((w) => w.code === item.code);
          const tone = item.change_pct != null && item.change_pct > 0 ? "rise" : item.change_pct != null && item.change_pct < 0 ? "fall" : "neutral";
          return (
            <article key={`${item.code}-${i}`} className={`stock-row ${tone}`}>
              <div className="stock-grid">
                <div className="stock-title">
                  <strong>{item.name || item.code}</strong>
                  <span>{item.code} {item.industry || ""}</span>
                </div>
                <div className="score-badge">
                  <small>{item.scoreLabel || "score"}</small>
                  <b>{formatNumber(item.score)}</b>
                </div>
                <div className="quote-number">
                  <strong>{formatNumber(item.pe)}</strong>
                  <span>PE</span>
                </div>
                <div className="quote-number">
                  <strong>{formatNumber(item.pb)}</strong>
                  <span>PB</span>
                </div>
              </div>

              <div className="row-actions">
                <div className="stock-meta">
                  <span>Price {formatPrice(item.price)}</span>
                  <span className={tone}>Change {item.change_pct != null ? formatPercent(item.change_pct) : "--"}</span>
                  <span>ROE {formatPercent(item.roe)}</span>
                  <span>Market cap {formatNumber(item.market_cap_billion)}</span>
                </div>
                <div className="row-button-group">
                  <button
                    type="button"
                    className={`stock-row-action watchlist-action ${inWatchlist ? "saved" : ""}`}
                    onClick={() => onToggleWatchlist(item)}
                    aria-pressed={inWatchlist}
                  >
                    {inWatchlist ? "Saved" : "Save"}
                  </button>
                  {onObserveStock && (
                    <button type="button" className="stock-row-action observe-action" onClick={() => onObserveStock(item.code)}>
                      Observe
                    </button>
                  )}
                </div>
              </div>

              {item.concept && <div className="tag-row"><span>{item.concept}</span></div>}
              {item.reasons?.length ? (
                <div className="tag-row">{item.reasons.slice(0, 5).map((reason) => <span key={reason}>{reason}</span>)}</div>
              ) : null}
              {item.explanation?.basis?.length ? (
                <details className="selection-explain">
                  <summary>Explanation</summary>
                  {item.explanation.basis.map((line) => <p key={line}>{line}</p>)}
                </details>
              ) : null}
            </article>
          );
        })}
      </div>
    </div>
  );
}
