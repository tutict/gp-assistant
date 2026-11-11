import type { HkObserveResult } from "../../types";
import { finiteHkNumber, formatHkAmount, formatHkPrice, formatHkShares } from "../../lib/hkFormat";
import { formatObserveTime } from "./ObservePanel";

export function HkStockInformation({ result }: { result: HkObserveResult }) {
  const quote = result.quote;
  const boardLot = result.security.board_lot;
  const hasBoardLot = boardLot != null && Number.isSafeInteger(boardLot) && boardLot > 0;
  const price = finiteHkNumber(quote?.price);
  const lotAmount = hasBoardLot && price != null && price > 0 ? price * boardLot : undefined;
  const quoteError = [...new Set([result.refresh_error, result.quote_error, quote?.last_error].filter(Boolean))].join(" · ");
  const metrics = [
    { label: "昨收", value: formatHkPrice(quote?.previous_close) },
    { label: "今开", value: formatHkPrice(quote?.open) },
    { label: "最高", value: formatHkPrice(quote?.high) },
    { label: "最低", value: formatHkPrice(quote?.low) },
    { label: "成交量", value: formatHkShares(quote?.volume_shares) },
    { label: "成交额", value: formatHkAmount(quote?.turnover_hkd, "HKD") },
    { label: "每手股数", value: hasBoardLot ? formatHkShares(boardLot) : "暂无" },
    { label: "一手金额（参考）", value: formatHkAmount(lotAmount, quote?.currency || result.security.currency) },
  ];

  return <section className="observe-fundamental-snapshot hk-stock-information" aria-label="港股交易信息">
    <header>
      <div>
        <h3>交易信息</h3>
        <p className="data-provenance">
          {quote ? `${quote.source} · 报价 ${formatObserveTime(quote.quote_time)}` : "暂无报价，交易单位来自证券目录。"}
          {quote ? ` · ${quote.currency}` : ""}
        </p>
      </div>
    </header>
    <dl className="observe-fundamental-grid">
      {metrics.map(metric => <div key={metric.label} className={metric.value === "暂无" ? "is-missing" : undefined}>
        <dt>{metric.label}</dt><dd>{metric.value}</dd>
      </div>)}
    </dl>
    <p className="data-provenance hk-stock-information-meta">
      {quote && <>报价缓存更新 {formatObserveTime(quote.fetched_at || result.quote_updated_at)} · </>}
      日 K 截至 {result.history.at(-1)?.date || "暂无"} · {result.history.length} 条
      {quoteError ? <span className="hk-financial-error" role="status"> · 行情更新未完成：{quoteError}</span> : null}
    </p>
  </section>;
}
