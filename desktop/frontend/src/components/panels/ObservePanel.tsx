import { useCallback, useEffect, useState } from "react";
import type { CapitalEvidenceItem, CapitalEvidenceResult, CapitalEvidenceSection, FinancialIndicatorItem, FinancialIndicatorSection, ObserveResult, StockItem, TrendIndicatorPoint, TrendIndicatorSignal, WatchlistItem } from "../../types";
import { computeKdj, toDailyBars } from "../../lib/kline";
import { calculateObserveQuant } from "../../lib/observeQuant";
import type { ObserveQuantConclusion } from "../../lib/observeQuant";
import { buildFundamentalSnapshotData } from "../../lib/fundamentalSnapshot";
import { buildMainFundFlowView, isLocalFundFlowProxy } from "../../lib/mainFundFlow";
import { buildSeatBehaviorViews } from "../../lib/seatBehavior";
import { getJson } from "../../lib/tauri";
import { CollapsibleNotes } from "../CollapsibleNotes";
import { RawJson } from "../RawJson";
import { StockCodeInput } from "../StockCodeInput";
import { TrendCharts } from "../observe/ObserveCharts";
import { PanelFeedback } from "../ui/PanelFeedback";
import {
  currentSystemDateInputValue,
  formatNumber,
  formatPrice,
  formatRatioPercent,
  metricOrMissing,
  normalizeStockCode,
  reasonLabel,
} from "../../lib/format";

interface ObservePanelProps {
  watchlist: WatchlistItem[];
  onWatchlistChange: (items: WatchlistItem[]) => void;
  initialCode?: string;
  initialCodeRequestId?: number;
  mobileRuntime?: boolean;
}

const OBSERVE_FULL_HISTORY_START = "1990-01-01";
const OBSERVE_FULL_HISTORY_LIMIT = "10000";

export function ObservePanel({
  initialCode,
  initialCodeRequestId = 0,
  watchlist,
  onWatchlistChange,
  mobileRuntime = false,
}: ObservePanelProps) {
  const [code, setCode] = useState(initialCode || "");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<ObserveResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (initialCode) setCode(initialCode);
  }, [initialCode, initialCodeRequestId]);

  const toggleObservedWatchlist = useCallback(() => {
    const stock = result?.stock;
    const normalizedCode = normalizeStockCode(stock?.code || code);
    if (!normalizedCode) return;
    const exists = watchlist.some((item) => normalizeStockCode(item.code) === normalizedCode);
    if (exists) {
      onWatchlistChange(watchlist.filter((item) => normalizeStockCode(item.code) !== normalizedCode));
      return;
    }
    onWatchlistChange([
      {
        code: normalizedCode,
        name: stock?.name || normalizedCode,
        industry: stock?.industry,
        added_at: new Date().toISOString(),
        source: "observe",
      },
      ...watchlist,
    ]);
  }, [code, onWatchlistChange, result?.stock, watchlist]);

  const observedCode = normalizeStockCode(result?.stock?.code || code);
  const observedInWatchlist = Boolean(observedCode && watchlist.some((item) => normalizeStockCode(item.code) === observedCode));

  const runObserve = useCallback(async () => {
    const normalizedCode = normalizeStockCode(code);
    if (!normalizedCode) {
      setError("请输入有效股票代码。");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const query = new URLSearchParams({
        start_date: OBSERVE_FULL_HISTORY_START.replace(/-/g, ""),
        end_date: currentSystemDateInputValue().replace(/-/g, ""),
        series_limit: OBSERVE_FULL_HISTORY_LIMIT,
        include_order_book: "false",
        include_chip_distribution: "true",
      });
      const data = await getJson<ObserveResult>(`/api/observe/${encodeURIComponent(normalizedCode)}?${query}`);
      setResult(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, [code]);

  return (
    <div className="panel-container observe-panel-container">
      <div className="panel-controls observe-panel-controls">
        <div className="form-row inline stock-code-row observe-code-row">
          <label htmlFor="observeCode">股票代码</label>
          <StockCodeInput
            id="observeCode"
            value={code}
            onChange={setCode}
            placeholder="输入股票代码或名称"
            resolveBareCode={!mobileRuntime}
          />
        </div>
        <button type="button" className="run-btn observe-run-btn" onClick={runObserve} disabled={loading}>{loading ? "观察中..." : "开始观察"}</button>
      </div>

      <div className="panel-result observe-panel-result">
        {error && <PanelFeedback kind="error" title="观察失败" description={error} />}
        {loading && !result && !error && <PanelFeedback kind="loading" description="正在加载行情、财务和趋势数据..." />}
        {result && !loading && <ObserveResultView result={result} inWatchlist={observedInWatchlist} onToggleWatchlist={toggleObservedWatchlist} />}
        {!result && !loading && !error && <PanelFeedback kind="empty" description="输入股票代码后开始观察。" />}
      </div>
    </div>
  );
}

export function ObserveResultView({ result, inWatchlist = false, onToggleWatchlist }: { result: ObserveResult; inWatchlist?: boolean; onToggleWatchlist?: () => void }) {
  const stock = result.stock || { code: "", name: "" };
  const trend = result.trend;
  const signal = trend?.signal;
  const series = trend?.series || [];
  const financial = result.financial_indicators;
  const capital = result.capital_evidence;

  return (
    <div className="observe-result">
      <section className="observe-overview">
        <header className="observe-overview-header">
          <div><h3>{stock.name || stock.code}</h3><p>{stock.code} {stock.industry || ""}</p></div>
          <div className="observe-overview-actions">
            {signal?.status && <span className={`state-pill ${signalStatusTone(signal.status)}`}>{signalStatusLabel(signal.status)}</span>}
            {onToggleWatchlist && (
              <button type="button" className={`stock-row-action watchlist-action ${inWatchlist ? "saved" : ""}`} onClick={onToggleWatchlist}>
                {inWatchlist ? "已收藏" : "收藏"}
              </button>
            )}
          </div>
        </header>
        <FundamentalSnapshot stock={stock} financial={financial} />
        <ObserveTextMetrics stock={stock} signal={signal} series={series} capital={capital} financialItems={financial?.items || []} />
      </section>

      {!signal && series.length > 1 ? (
        <section className="signal-card observe-chart-card">
          <header><div><h3>行情图表</h3><p>K 线、均线、KDJ 与 MACD</p></div></header>
          <TrendCharts series={series} />
        </section>
      ) : null}

      {signal && (
        <section className="signal-card observe-chart-card">
          <header><div><h3>趋势信号</h3><p>{signal.date || ""}</p></div><span className={`state-pill ${signalStatusTone(signal.status)}`}>{signalStatusLabel(signal.status)}</span></header>
          {series.length > 1 ? <TrendCharts series={series} /> : null}
        </section>
      )}

      <CollapsibleNotes notes={[...(result.notes || []), ...(signal?.notes || [])]} />
      <RawJson result={result} />
    </div>
  );
}

function FundamentalSnapshot({ stock, financial }: { stock: StockItem; financial?: FinancialIndicatorSection | null }) {
  const snapshot = buildFundamentalSnapshotData(stock, financial);
  return (
    <section className="observe-fundamental-snapshot" aria-label="最新基本面">
      <header>
        <div>
          <span>基本面快照</span>
          <h3>最新指标</h3>
        </div>
        <div className="observe-fundamental-periods">
          <time><span>行情</span>{snapshot.quoteTime}</time>
          <time><span>财务</span>{snapshot.financialPeriod}</time>
        </div>
      </header>
      <dl className="observe-fundamental-primary" aria-label="每股收益与股本结构">
        {snapshot.primary.map((item) => (
          <div key={item.label}>
            <dt>{item.label}</dt>
            <dd>{item.value}</dd>
          </div>
        ))}
      </dl>
      <dl className="observe-fundamental-grid">
        {snapshot.details.map((item) => (
          <div key={item.label} className={item.tone || "neutral"}>
            <dt>{item.label}</dt>
            <dd>{item.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

function ObserveTextMetrics({
  stock,
  signal,
  series,
  capital,
  financialItems,
}: {
  stock: StockItem;
  signal?: TrendIndicatorSignal | null;
  series: TrendIndicatorPoint[];
  capital?: CapitalEvidenceResult | null;
  financialItems: FinancialIndicatorItem[];
}) {
  const latest = series.length ? series[series.length - 1] : null;
  const previous = series.length > 1 ? series[series.length - 2] : null;
  const kdj = kdjFromSignal(signal) || latestKdjFromSeries(series);
  const close = signal?.close ?? latest?.close ?? stock.price;
  const previousClose = signal?.previous_close ?? previous?.close;
  const change = signal?.close_change ?? numeric(close) - numeric(previousClose);
  const changePct = signal?.close_change_pct ?? ratioChange(close, previousClose);
  const financialLookup = buildFinancialLookup(financialItems);
  const capitalSummary = summarizeCapitalEvidence(capital);
  const analysisItems = buildMetricAnalysis({ stock, signal, latest, kdj, changePct, capital, capitalSummary, financialLookup });
  const specialQuant = calculateObserveQuant(series);

  const sections: Array<{ title: string; items: Array<[string, string]> }> = [
    {
      title: "特殊量化明细",
      items: specialQuant.details,
    },
    {
      title: "行情与估值",
      items: compactMetricItems([
        ["最新价", formatPrice(close)],
        ["涨跌额", formatSignedNumber(change)],
        ["涨跌幅", formatSignedPercent(changePct)],
        ["成交量", formatNumber(latest?.volume)],
        ["市盈率", formatNumber(stock.pe)],
        ["市净率", formatNumber(stock.pb)],
        ["ROE", formatRatioPercent(stock.roe)],
        ["股息率", formatRatioPercent(stock.dividend_yield)],
        ["总市值", formatMarketCap(stock)],
        ["行情时间", String(stock.quote_time || signal?.date || latest?.date || "--")],
      ]),
    },
    {
      title: "趋势与位置",
      items: compactMetricItems([
        ["信号状态", signalStatusLabel(signal?.status)],
        ["信号类型", signalTypeLabel(signal?.signal_type)],
        ["SWL", formatNumber(signal?.swl ?? latest?.swl)],
        ["SWS", formatNumber(signal?.sws ?? latest?.sws)],
        ["多空线", signal?.swl_above_sws ? "SWL 高于 SWS" : signal?.swl_above_sws === false ? "SWL 低于 SWS" : "--"],
        ["支撑位", formatPrice(signal?.support)],
        ["压力位", formatPrice(signal?.resistance)],
        ["风险标记", formatList(signal?.risk_flags, riskFlagLabel)],
      ]),
    },
    {
      title: "KDJ 与交易信号",
      items: compactMetricItems([
        ["K / D / J", kdj ? `${formatNumber(kdj.k)} / ${formatNumber(kdj.d)} / ${formatNumber(kdj.j)}` : "--"],
        ["交叉信号", signal?.kdj_golden_cross ? "金叉" : signal?.kdj_dead_cross ? "死叉" : signal ? "无明显交叉" : "--"],
        ["极端区间", signal?.kdj_overbought ? "超买" : signal?.kdj_oversold || signal?.oversold ? "超卖" : signal ? "未处于极端区间" : "--"],
        ["操作提示", [
          (signal?.red_hold ?? latest?.red_hold) ? "红持" : "",
          (signal?.cyan_watch ?? latest?.cyan_watch) ? "青观" : "",
          (signal?.short_buy ?? latest?.short_buy) ? "短买" : "",
          (signal?.white_exit ?? latest?.white_exit) ? "白离" : "",
        ].filter(Boolean).join("、") || "--"],
      ]),
    },
    {
      title: "热度与吸筹",
      items: compactMetricItems([
        ["吸筹指标", formatNumber(latest?.accumulation_index)],
        ["吸筹强度", formatNumber(latest?.accumulation_strength)],
        ["波段机会", formatNumber(latest?.swing_opportunity)],
        ["反弹信号", formatNumber(latest?.rebound_signal)],
        ["趋势热度", formatNumber(latest?.trend_heat)],
        ["量价热度", formatNumber(latest?.volume_price_heat)],
        ["异动热度", formatNumber(latest?.anomaly_heat)],
        ["人气热度", formatNumber(latest?.popularity_heat)],
      ]),
    },
    {
      title: "模型评分",
      items: compactMetricItems([
        ["量化分", formatScore(signal?.quant_score, signal?.quant_score_max)],
        ["形态分", formatScore(signal?.pattern_score, signal?.pattern_score_max)],
        ["技术层分", formatNumber(signal?.technical_score)],
        ["形态层分", formatNumber(signal?.pattern_layer_score)],
        ["质量分", formatNumber(signal?.quality_score)],
        ["形态信号", formatList(signal?.pattern_signals, patternSignalLabel)],
        ["触发原因", formatList(signal?.reasons, reasonLabel)],
      ]),
    },
    {
      title: "资金证据",
      items: compactMetricItems([
        ["资金综合分", metricOrMissing(formatNumber(capital?.composite_score))],
        ["证据置信度", capital?.confidence || "暂无"],
        ["资金流", capitalSummary.fundFlow === "--" ? "暂无" : capitalSummary.fundFlow],
        ["机构席位", capitalSummary.institution === "--" ? "暂无" : capitalSummary.institution],
        ["消息情绪", capitalSummary.news === "--" ? "暂无" : capitalSummary.news],
        ["技术资金", capitalSummary.technical === "--" ? "暂无" : capitalSummary.technical],
      ]),
    },
  ].map((section) => ({ ...section, items: section.items.filter(([, value]) => value && value !== "--" && value !== "—" && value !== "暂无") }));

  const decisionItems = selectObservationDecisions(analysisItems);
  const verdict = buildObservationVerdict(signal);
  const nextSignal = buildObservationNextSignal(signal);
  const changeTone: MetricAnalysisTone = numeric(changePct) > 0 ? "positive" : numeric(changePct) < 0 ? "negative" : "neutral";
  const keyMetrics: Array<{ label: string; value: string; tone?: MetricAnalysisTone }> = [
    { label: "最新价", value: metricOrMissing(formatPrice(close)) },
    { label: "涨跌幅", value: metricOrMissing(formatSignedPercent(changePct)), tone: changeTone },
    { label: "支撑位", value: metricOrMissing(formatPrice(signal?.support)) },
    { label: "压力位", value: metricOrMissing(formatPrice(signal?.resistance)) },
  ];
  const detailSections = sections;

  return (
    <section className="observe-text-metrics observe-decision-summary" aria-label="观察判断">
      <header className="observe-decision-heading">
        <h3>观察摘要</h3>
        <time>{String(stock.quote_time || signal?.date || latest?.date || "数据时间未知")}</time>
      </header>

      <section className={`observe-verdict ${verdict.tone}`} aria-label="当前观察结论">
        <h4>{verdict.title}</h4>
        <p>{verdict.summary}</p>
      </section>

      <div className="observe-decision-grid" aria-label="核心判断">
        {decisionItems.map((item) => (
          <article key={item.title} className={`observe-decision-item ${item.tone}`}>
            <span>{observationDecisionLabel(item.title)}</span>
            <p>{firstSentence(item.text)}</p>
          </article>
        ))}
      </div>

      <ObserveSpecialQuant conclusions={specialQuant.conclusions} />

      <section className="observe-next-signal" aria-label="下一步观察信号">
        <span>下一步看什么</span>
        <p>{nextSignal}</p>
      </section>

      <dl className="observe-key-metrics" aria-label="关键数值">
        {keyMetrics.map((item) => (
          <div key={item.label} className={item.tone || "neutral"}>
            <dt>{item.label}</dt>
            <dd>{item.value}</dd>
          </div>
        ))}
      </dl>

      <CapitalQuantPanel capital={capital} />

      <details className="observe-detail-disclosure">
        <summary><span>专业指标明细</span><small>需要时展开</small></summary>
        <div className="observe-text-metric-sections">
          {detailSections.filter((section) => section.items.length).map((section) => (
            <article key={section.title} className="observe-text-metric-section">
              <h4>{section.title}</h4>
              <div className="observe-text-metric-grid">
                {section.items.map(([label, value]) => (
                  <div key={`${section.title}-${label}`} className="observe-text-metric-item">
                    <span>{label}</span>
                    <strong>{value}</strong>
                  </div>
                ))}
              </div>
            </article>
          ))}
        </div>
      </details>

      <p className="observe-metric-analysis-disclaimer">仅供选股研究，不构成投资建议；请结合公告、财报和自身风险承受能力独立判断。</p>
    </section>
  );
}

function ObserveSpecialQuant({ conclusions }: { conclusions: ObserveQuantConclusion[] }) {
  return (
    <section className="observe-special-quant" aria-label="特殊量化结论">
      {conclusions.map((item) => (
        <article key={item.key} className={`observe-special-quant-item ${item.tone}`}>
          <span>{item.label}</span>
          <strong>{item.state}</strong>
          <p>{item.summary}</p>
        </article>
      ))}
    </section>
  );
}

function CapitalQuantPanel({ capital }: { capital?: CapitalEvidenceResult | null }) {
  const items = capital?.items || [];
  const institution = items.find((item) => item.category === "institution_lhb");
  const institutionStatus = items.find((item) => item.category === "institution_lhb_status");
  const proxy = items.find((item) => item.category === "fund_flow" && isLocalFundFlowProxy(item));
  const mainFlow = buildMainFundFlowView(capital);
  const seatBehaviors = buildSeatBehaviorViews(institution);

  const institutionTone = capitalSentimentTone(institution?.sentiment);
  const institutionMetrics: CapitalQuantMetric[] = institution ? [
    { label: "机构买入", value: capitalMetricValue(institution, "机构买入额"), tone: "positive" },
    { label: "机构卖出", value: capitalMetricValue(institution, "机构卖出额"), tone: "negative" },
    { label: "机构净买", value: capitalMetricValue(institution, "机构净买额"), tone: institutionTone },
    { label: "净买占成交", value: capitalMetricValue(institution, "净买额占成交额比"), tone: institutionTone },
  ] : [];
  const institutionMeta = institution ? [
    capitalMetricPart("机构买卖比", capitalMetricValue(institution, "机构买卖比")),
    capitalMetricPair(
      "买方/卖方机构",
      capitalMetricValue(institution, "买方机构数"),
      capitalMetricValue(institution, "卖方机构数"),
    ),
  ].filter(Boolean).join(" · ") : "";

  const proxyScore = numeric(proxy?.score);
  const proxyTone: MetricAnalysisTone = Number.isFinite(proxyScore)
    ? proxyScore >= 60 ? "positive" : proxyScore <= 40 ? "negative" : "neutral"
    : "neutral";
  const proxyDirection = capitalMetricValue(proxy, "推断方向") !== "暂无"
    ? capitalMetricValue(proxy, "推断方向")
    : proxyDirectionFromScore(proxyScore);
  const proxyMetrics: CapitalQuantMetric[] = proxy ? [
    { label: "代理分", value: Number.isFinite(proxyScore) ? `${formatNumber(proxyScore)}/100` : capitalMetricValue(proxy, "隐性资金代理分"), tone: proxyTone },
    { label: "推断方向", value: proxyDirection, tone: proxyTone },
    { label: "量价热度", value: capitalMetricValue(proxy, "量价热度") },
    { label: "吸筹强度", value: capitalMetricValue(proxy, "吸筹强度") },
  ] : [];
  const proxyMeta = proxy ? [
    capitalMetricPart("趋势热度", capitalMetricValue(proxy, "趋势热度")),
    capitalMetricPart("异动热度", capitalMetricValue(proxy, "异动热度")),
  ].filter(Boolean).join(" · ") : "";

  return (
    <section className="observe-capital-quant" aria-label="主力资金与龙虎榜公开席位">
      <header>
        <h4>资金量化证据</h4>
        <small>真实资金流 · 机构/营业部 · 本地估算</small>
      </header>
      <article className={"capital-main-flow " + mainFlow.tone + (mainFlow.available ? "" : " unavailable")}>
        <header>
          <div>
            <h5>最新交易日主力资金</h5>
            <small>{mainFlow.tradeDate} · {mainFlow.source}</small>
          </div>
          <span>{mainFlow.status}</span>
        </header>
        <CapitalQuantMetrics metrics={[
          { label: "主力净流入额", value: mainFlow.netAmount, tone: mainFlow.tone },
          { label: "主力净占比", value: mainFlow.netRatio, tone: mainFlow.tone },
          { label: "主力介入度", value: mainFlow.involvement },
        ]} />
        <p className="capital-main-flow-conclusion">
          <strong>怎么看：</strong>{mainFlow.conclusion}
        </p>
        <p className="capital-quant-note">介入度按净占比绝对值分档：低 &lt;3%，中 3%-8%，高 ≥8%；高介入只代表主力交易影响较大，不代表一定上涨。</p>
      </article>
      <div className="capital-quant-lanes">
        <article className="capital-quant-lane institution">
          <header>
            <h5>龙虎榜机构席位</h5>
            <small>{institution ? `${institution.date || "窗口内"} · ${institution.confidence || "高"}置信` : "当前无公开记录"}</small>
          </header>
          {institution ? (
            <>
              <CapitalQuantMetrics metrics={institutionMetrics} />
              {institutionMeta && (
                <details className="capital-quant-more">
                  <summary>更多席位数据</summary>
                  <p>{institutionMeta}</p>
                </details>
              )}
              <p className="capital-quant-note">仅代表龙虎榜公开机构席位，不等同于全部机构持仓。</p>
            </>
          ) : (
            <>
              <p className="capital-quant-empty">{institutionStatus?.title || "查询窗口内没有可展示的龙虎榜机构席位记录。"}</p>
              <p className="capital-quant-note">未上榜不代表机构没有买卖，只表示当前没有公开席位证据。</p>
            </>
          )}
        </article>

        {proxy && (
          <article className="capital-quant-lane proxy">
            <header>
              <h5>量价资金代理</h5>
              <small className="capital-quant-estimate">估算 · {proxy.confidence || "中"}置信</small>
            </header>
            <CapitalQuantMetrics metrics={proxyMetrics} />
            {proxyMeta && (
              <details className="capital-quant-more">
                <summary>更多代理数据</summary>
                <p>{proxyMeta}</p>
              </details>
            )}
            <p className="capital-quant-note">量价模型估算，非交易所披露数据，也不代表主力净流入。</p>
          </article>
        )}
      </div>
      {institution && (
        <article className="capital-seat-behavior">
          <header>
            <div>
              <h5>机构与活跃营业部</h5>
              <small>{institution.date || "窗口内"} · 龙虎榜公开席位 · 行为画像为推断</small>
            </div>
            <span>{seatBehaviors.length ? seatBehaviors.length + " 席" : "明细暂缺"}</span>
          </header>
          {seatBehaviors.length ? (
            <ul className="capital-seat-list">
              {seatBehaviors.map((seat) => (
                <li key={seat.key} className={seat.tone}>
                  <div className="capital-seat-name">
                    <strong>{seat.name}</strong>
                    <span>{seat.typeLabel}</span>
                  </div>
                  <div className="capital-seat-flow">
                    <strong>{seat.directionLabel}</strong>
                    <span>{seat.amountLabel}</span>
                  </div>
                  <div className="capital-seat-tactic">
                    <strong>常见形态：{seat.tactic}</strong>
                    <p>{seat.explanation}</p>
                    {seat.stats && <small>{seat.stats}</small>}
                  </div>
                </li>
              ))}
            </ul>
          ) : (
            <p className="capital-quant-empty">本次未取得公开营业部买卖明细，机构汇总数据仍可单独参考。</p>
          )}
          {institution.seat_detail_status !== "complete" && institution.seat_detail_note && (
            <p className="capital-quant-note">{institution.seat_detail_note}</p>
          )}
          <p className="capital-seat-disclaimer">营业部名称只代表公开交易通道，不能确认具体游资、账户实际控制人或是否使用量化程序。</p>
        </article>
      )}
    </section>
  );
}

type CapitalQuantMetric = {
  label: string;
  value: string;
  tone?: MetricAnalysisTone;
};

function CapitalQuantMetrics({ metrics }: { metrics: CapitalQuantMetric[] }) {
  return (
    <dl className="capital-quant-metrics">
      {metrics.map((metric) => (
        <div key={metric.label} className={metric.tone || "neutral"}>
          <dt>{metric.label}</dt>
          <dd>{metric.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function capitalMetricValue(item: CapitalEvidenceItem | null | undefined, key: string): string {
  const value = item?.metrics?.[key];
  if (value == null) return "暂无";
  const text = String(value).trim();
  return !text || text === "-" || text === "--" || text === "—" ? "暂无" : text;
}

function capitalMetricPart(label: string, value: string): string {
  return value === "暂无" ? "" : `${label} ${value}`;
}

function capitalMetricPair(label: string, left: string, right: string): string {
  return left === "暂无" && right === "暂无" ? "" : `${label} ${left}/${right}`;
}

function proxyDirectionFromScore(score: number): string {
  if (!Number.isFinite(score)) return "暂无";
  if (score >= 60) return "偏流入";
  if (score <= 40) return "偏流出";
  return "中性";
}

function capitalSentimentTone(sentiment?: string): MetricAnalysisTone {
  if (sentiment === "positive") return "positive";
  if (sentiment === "negative") return "negative";
  return "neutral";
}

type MetricAnalysisTone = "positive" | "warning" | "negative" | "neutral";

type MetricAnalysisItem = {
  title: string;
  text: string;
  tone: MetricAnalysisTone;
};

function selectObservationDecisions(items: MetricAnalysisItem[]): MetricAnalysisItem[] {
  const preferred = ["趋势位置", "行情估值", "财务资金"]
    .map((title) => items.find((item) => item.title === title))
    .filter((item): item is MetricAnalysisItem => Boolean(item));
  return preferred.length ? preferred : items.slice(0, 3);
}

function firstSentence(text: string): string {
  const sentence = String(text || "").split("。").find((item) => item.trim());
  return sentence ? `${sentence.trim()}。` : "当前证据不足。";
}

function observationDecisionLabel(title: string): string {
  const labels: Record<string, string> = {
    趋势位置: "趋势位置",
    行情估值: "价格表现",
    财务资金: "财务概览",
  };
  return labels[title] || title;
}

function buildObservationVerdict(signal?: TrendIndicatorSignal | null): { title: string; summary: string; tone: MetricAnalysisTone } {
  const tone = signalStatusTone(signal?.status);
  const risks = Array.isArray(signal?.risk_flags) ? signal.risk_flags.map(riskFlagLabel).filter(Boolean) : [];
  if (risks.length) {
    return {
      title: tone === "negative" ? "趋势承压，先等待风险收敛" : "已有方向，但风险尚未解除",
      summary: `当前识别到${risks.slice(0, 3).join("、")}。先确认风险信号是否消退，再判断趋势是否成立。`,
      tone: tone === "negative" ? "negative" : "warning",
    };
  }
  if (tone === "positive") return { title: "趋势偏强，进入确认阶段", summary: "积极结构已经出现，下一步验证压力位突破和量能延续。", tone };
  if (tone === "negative") return { title: "趋势偏弱，暂不急于下结论", summary: "价格与趋势结构仍承压，重点观察支撑是否有效。", tone };
  if (tone === "warning") return { title: "信号未确认，适合继续观察", summary: "当前有变化，但证据还不足以形成稳定方向。", tone };
  return { title: "方向尚未形成，等待关键信号", summary: "当前证据偏中性，优先观察趋势、量能和关键价位能否形成一致。", tone: "neutral" };
}

function buildObservationNextSignal(signal?: TrendIndicatorSignal | null): string {
  const support = formatPrice(signal?.support);
  const resistance = formatPrice(signal?.resistance);
  const supportText = support !== "--" ? `${support} 支撑` : "关键支撑";
  const resistanceText = resistance !== "--" ? `${resistance} 压力位` : "上方压力位";
  const tone = signalStatusTone(signal?.status);
  if (signal?.kdj_overbought) return `先看超买状态能否降温，同时确认回落时 ${supportText} 是否有效。`;
  if (signal?.kdj_dead_cross) return `先看 KDJ 死叉是否修复，并确认价格能否守住 ${supportText}。`;
  if (tone === "positive") return `关注能否放量突破 ${resistanceText}；若回落，再看 ${supportText} 是否有效。`;
  if (tone === "negative") return `关注价格能否重新站稳 ${supportText}，并等待 SWL 与 SWS 关系改善。`;
  return `等待趋势方向与成交量形成一致；向上关注 ${resistanceText}，向下关注 ${supportText}。`;
}
function buildMetricAnalysis({
  stock,
  signal,
  latest,
  kdj,
  changePct,
  capital,
  capitalSummary,
  financialLookup,
}: {
  stock: StockItem;
  signal?: TrendIndicatorSignal | null;
  latest: TrendIndicatorPoint | null;
  kdj: { k?: number | null; d?: number | null; j?: number | null } | null;
  changePct: unknown;
  capital?: CapitalEvidenceResult | null;
  capitalSummary: Record<string, string>;
  financialLookup: Map<string, string>;
}): MetricAnalysisItem[] {
  const items: MetricAnalysisItem[] = [];
  const pct = numeric(changePct);
  const pe = numeric(stock.pe);
  const pb = numeric(stock.pb);
  const roe = numeric(stock.roe);
  const quant = numeric(signal?.quant_score);
  const quantMax = numeric(signal?.quant_score_max);
  const pattern = numeric(signal?.pattern_score);
  const patternMax = numeric(signal?.pattern_score_max);
  const capitalScore = numeric(capital?.composite_score);
  const accumulation = numeric(latest?.accumulation_strength ?? latest?.accumulation_index);
  const trendHeat = numeric(latest?.trend_heat);
  const volumeHeat = numeric(latest?.volume_price_heat);
  const k = numeric(kdj?.k);
  const d = numeric(kdj?.d);
  const j = numeric(kdj?.j);

  const priceText = Number.isFinite(pct)
    ? `近端涨跌幅为 ${formatSignedPercent(pct)}，${pct > 0.03 ? "短线情绪偏强" : pct < -0.03 ? "短线承压较明显" : pct > 0 ? "价格小幅走强" : pct < 0 ? "价格小幅回落" : "价格变化不大"}。`
    : "当前缺少可用涨跌幅，价格动量需要结合图表确认。";
  const valuationBits = [
    Number.isFinite(pe) ? `PE ${formatNumber(pe)}` : "PE 暂缺",
    Number.isFinite(pb) ? `PB ${formatNumber(pb)}` : "PB 暂缺",
    Number.isFinite(roe) ? `ROE ${formatRatioPercent(roe)}` : "ROE 暂缺",
  ];
  items.push({
    title: "行情估值",
    tone: pct > 0.03 ? "positive" : pct < -0.03 ? "negative" : "neutral",
    text: `${priceText} 估值侧 ${valuationBits.join("、")}，更适合与同行业分位做横向比较。`,
  });

  const signalState = signalStatusLabel(signal?.status);
  const signalType = signalTypeLabel(signal?.signal_type);
  const trendPosition = signal?.swl_above_sws === true ? "SWL 位于 SWS 上方，趋势结构偏积极" : signal?.swl_above_sws === false ? "SWL 位于 SWS 下方，趋势结构仍需观察" : "SWL/SWS 关系暂不完整";
  const riskText = Array.isArray(signal?.risk_flags) && signal.risk_flags.length ? `风险标记包括 ${formatList(signal.risk_flags, riskFlagLabel)}。` : "暂未识别到明确风险标记。";
  items.push({
    title: "趋势位置",
    tone: signalStatusTone(signal?.status),
    text: `当前信号为${signalState}，类型为${signalType}；${trendPosition}。${riskText}`,
  });

  if (Number.isFinite(k) || Number.isFinite(d) || Number.isFinite(j) || signal?.kdj_golden_cross || signal?.kdj_dead_cross) {
    const cross = signal?.kdj_golden_cross ? "出现金叉信号" : signal?.kdj_dead_cross ? "出现死叉信号" : "暂未出现明显交叉信号";
    const zone = signal?.kdj_overbought ? "处于超买区，追高风险上升" : signal?.kdj_oversold || signal?.oversold ? "处于超卖区，反弹信号需要成交确认" : "未处于极端区间";
    items.push({
      title: "KDJ 信号",
      tone: signal?.kdj_golden_cross ? "positive" : signal?.kdj_dead_cross || signal?.kdj_overbought ? "warning" : "neutral",
      text: `K/D/J 为 ${formatNumber(k)} / ${formatNumber(d)} / ${formatNumber(j)}，${cross}，${zone}。`,
    });
  }

  if (Number.isFinite(accumulation) || Number.isFinite(trendHeat) || Number.isFinite(volumeHeat)) {
    const heatText = [
      Number.isFinite(accumulation) ? `吸筹强度 ${formatNumber(accumulation)}` : "吸筹暂缺",
      Number.isFinite(trendHeat) ? `趋势热度 ${formatNumber(trendHeat)}` : "趋势热度暂缺",
      Number.isFinite(volumeHeat) ? `量价热度 ${formatNumber(volumeHeat)}` : "量价热度暂缺",
    ].join("、");
    const tone: MetricAnalysisTone = accumulation >= 60 || trendHeat >= 60 || volumeHeat >= 60 ? "positive" : accumulation <= 20 && trendHeat <= 20 ? "warning" : "neutral";
    items.push({
      title: "热度吸筹",
      tone,
      text: `${heatText}。若热度走强但价格接近压力位，应重点观察量能延续性。`,
    });
  }

  if (Number.isFinite(quant) || Number.isFinite(pattern)) {
    const quantRatio = Number.isFinite(quant) && Number.isFinite(quantMax) && quantMax > 0 ? quant / quantMax : NaN;
    const patternRatio = Number.isFinite(pattern) && Number.isFinite(patternMax) && patternMax > 0 ? pattern / patternMax : NaN;
    const scoreTone: MetricAnalysisTone = quantRatio >= 0.65 || patternRatio >= 0.65 ? "positive" : quantRatio <= 0.35 && patternRatio <= 0.35 ? "warning" : "neutral";
    items.push({
      title: "模型评分",
      tone: scoreTone,
      text: `量化分 ${formatScore(signal?.quant_score, signal?.quant_score_max)}，形态分 ${formatScore(signal?.pattern_score, signal?.pattern_score_max)}；评分用于排序和观察，不等同于确定性结论。`,
    });
  }

  const eps = financialLookup.get("eps") || financialLookup.get("EPS");
  const profitGrowth = financialLookup.get("deducted_net_profit_growth_rate") || financialLookup.get("扣非增长");
  const capitalText = capital ? `资金综合分 ${formatNumber(capitalScore)}，证据置信度 ${capital.confidence || "--"}；资金流 ${capitalSummary.fundFlow}，机构席位 ${capitalSummary.institution}。` : "资金证据暂缺，不能把缺失数据解读为流入或流出。";
  items.push({
    title: "财务资金",
    tone: capitalScore >= 60 ? "positive" : capitalScore <= 30 ? "warning" : "neutral",
    text: `财务侧 EPS ${eps || "暂缺"}、扣非增长 ${profitGrowth || "暂缺"}。${capitalText}`,
  });

  return items;
}
function buildFinancialLookup(items: FinancialIndicatorItem[]): Map<string, string> {
  const lookup = new Map<string, string>();
  for (const item of items || []) {
    const value = item.value == null || item.value === "" ? "" : String(item.value);
    if (!value) continue;
    if (item.metric_key) lookup.set(item.metric_key, value);
    if (item.label) lookup.set(item.label, value);
  }
  return lookup;
}
function summarizeCapitalEvidence(capital?: CapitalEvidenceResult | null): Record<string, string> {
  const sections = capital?.sections || [];
  return {
    fundFlow: evidenceSectionSummary(sections, ["fund_flow", "资金流"]),
    institution: evidenceSectionSummary(sections, ["institution_lhb", "institution", "机构"]),
    news: evidenceSectionSummary(sections, ["news_sentiment", "message_sentiment", "消息"]),
    technical: evidenceSectionSummary(sections, ["technical_behavior", "technical", "技术"]),
  };
}

function evidenceSectionSummary(sections: CapitalEvidenceSection[], keys: string[]): string {
  const section = sections.find((item) => keys.some((key) => item.key?.includes(key) || item.title?.includes(key)));
  if (!section) return "--";
  const score = section.score == null ? "" : `${formatNumber(section.score)}分`;
  const state = section.available === false ? "暂无证据" : section.summary || section.items?.[0]?.title || "有证据";
  return [score, state].filter(Boolean).join(" · ");
}

function compactMetricItems(items: Array<[string, string]>): Array<[string, string]> {
  return items.map(([label, value]) => [label, value || "--"]);
}

function numeric(value: unknown): number {
  if (value == null || value === "") return NaN;
  const n = Number(value);
  return Number.isFinite(n) ? n : NaN;
}

function ratioChange(value: unknown, base: unknown): number | null {
  const current = numeric(value);
  const previous = numeric(base);
  if (!Number.isFinite(current) || !Number.isFinite(previous) || previous === 0) return null;
  return (current - previous) / previous;
}

function formatSignedNumber(value: unknown): string {
  const n = numeric(value);
  if (!Number.isFinite(n)) return "--";
  return `${n > 0 ? "+" : ""}${n.toFixed(2)}`;
}

function formatSignedPercent(value: unknown): string {
  const n = numeric(value);
  if (!Number.isFinite(n)) return "--";
  const percent = Math.abs(n) <= 1 ? n * 100 : n;
  return `${percent > 0 ? "+" : ""}${percent.toFixed(2)}%`;
}

function formatMarketCap(stock: StockItem): string {
  if (stock.market_cap_billion != null) return `${formatNumber(stock.market_cap_billion)}亿`;
  return formatNumber(stock.market_cap);
}


function formatScore(value: unknown, max: unknown): string {
  const n = numeric(value);
  if (!Number.isFinite(n)) return "--";
  const m = numeric(max);
  return Number.isFinite(m) && m > 0 ? `${n}/${m}` : String(n);
}

function formatList(values: unknown, mapper?: (value: unknown) => string): string {
  if (!Array.isArray(values) || !values.length) return "--";
  return values.map((item) => mapper ? mapper(item) : String(item)).filter(Boolean).join("、") || "--";
}

function signalTypeLabel(value: unknown): string {
  const labels: Record<string, string> = {
    bullish: "看多",
    bearish: "看空",
    neutral: "中性",
    watch: "观察",
    buy_setup: "买点预备",
    exit: "离场",
    uptrend: "上升趋势",
  };
  return labels[String(value || "")] || String(value || "--");
}

function riskFlagLabel(value: unknown): string {
  const labels: Record<string, string> = {
    high_upper_shadow: "上影线偏长",
    bearish_long_ma_stack: "均线空头排列",
    kdj_overbought: "KDJ 超买",
    kdj_dead_cross: "KDJ 死叉",
    below_support: "跌破支撑",
  };
  return labels[String(value || "")] || String(value || "");
}

function patternSignalLabel(value: unknown): string {
  const labels: Record<string, string> = {
    red_hold: "红持",
    cyan_watch: "青观",
    short_buy: "短买",
    white_exit: "白离",
    swl_above_sws: "SWL 高于 SWS",
    kdj_golden_cross: "KDJ 金叉",
    kdj_dead_cross: "KDJ 死叉",
    accumulation_strength: "吸筹增强",
    swing_opportunity: "波段机会",
  };
  return labels[String(value || "")] || String(value || "");
}

function kdjFromSignal(signal: { k?: number | null; d?: number | null; j?: number | null } | null | undefined) {
  const k = finiteNumber(signal?.k);
  const d = finiteNumber(signal?.d);
  const j = finiteNumber(signal?.j);
  return k == null || d == null || j == null ? null : { k, d, j };
}

function latestKdjFromSeries(series: TrendIndicatorPoint[]) {
  for (let index = series.length - 1; index >= 0; index -= 1) {
    const kdj = kdjFromSignal(series[index]);
    if (kdj) return kdj;
  }
  const bars = toDailyBars(series);
  const computed = computeKdj(bars);
  return computed.length ? computed[computed.length - 1] : null;
}

function signalStatusLabel(status?: string): string {
  const labels: Record<string, string> = {
    neutral: "中性",
    positive: "偏强",
    negative: "偏弱",
    bullish: "看多",
    bearish: "看空",
    buy_setup: "买点预备",
    exit: "离场",
    uptrend: "上升趋势",
    hold: "持有",
    watch: "观察",
    oversold: "超卖",
  };
  return labels[String(status || "neutral")] || String(status || "中性");
}

function signalStatusTone(status?: string): "positive" | "negative" | "warning" | "neutral" {
  switch (status) {
    case "positive":
    case "bullish":
    case "buy_setup":
    case "uptrend":
    case "hold":
      return "positive";
    case "negative":
    case "bearish":
    case "exit":
      return "negative";
    case "watch":
    case "oversold":
      return "warning";
    default:
      return "neutral";
  }
}

function finiteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") return null;
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
}


