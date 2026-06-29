import type { StockRowView, WatchlistItem } from "../types";
import { formatNumber, formatPrice, formatRatioPercent, reasonLabel } from "../lib/format";

interface StockListProps {
  items: StockRowView[];
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
}

export function StockList({ items, watchlist, onToggleWatchlist, onObserveStock }: StockListProps) {
  if (!items.length) return <div className="empty-list">暂无匹配股票</div>;

  return (
    <div className="quote-table">
      <div className="quote-table-head">
        <span>名称</span>
        <span>评分</span>
        <span>PE</span>
        <span>PB</span>
        <span>ROE</span>
        <span>操作</span>
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
                  <small>{scoreLabel(item.scoreLabel)}</small>
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
                  <span>价格 {formatPrice(item.price)}</span>
                  <span className={tone}>涨跌 {item.change_pct != null ? formatRatioPercent(item.change_pct) : "--"}</span>
                  <span>ROE {formatRatioPercent(item.roe)}</span>
                  <span>市值 {formatNumber(item.market_cap_billion)}</span>
                </div>
                <div className="row-button-group">
                  <button
                    type="button"
                    className={`stock-row-action watchlist-action ${inWatchlist ? "saved" : ""}`}
                    onClick={() => onToggleWatchlist(item)}
                    aria-pressed={inWatchlist}
                  >
                    {inWatchlist ? "已收藏" : "收藏"}
                  </button>
                  {onObserveStock && (
                    <button type="button" className="stock-row-action observe-action" onClick={() => onObserveStock(item.code)}>
                      观察
                    </button>
                  )}
                </div>
              </div>

              {item.concept && <div className="tag-row"><span>{item.concept}</span></div>}
              {item.reasons?.length ? (
                <div className="tag-row">{item.reasons.slice(0, 5).map((reason) => <span key={reason}>{reasonLabel(reason)}</span>)}</div>
              ) : null}
              {item.explanation?.basis?.length ? (
                <details className="selection-explain">
                  <summary>入选依据</summary>
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

function scoreLabel(label?: string): string {
  const labels: Record<string, string> = {
    score: "评分",
    final: "综合",
    trend: "趋势",
  };
  return labels[String(label || "score")] || String(label);
}
