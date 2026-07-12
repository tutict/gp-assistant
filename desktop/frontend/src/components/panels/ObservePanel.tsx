import { useCallback, useEffect, useState } from "react";
import type { CapitalEvidenceResult, CapitalEvidenceSection, FinancialIndicatorItem, ObserveResult, StockItem, TrendIndicatorPoint, TrendIndicatorSignal, WatchlistItem } from "../../types";
import { computeKdj, toDailyBars } from "../../lib/kline";
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
}

const OBSERVE_FULL_HISTORY_START = "1990-01-01";
const OBSERVE_FULL_HISTORY_LIMIT = "10000";

export function ObservePanel({ initialCode, initialCodeRequestId = 0, watchlist, onWatchlistChange }: ObservePanelProps) {
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
          <StockCodeInput id="observeCode" value={code} onChange={setCode} placeholder="输入股票代码或名称" />
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

  const sections: Array<{ title: string; items: Array<[string, string]> }> = [
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
        ["突破值", formatNumber(valueFromRecord(signal, "breakout"))],
        ["反转值", formatNumber(valueFromRecord(signal, "reversal"))],
        ["风险标记", formatList(signal?.risk_flags, riskFlagLabel)],
      ]),
    },
    {
      title: "KDJ 与交易信号",
      items: compactMetricItems([
        ["K", formatNumber(kdj?.k)],
        ["D", formatNumber(kdj?.d)],
        ["J", formatNumber(kdj?.j)],
        ["金叉", yesNo(signal?.kdj_golden_cross)],
        ["死叉", yesNo(signal?.kdj_dead_cross)],
        ["超买", yesNo(signal?.kdj_overbought)],
        ["超卖", yesNo(signal?.kdj_oversold ?? signal?.oversold)],
        ["红持", yesNo(signal?.red_hold ?? latest?.red_hold)],
        ["青观", yesNo(signal?.cyan_watch ?? latest?.cyan_watch)],
        ["短买", yesNo(signal?.short_buy ?? latest?.short_buy)],
        ["白离", yesNo(signal?.white_exit ?? latest?.white_exit)],
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
    },    {
      title: "最新指标",
      items: compactMetricItems([
        ["市盈率(TTM)", metricOrMissing(formatNumber(stock.pe))],
        ["市净率(最新)", metricOrMissing(formatNumber(stock.pb))],
        ["每股收益(计算)", financialMetricAny(financialLookup, ["latest_eps", "eps", "estimated_eps", "最新每股收益", "每股收益(估算)", "每股收益"], metricOrMissing(formatNumber(stock.latest_eps ?? stock.eps)))],
        ["每股净资产", financialMetricAny(financialLookup, ["latest_bps", "estimated_bps", "每股净资产", "每股净资产(估算)"])],
        ["营业总收入", financialMetricAny(financialLookup, ["operating_revenue", "total_operating_revenue", "revenue", "营业总收入", "营业收入"])],
        ["总营收同比", financialMetricAny(financialLookup, ["operating_revenue_yoy", "revenue_growth_rate", "total_operating_revenue_yoy", "总营收同比", "营业收入同比"])],
        ["归母净利润", financialMetricAny(financialLookup, ["net_profit_parent", "parent_net_profit", "np_parent_company_owners", "归母净利润"])],
        ["归母净利同比", financialMetricAny(financialLookup, ["net_profit_parent_yoy", "parent_net_profit_yoy", "归母净利同比", "归母净利润同比"])],
        ["扣非净利润", financialMetricAny(financialLookup, ["deducted_net_profit", "deducted_net_profit_billion", "扣非净利润"])],
        ["扣非净利同比", financialMetricAny(financialLookup, ["deducted_net_profit_growth_rate", "扣非净利同比", "扣非净利润增长率", "扣非增长"])],
        ["毛利率", financialMetricAny(financialLookup, ["gross_margin", "gross_profit_margin", "毛利率"])],
        ["净利率", financialMetricAny(financialLookup, ["net_margin", "net_profit_margin", "deducted_net_profit_margin", "净利率", "扣非净利率"])],
        ["净资产收益率", metricOrMissing(formatRatioPercent(stock.roe))],
        ["资产负债率", financialMetricAny(financialLookup, ["asset_liability_ratio", "debt_to_asset_ratio", "资产负债率"])],
        ["商誉净资产比", financialMetricAny(financialLookup, ["goodwill_to_net_assets", "goodwill_net_asset_ratio", "商誉净资产比"])],
        ["质押总股本比", financialMetricAny(financialLookup, ["pledged_share_ratio", "pledge_total_share_ratio", "质押总股本比"])],
        ["股息率", metricOrMissing(formatRatioPercent(stock.dividend_yield))],
        ["股利支付率(静)", financialMetricAny(financialLookup, ["dividend_payout_ratio", "static_dividend_payout_ratio", "股利支付率(静)", "股利支付率"])],
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
  ].map((section) => ({ ...section, items: section.items.filter(([, value]) => value && value !== "--" && value !== "—") }));

  return (
    <section className="observe-text-metrics" aria-label="文字指标面板">
      <header>
        <h3>文字指标</h3>
        <p>汇总行情、趋势、KDJ、热度、评分、财务和资金证据。</p>
      </header>
      <div className="observe-text-metric-sections">
        {sections.filter((section) => section.items.length).map((section) => (
          <article key={section.title} className="observe-text-metric-section">
            <h4>{section.title}</h4>
            <div className="observe-text-metric-grid">
              {section.items.map(([label, value]) => (
                <div key={`${section.title}-${label}`} className={`observe-text-metric-item ${metricItemSizeClass(label, value)}`}>
                  <span>{label}</span>
                  <strong>{value}</strong>
                </div>
              ))}
            </div>
          </article>
        ))}
      </div>
      {analysisItems.length ? (
        <section className="observe-metric-analysis compact" aria-label="指标总结分析">
          <h4>指标总结</h4>
          <p className="observe-metric-one-line">{buildMetricOneLineSummary(analysisItems)}</p>
          <p className="observe-metric-analysis-disclaimer">仅供选股研究，不构成投资建议；请结合公告、财报和自身风险承受能力独立判断。</p>
        </section>
      ) : null}
    </section>
  );
}

type MetricAnalysisTone = "positive" | "warning" | "negative" | "neutral";

type MetricAnalysisItem = {
  title: string;
  text: string;
  tone: MetricAnalysisTone;
};

function buildMetricOneLineSummary(items: MetricAnalysisItem[]): string {
  const priority = { negative: 0, warning: 1, positive: 2, neutral: 3 } as const;
  const ordered = [...items].sort((a, b) => priority[a.tone] - priority[b.tone]);
  const main = ordered.slice(0, 2).map((item) => `${item.title}：${trimSentence(item.text)}`).join("；");
  return main ? `${main}。` : "当前指标证据不足，建议补充行情、财务和资金数据后再判断。";
}

function trimSentence(text: string): string {
  return String(text || "").replace(/[。；;\s]+$/g, "");
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
function metricItemSizeClass(label: string, value: string): "metric-short" | "metric-long" | "metric-normal" {
  const compactValue = String(value || "").replace(/\s+/g, "");
  if (compactValue.length <= 8 && label.length <= 5) return "metric-short";
  if (compactValue.length >= 14 || /、|·|，|。|;|；/.test(value)) return "metric-long";
  return "metric-normal";
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
function financialMetricAny(lookup: Map<string, string>, keys: string[], fallback = "暂无"): string {
  for (const key of keys) {
    const value = lookup.get(key);
    if (value != null && String(value).trim() && String(value).trim() !== "--") return String(value);
  }
  return fallback;
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

function yesNo(value: unknown): string {
  if (value === true) return "是";
  if (value === false) return "否";
  return "--";
}

function formatList(values: unknown, mapper?: (value: unknown) => string): string {
  if (!Array.isArray(values) || !values.length) return "--";
  return values.map((item) => mapper ? mapper(item) : String(item)).filter(Boolean).join("、") || "--";
}

function valueFromRecord(record: unknown, key: string): unknown {
  if (!record || typeof record !== "object") return undefined;
  return (record as Record<string, unknown>)[key];
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





