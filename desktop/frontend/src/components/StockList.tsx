import { Fragment, memo, useCallback, useId, useMemo, useRef, useState } from "react";
import type { StockRowView, WatchlistItem } from "../types";
import { formatNumber, formatPrice, formatRatioPercent, formatSignedPercent } from "../lib/format";
import { selectionEvidence, type SelectionEvidence } from "../lib/selectionEvidence";
import { useMediaQuery } from "../hooks/useMediaQuery";

interface StockListProps {
  items: StockRowView[];
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onObserveStock?: (code: string) => void;
  onNewsStock?: (code: string) => void;
}

export const StockList = memo(function StockList({ items, watchlist, onToggleWatchlist, onObserveStock, onNewsStock }: StockListProps) {
  const mobile = useMediaQuery("(max-width: 768px)");
  const savedCodes = useMemo(() => new Set(watchlist.map(item => item.code)), [watchlist]);
  const sortedItems = useMemo(() => sortStocksByDisplayScore(items), [items]);
  const [expanded, setExpanded] = useState<string | null>(null);
  const actions = useRef({onToggleWatchlist, onObserveStock, onNewsStock});
  actions.current = {onToggleWatchlist, onObserveStock, onNewsStock};
  const toggleWatchlist = useCallback((item: StockRowView) => actions.current.onToggleWatchlist(item), []);
  const observeStock = useCallback((code: string) => actions.current.onObserveStock?.(code), []);
  const newsStock = useCallback((code: string) => actions.current.onNewsStock?.(code), []);
  const id = useId();
  if (!sortedItems.length) return <div className="empty-list">暂无匹配股票</div>;
  const rows = sortedItems.map((item, index) => <StockEntry key={`${item.code}-${index}`} item={item} mobile={mobile}
    saved={savedCodes.has(item.code)} expanded={expanded === item.code} detailId={`${id}-${index}`}
    onExpand={setExpanded} onToggleWatchlist={toggleWatchlist} onObserveStock={onObserveStock ? observeStock : undefined} onNewsStock={onNewsStock ? newsStock : undefined} />);
  return <div className="quote-table">
    {mobile ? <div className="stock-list stock-mobile-list">{rows}</div> : <table className="stock-comparison-table">
      <caption className="visually-hidden">选股结果，按综合分从高到低排列</caption>
      <thead><tr><th scope="col">股票／代码</th><th scope="col">现价</th><th scope="col">涨跌幅</th><th scope="col">PE</th><th scope="col">PB</th><th scope="col" className="stock-eps-column">EPS</th><th scope="col">综合分</th><th scope="col">操作</th></tr></thead>
      <tbody>{rows}</tbody>
    </table>}
    <p className="stock-result-boundary">仅供选股研究，不构成投资建议。</p>
  </div>;
});

interface EntryProps {
  item: StockRowView; mobile: boolean; saved: boolean; expanded: boolean; detailId: string;
  onExpand: (code: string | null) => void;
  onToggleWatchlist: StockListProps["onToggleWatchlist"];
  onObserveStock: StockListProps["onObserveStock"];
  onNewsStock: StockListProps["onNewsStock"];
}

const StockEntry = memo(function StockEntry({item, mobile, saved, expanded, detailId, onExpand, onToggleWatchlist, onObserveStock, onNewsStock}: EntryProps) {
  const tone = Number.isFinite(item.change_pct) ? Number(item.change_pct)>0 ? "rise" : Number(item.change_pct)<0 ? "fall" : "neutral" : "neutral";
  const change = typeof item.change_pct === "number" ? formatSignedPercent(item.change_pct * 100) : "—";
  const evidence = useMemo(() => selectionEvidence(item), [item]);
  const riskWarning = evidence.riskSummary.includes("风险预警");
  const reason = evidence.historyBehindQuote ? "历史指标待更新" : riskWarning ? "风险预警" : evidence.supports[0];
  const identity = <div className="stock-title"><strong>{item.name || item.code}</strong><span>{item.code} {item.industry}</span>{reason ? <small className="stock-selection-reason">{evidence.historyBehindQuote || riskWarning ? "状态" : "入选"}：{reason}</small> : null}{item.industrySampleCount !== undefined ? <small className="stock-selection-reason">行业样本：{item.industrySampleCount === 0 ? "未核验，使用中性分位" : item.industrySampleCount}</small> : null}{Boolean(evidence.riskSummary.length) && <small className="stock-risk-summary">关注：{evidence.riskSummary.join("、")}</small>}</div>;
  const actions = <div className="row-button-group">
    <button type="button" className={`stock-row-action watchlist-action ${saved ? "saved" : ""}`} aria-pressed={saved} onClick={() => onToggleWatchlist(item)}>{saved ? "已收藏" : "收藏"}</button>
    {onObserveStock && <button type="button" className="stock-row-action observe-action" onClick={()=>onObserveStock(item.code)}>观察</button>}
    {onNewsStock && <button type="button" className="stock-row-action" onClick={()=>onNewsStock(item.code)}>消息</button>}
    <button type="button" className="stock-row-action stock-details-toggle" aria-expanded={expanded} aria-controls={expanded ? detailId : undefined} onClick={()=>onExpand(expanded ? null : item.code)}>{expanded ? "收起依据" : "展开依据"}</button>
  </div>;
  const details = expanded ? <StockDetails item={item} evidence={evidence} id={detailId} /> : null;
  if (mobile) return <article className="stock-row">
    <header className="stock-row-head">{identity}<div className={`stock-current-price ${tone}`}><span>现价</span><strong>{formatPrice(item.price)}</strong><span>涨跌幅 {change}</span></div></header>
    <dl className="stock-mobile-metrics"><div><dt>PE</dt><dd>{formatNumber(item.pe)}</dd></div><div><dt>PB</dt><dd>{formatNumber(item.pb)}</dd></div><div><dt>综合分</dt><dd>{formatNumber(displayStockScore(item))}</dd></div></dl>
    {actions}{details}
  </article>;
  return <Fragment><tr className="stock-row"><th scope="row">{identity}</th><td className={tone}>{formatPrice(item.price)}</td><td className={tone}>{change}</td><td>{formatNumber(item.pe)}</td><td>{formatNumber(item.pb)}</td><td className="stock-eps-column">{formatNumber(item.eps)}</td><td>{formatNumber(displayStockScore(item))}</td><td>{actions}</td></tr>
    {expanded && <tr className="stock-detail-row"><td colSpan={8}>{details}</td></tr>}
  </Fragment>;
});

function StockDetails({item,evidence,id}: {item:StockRowView;evidence:SelectionEvidence;id:string}) {
  return <section className="stock-details" id={id} aria-label={`${item.name || item.code}入选依据`}>
    <dl className="stock-detail-metrics">{[["质量分",item.qualityScore],["趋势分",item.trendScore],["风险分（越高风险越大）",item.riskScore],["EPS",item.eps]].map(([label,value])=><div key={String(label)}><dt>{label}</dt><dd>{formatNumber(value)}</dd></div>)}</dl>
    {item.concept && <p>概念：{item.concept}</p>}
    <p>净资产收益率：{item.roe == null ? "—" : formatRatioPercent(item.roe)} · 市值：{formatNumber(item.market_cap_billion)} 亿</p>
    {!!item.suitablePeriods?.length && <p>观察周期：{item.suitablePeriods.join("、")}</p>}
    {evidence.context && <p>{evidence.context}</p>}
    {!!evidence.scoreBreakdown.length && <p>评分贡献：{evidence.scoreBreakdown.map(part => `${part.label} ${formatNumber(part.contribution)}`).join(" · ")}</p>}
    {evidence.supports.length ? <><p>{evidence.historyBehindQuote ? "历史支持依据" : "支持依据"}</p><ul aria-label="支持依据">{evidence.supports.map((line,index)=><li key={index}>{line}</li>)}</ul></> : <p>暂无详细入选依据，请结合公告与财务数据继续核查。</p>}
    {!!evidence.risks.length && <><p className="stock-risk-summary">风险提示</p><ul aria-label="风险提示">{evidence.risks.map((line,index)=><li key={index}>{line}</li>)}</ul></>}
    {!!evidence.verification.length && <p>后续核验：{evidence.verification.join(" ")}</p>}
  </section>;
}

export function displayStockScore(item: StockRowView): number | undefined {
  const score=item.balancedScore ?? item.score;
  return typeof score === "number" && Number.isFinite(score) ? score : undefined;
}
export function sortStocksByDisplayScore(items: StockRowView[]): StockRowView[] {
  return [...items].sort((a,b)=>(displayStockScore(b) ?? -Infinity)-(displayStockScore(a) ?? -Infinity));
}
