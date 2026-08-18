import { memo, useMemo } from "react";
import type { ScoreContribution, StockRowView, WatchlistItem } from "../types";
import { formatNumber, formatPrice, formatRatioPercent, reasonLabel } from "../lib/format";

interface StockListProps {
  items: StockRowView[];
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
}

export const StockList = memo(function StockList({ items, watchlist, onToggleWatchlist, onObserveStock }: StockListProps) {
  const savedCodes = useMemo(() => new Set(watchlist.map((item) => item.code)), [watchlist]);
  const sortedItems = useMemo(() => sortStocksByDisplayScore(items), [items]);

  if (!sortedItems.length) return <div className="empty-list">暂无匹配股票</div>;

  return (
    <div className="quote-table">
      <div className="stock-list">
        {sortedItems.map((item, i) => {
          const inWatchlist = savedCodes.has(item.code);
          const tone = item.change_pct != null && item.change_pct > 0 ? "rise" : item.change_pct != null && item.change_pct < 0 ? "fall" : "neutral";
          return (
            <article key={`${item.code}-${i}`} className={`stock-row ${tone}`}>
              <header className="stock-row-head">
                <div className="stock-title">
                  <strong>{item.name || item.code}</strong>
                  <span>{item.code} {item.industry || ""}</span>
                </div>
                <div className="stock-current-price">
                  <strong>{formatPrice(item.price)}</strong>
                  <span>当前股价</span>
                </div>
              </header>

              <div className="stock-insight-board" aria-label="估值与评分概览">
                <div className="stock-grid stock-valuation-grid" aria-label="估值指标">
                  <div className="quote-number quote-pe">
                    <strong>{formatNumber(item.pe)}</strong>
                    <span>市盈率</span>
                  </div>
                  <div className="quote-number quote-eps">
                    <strong>{formatNumber(item.eps)}</strong>
                    <span>每股收益</span>
                  </div>
                  <div className="quote-number quote-pb">
                    <strong>{formatNumber(item.pb)}</strong>
                    <span>市净率</span>
                  </div>
                </div>

                <div className="score-strip" aria-label="评分概览">
                  <div className="score-strip-metrics">
                    <ScoreChip label="质量" value={item.qualityScore} />
                    <ScoreChip label="趋势" value={item.trendScore} />
                    <ScoreChip label="风险" value={item.riskScore} inverted />
                  </div>
                  <div className="score-strip-primary">
                    <span>综合评分</span>
                    <strong>{formatNumber(item.balancedScore ?? item.score)}</strong>
                  </div>
                </div>
              </div>
              <ScoreSummary item={item} />

              <div className="row-actions">
                <div className="stock-meta">
                  <span className={tone}>涨跌 {item.change_pct != null ? formatRatioPercent(item.change_pct) : "--"}</span>
                  <span>净资产收益率 {formatRatioPercent(item.roe)}</span>
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
              {item.explanation?.basis?.length ? (
                <details className="selection-explain compact-selection-explain">
                  <summary>查看入选依据</summary>
                  {item.explanation.basis.slice(0, 3).map((line) => <p key={line}>{line}</p>)}
                </details>
              ) : null}
            </article>
          );
        })}
      </div>
    </div>
  );
});

const ScoreChip = memo(function ScoreChip({ label, value, inverted = false }: { label: string; value?: number | null; inverted?: boolean }) {
  const score = typeof value === "number" && Number.isFinite(value) ? value : null;
  const level = score == null ? "neutral" : inverted ? (score >= 70 ? "bad" : score >= 40 ? "watch" : "good") : score >= 70 ? "good" : score >= 50 ? "watch" : "neutral";
  return (
    <span className={`score-chip ${level}`}>
      <em>{label}</em>
      <b>{score == null ? "--" : formatNumber(score)}</b>
    </span>
  );
});

const ScoreSummary = memo(function ScoreSummary({ item }: { item: StockRowView }) {
  const positiveTags = compactStrings([...(item.reasonTags || []), ...(item.reasons || []).map(reasonLabel)]).slice(0, 2);
  const riskTags = compactStrings(item.riskTags || []).slice(0, 2);
  const periods = compactStrings(item.suitablePeriods || []).slice(0, 2);
  const strongest = strongestContribution(item.scoreBreakdown || []);
  const highlights = positiveTags.length ? positiveTags.join("、") : strongest ? `${strongest.label}贡献较高` : "综合评分较为均衡";
  const riskText = riskTags.length ? `需关注${riskTags.join("、")}` : "暂未出现突出的扣分标签";
  return (
    <p className="stock-score-summary">
      {highlights}，{riskText}，
      {periods.length ? `适合${periods.join("、")}观察` : <><span className="text-no-wrap">建议</span>结合短中周期继续跟踪</>}
      ；仅供选股研究，不构成<span className="text-no-wrap">投资建议</span>。
    </p>
  );
});

function strongestContribution(parts: ScoreContribution[]): ScoreContribution | null {
  return parts
    .filter((part) => typeof part.contribution === "number" && Number.isFinite(part.contribution))
    .sort((a, b) => Number(b.contribution) - Number(a.contribution))[0] || null;
}

function compactStrings(values: unknown[]): string[] {
  return values.map((value) => String(value || "").trim()).filter(Boolean);
}

export function sortStocksByDisplayScore(items: StockRowView[]): StockRowView[] {
  return [...items].sort((a, b) => stockDisplayScore(b) - stockDisplayScore(a));
}

function stockDisplayScore(item: StockRowView): number {
  const score = item.balancedScore ?? item.score;
  return typeof score === "number" && Number.isFinite(score) ? score : Number.NEGATIVE_INFINITY;
}
