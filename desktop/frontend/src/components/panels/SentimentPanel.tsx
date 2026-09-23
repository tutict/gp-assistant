import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { Activity, ExternalLink, Play, RefreshCw, Send, X } from "lucide-react";
import type { LlmSettings, WatchlistItem } from "../../types";
import type { SentimentEvidence, SentimentSnapshot, SentimentTimelinePoint } from "../../types/sentiment";
import { normalizeStockCode } from "../../lib/format";
import { buildLlmConfig } from "../../lib/contracts";
import { useSentiment } from "../../hooks/useSentiment";
import { useMediaQuery } from "../../hooks/useMediaQuery";
import { useMobileComposer } from "../../hooks/useMobileComposer";
import {
  chartBounds,
  chartMarkBottom,
  evidenceNotes,
  formatMetric,
  metricEntries,
  metricIncludesZero,
  safeHttpUrl,
  sentimentCivilDate,
  sourceHostLabel,
  type MetricReferenceId,
} from "../../lib/sentimentView";
import { StockCodeInput } from "../StockCodeInput";
import { LlmSettingsPanel } from "./LlmSettingsPanel";
import { Sheet } from "../ui/Sheet";

const NewsRagPanel = lazy(async () => ({ default: (await import("./NewsRagPanel")).NewsRagPanel }));
type SettingsUpdater = LlmSettings | null | ((previous: LlmSettings | null) => LlmSettings | null);
type WorkspaceView = "sources" | "sentiment";
interface Props {
  llmSettings?: LlmSettings | null;
  onLlmSettingsChange?: (value: SettingsUpdater) => void;
  watchlist?: WatchlistItem[];
  initialCode?: string;
  initialCodeRequestId?: number;
  initialView?: WorkspaceView;
  onAskAgent?: (prompt: string) => void;
}
const dimensionLabels = { messages: "消息情绪", price: "价格与成交量", industry: "行业情绪" };
const metricLabels: Record<string, string> = {
  fact_count: "事实事件数", verified_fact_count: "已核验事实数", discussion_count: "讨论数", positive_count: "正面数", negative_count: "负面数",
  positive_ratio: "正面占比", negative_ratio: "负面占比", sentiment_balance: "情绪净值", discussion_balance: "讨论情绪净值", sentiment_change_7d: "近 7 天情绪变化",
  heat_percentile: "历史热度分位", discussion_change_7d_pct: "近 7 天讨论变化（%）", baseline_sentiment_balance: "历史基准情绪",
  price_return_30d_pct: "30 天收益（%）", price_return_7d_pct: "7 天收益（%）", volume_change_7d_pct: "7 天成交量变化（%）", latest_close: "最新收盘价",
  industry_return_pct: "行业当日收益（%）", industry_return_30d_pct: "行业 30 天收益（%）", industry_return_7d_pct: "行业 7 天收益（%）", industry_breadth_pct: "行业上涨占比（%）", industry_coverage_pct: "行业覆盖率（%）",
};
const gateLabels = { messages: "消息样本不足", price: "行情样本不足", industry: "行业样本不足", historical_heat: "历史热度样本不足" };
const referenceLabels: Record<string, string> = { M1: "消息指标", M2: "量价指标", M3: "行业指标" };

export function sentimentTime(value: number) {
  return new Date(value < 1e12 ? value * 1000 : value).toLocaleString("zh-CN", { hour12: false });
}
function number(value: number | null | undefined, digits = 2) {
  return value == null || !Number.isFinite(value) ? "缺失" : value.toLocaleString("zh-CN", { maximumFractionDigits: digits });
}
function referenceLabel(id: string) {
  return referenceLabels[id] || id;
}
function Refs({ ids, onOpen }: { ids: string[]; onOpen: (id: string) => void }) {
  return <span className="sentiment-refs">{[...new Set(ids)].map((id) => <button type="button" key={id} onClick={() => onOpen(id)} aria-label={`查看证据 ${id}`}>{referenceLabel(id)}</button>)}</span>;
}
function Notes({ title, items }: { title: string; items: string[] }) {
  return <div className="sentiment-notes"><h4>{title}</h4>{items.length ? <ul>{items.map((item) => <li key={item}>{item}</li>)}</ul> : <p>暂无可验证证据</p>}</div>;
}

export function SentimentPanel(props: Props) {
  const [code, setCode] = useState(() => normalizeStockCode(props.initialCode || props.watchlist?.[0]?.code || ""));
  const watchlistCodes = useMemo(
    () => [...new Set((props.watchlist ?? []).map((item) => normalizeStockCode(item.code)).filter(Boolean))],
    [props.watchlist],
  );
  const state = useSentiment(code, watchlistCodes);
  const [input, setInput] = useState(code);
  const [view, setView] = useState<WorkspaceView>(props.initialView || "sources");
  const [mountedViews, setMountedViews] = useState(() => ({ sources: true, sentiment: props.initialView === "sentiment" }));
  const [inputError, setInputError] = useState("");
  const selectCode = (next: string) => {
    const normalized = normalizeStockCode(next);
    setCode(normalized);
    setInput(normalized);
    setInputError("");
  };
  const openView = (next: WorkspaceView) => {
    setView(next);
    setMountedViews((current) => current[next] ? current : { ...current, [next]: true });
  };
  const commitInput = (value: string) => {
    const trimmed = value.trim();
    if (/^\d{6}(\.(SH|SZ|BJ))?$/i.test(trimmed)) {
      selectCode(trimmed);
      return;
    }
    const matches = (props.watchlist || []).filter((item) => {
      const name = item.name?.trim();
      return Boolean(name) && (name === trimmed || name!.includes(trimmed) || trimmed.includes(name!));
    });
    if (matches.length === 1) {
      selectCode(matches[0].code);
      return;
    }
    setInputError(trimmed ? "请从搜索结果中选择一只股票，或输入六位代码。" : "请输入股票代码或名称。");
  };
  useEffect(() => {
    if (props.initialCode) {
      const next = normalizeStockCode(props.initialCode);
      setCode(next);
      setInput(next);
      setInputError("");
    }
    if (props.initialView) openView(props.initialView);
  }, [props.initialCode, props.initialCodeRequestId, props.initialView]);
  return <section className={`sentiment-panel ${view === "sources" ? "is-sources" : "is-sentiment"}`} id="sectionNewsRag" aria-label="研究工作区">
    <header className="sentiment-header">
      <div className="sentiment-title">
        <h2><Activity size={19} aria-hidden="true" /> {view === "sources" ? "消息" : "个股情绪"}</h2>
        <p>{view === "sources" ? "公告、新闻与资料，按股票查阅" : "事实、讨论与市场表现，交叉验证情绪阶段"}</p>
      </div>
      <div className="sentiment-tabs" role="tablist" aria-label="消息或情绪" onKeyDown={(event) => {
        const order: WorkspaceView[] = ["sources", "sentiment"];
        const index = order.indexOf(view);
        const go = (next: WorkspaceView) => {
          event.preventDefault();
          openView(next);
          if (typeof document !== "undefined") document.getElementById(next === "sources" ? "sentiment-tab-sources" : "sentiment-tab-sentiment")?.focus();
        };
        if (event.key === "ArrowRight" || event.key === "ArrowDown") go(order[(index + 1) % order.length]);
        else if (event.key === "ArrowLeft" || event.key === "ArrowUp") go(order[(index - 1 + order.length) % order.length]);
        else if (event.key === "Home") go("sources");
        else if (event.key === "End") go("sentiment");
      }}>
        <button type="button" id="sentiment-tab-sources" role="tab" aria-selected={view === "sources"} aria-controls="sentiment-panel-sources" tabIndex={view === "sources" ? 0 : -1} onClick={() => openView("sources")}>消息</button>
        <button type="button" id="sentiment-tab-sentiment" role="tab" aria-selected={view === "sentiment"} aria-controls="sentiment-panel-analysis" tabIndex={view === "sentiment" ? 0 : -1} onClick={() => openView("sentiment")}>情绪</button>
      </div>
      <form className="sentiment-toolbar" onSubmit={(event) => { event.preventDefault(); commitInput(input); }}>
        <label htmlFor="sentiment-stock">股票</label>
        <StockCodeInput id="sentiment-stock" value={input} onChange={(value) => { setInput(value); setInputError(""); }} onCommit={commitInput} placeholder="代码或名称" inputAriaLabel="股票代码或名称" />
        <button className="btn" type="submit">查看</button>
        <span className="sentiment-window">近 30 天</span>
      </form>
      <div className="sentiment-header-actions">
        <LlmSettingsPanel settings={props.llmSettings || null} onChange={props.onLlmSettingsChange || (() => undefined)} presentation="dialog" />
      </div>
    </header>
    {inputError && <p role="alert" className="sentiment-error">{inputError}</p>}
    <div id="sentiment-panel-sources" role="tabpanel" aria-labelledby="sentiment-tab-sources" className="sentiment-view sentiment-view-sources" hidden={view !== "sources"}>
      {mountedViews.sources && <Suspense fallback={<p className="sentiment-empty">正在加载消息与资料…</p>}><NewsRagPanel llmSettings={props.llmSettings} onLlmSettingsChange={props.onLlmSettingsChange} watchlist={props.watchlist} code={code} onCodeChange={selectCode} /></Suspense>}
    </div>
    <div id="sentiment-panel-analysis" role="tabpanel" aria-labelledby="sentiment-tab-sentiment" className="sentiment-view sentiment-view-analysis" hidden={view !== "sentiment"}>
      {mountedViews.sentiment && <StockWorkspace key={code} code={code} props={props} state={state} onSelect={selectCode} />}
    </div>
  </section>;
}

function StockWorkspace({ code, props, state, onSelect }: { code: string; props: Props; state: ReturnType<typeof useSentiment>; onSelect: (code: string) => void }) {
  const watchlist = props.watchlist || [];
  const stock = watchlist.find((item) => normalizeStockCode(item.code) === code);
  const llm = useMemo(() => buildLlmConfig(props.llmSettings), [props.llmSettings]);
  const mobile = useMediaQuery("(max-width: 768px)");
  const wide = useMediaQuery("(min-width: 1181px)");
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const [evidenceId, setEvidenceId] = useState<string | null>(null);
  const [evidenceSource, setEvidenceSource] = useState<"analysis" | "timeline">("timeline");
  const [question, setQuestion] = useState("");
  const [chartMetric, setChartMetric] = useState<keyof SentimentTimelinePoint>("sentiment_balance");
  const [mobileWatchlistOpen, setMobileWatchlistOpen] = useState(false);
  const [showNewEvidence, setShowNewEvidence] = useState(false);
  const [expandedPools, setExpandedPools] = useState<Record<string, boolean>>({});
  const composer = useMobileComposer(question);
  const analysis = state.analysis;
  const fresh = state.snapshot;
  const frozen = analysis?.snapshot ?? null;
  const viewingHistory = Boolean(analysis && state.latest && analysis.analysis_id !== state.latest.analysis_id);
  const newerData = Boolean(!viewingHistory && analysis && fresh && frozen && (analysis.stale || fresh.snapshot_id !== frozen.snapshot_id));
  const showingNewEvidence = Boolean(newerData && showNewEvidence && fresh);
  const timelineSnapshot = viewingHistory ? frozen : showingNewEvidence ? fresh : frozen || fresh;
  const busy = state.starting || state.run?.status === "running";
  const history = state.history.filter((item) => item.stock_code === code);
  const onDate = (published: unknown) => !selectedDate || sentimentCivilDate(published) === selectedDate;
  const facts = timelineSnapshot?.evidence.filter((item) => item.pool === "fact" && onDate(item.published_at)) || [];
  const discussions = timelineSnapshot?.evidence.filter((item) => item.pool === "discussion" && onDate(item.published_at)) || [];
  useEffect(() => { setShowNewEvidence(false); setEvidenceId(null); }, [analysis?.analysis_id]);
  useEffect(() => { if (!mobile) setMobileWatchlistOpen(false); }, [mobile]);
  const openEvidence = (id: string, source: "analysis" | "timeline") => {
    setMobileWatchlistOpen(false);
    setEvidenceSource(source);
    setEvidenceId(id);
  };
  const evidenceSnapshot = evidenceSource === "analysis" ? frozen ?? timelineSnapshot : timelineSnapshot;
  const stageTitle = viewingHistory ? "历史阶段" : showingNewEvidence ? "上次结论" : "当前阶段";
  const watchlistBody = <>
    <div className="sentiment-watchlist-heading"><h3>自选股票</h3><span>{watchlist.length}</span></div>
    <div className="sentiment-watchlist-items">{watchlist.map((item) => {
      const normalized = normalizeStockCode(item.code);
      const last = state.stages.find((entry) => entry.stock_code === normalized) ?? (normalized === code ? state.latest : null);
      return <button type="button" key={item.code} aria-pressed={normalized === code} onClick={() => { onSelect(item.code); setMobileWatchlistOpen(false); }}>
        <strong>{item.name || item.code}</strong><span>{item.code}</span><small>{last ? `上次：${last.stage}` : "尚未分析"}</small>{last && <time>{sentimentTime(last.created_at)}</time>}
      </button>;
    })}</div>
    {!watchlist.length && <p>在选股或观察页添加自选，也可以直接输入股票代码。</p>}
    <p className="sentiment-watchlist-note">阶段取自各股票最近一次分析，不随行情自动更新。</p>
    {state.stageError && <p className="sentiment-error" role="alert">{state.stageError}</p>}
  </>;
  const failedGates = Object.entries(timelineSnapshot?.quality_gates ?? {}).filter((entry): entry is [keyof typeof gateLabels, boolean] => entry[1] === false && entry[0] in gateLabels);
  const askAgent = () => {
    if (!analysis || !props.onAskAgent) return;
    const name = timelineSnapshot?.stock_name || stock?.name || code;
    props.onAskAgent(`请研究 ${name}（${code}）。上次情绪阶段是「${analysis.stage}」。请核验这个阶段是否仍成立，并指出会让它失效的证据。`);
  };
  return <div className="sentiment-layout" data-inspecting={wide && !!evidenceId}>
    {mobile && <button type="button" className="sentiment-mobile-watchlist-trigger btn" onClick={() => { setEvidenceId(null); setMobileWatchlistOpen(true); }} aria-label="打开自选股票"><span>当前自选</span><strong>{stock?.name || code || "选择股票"}</strong><small>{watchlist.length} 只 · 查看阶段</small></button>}
    {!mobile && <aside className="sentiment-watchlist" aria-label="自选股票情绪">{watchlistBody}</aside>}
    <Sheet open={mobile && mobileWatchlistOpen} onClose={() => setMobileWatchlistOpen(false)} label="自选股票情绪" className="sentiment-watchlist-sheet">
      <header className="sentiment-section-heading"><h3>自选股票情绪</h3><button type="button" className="btn" onClick={() => setMobileWatchlistOpen(false)} aria-label="关闭自选股票"><X size={18} /></button></header>
      {watchlistBody}
    </Sheet>
    <div className="sentiment-main">
      <div className="sentiment-stock-heading">
        <div>
          <h3>{timelineSnapshot?.stock_name || stock?.name || code || "选择一只股票"} {code && <span>{code}</span>}</h3>
          <p>{timelineSnapshot?.industry || stock?.industry || "行业待补充"} · {timelineSnapshot ? `数据截至 ${sentimentTime(timelineSnapshot.cutoff)}` : "输入代码后读取已有数据"}</p>
        </div>
        <button type="button" className="btn sentiment-analyze" disabled={!code || !llm || state.loading || busy || state.asking} onClick={() => void state.start(llm)}><Play size={15} />{!llm ? "先配置模型" : busy ? "分析进行中" : analysis ? "重新分析" : "开始分析"}</button>
      </div>
      {timelineSnapshot && <section className="sentiment-quality" aria-label="数据覆盖">
        <h3>数据覆盖</h3>
        <p>事实 {timelineSnapshot.coverage.facts} · 讨论 {timelineSnapshot.coverage.discussions} · 行情 {timelineSnapshot.coverage.price_days} 天 · 历史 {timelineSnapshot.coverage.history_days} 天 · 行业 {timelineSnapshot.coverage.industry_covered}/{timelineSnapshot.coverage.industry_members} 家</p>
        {(timelineSnapshot.coverage.gaps.length > 0 || failedGates.length > 0) && <details><summary>查看数据缺口</summary><ul>{failedGates.map(([key]) => <li key={key}>{gateLabels[key]}</li>)}{timelineSnapshot.coverage.gaps.map((gap) => <li key={gap}>{gap}</li>)}</ul></details>}
        <p className="sentiment-quality-note">缺失数据不记为零；讨论不等同于事实，情绪不代表未来涨跌。</p>
      </section>}
      {viewingHistory && <div className="sentiment-snapshot-switch"><button type="button" className="btn" onClick={() => state.showLatest()}>返回最新分析</button><p className="sentiment-snapshot-note">正在查看历史分析的冻结快照，结论和追问都不会改用新消息。</p></div>}
      {newerData && <div className="sentiment-snapshot-switch" role="group" aria-label="更新的证据">
        <p className="sentiment-snapshot-note">已有更新的数据。结论仍来自这次分析，可查看新证据或重新分析。</p>
        <button type="button" className="btn" aria-pressed={showingNewEvidence} onClick={() => setShowNewEvidence((current) => !current)}>{showingNewEvidence ? "返回分析快照" : "查看新证据"}</button>
      </div>}
      {!llm && <p className="sentiment-notice">配置右上角 API 设置后可开始分析。读取快照和历史不会调用模型。</p>}
      {state.error && <div className="sentiment-error" role="alert">{state.error}{!busy && <button type="button" className="btn" onClick={state.retry}><RefreshCw size={14} />重试加载</button>}</div>}
      {busy && <div className="sentiment-progress" role="status"><span>{state.starting ? "正在创建分析任务…" : state.run?.stage}</span><progress max={100} value={state.run?.progress || 0} aria-label="分析进度" />{state.run?.status === "running" && <button type="button" className="btn" onClick={() => void state.cancel()}>取消分析</button>}</div>}
      {state.run?.status === "cancelled" && <p role="status" className="sentiment-notice">本次分析已取消，上次结果仍可查看。</p>}
      {state.loading && <p className="sentiment-empty" role="status">正在读取证据快照与分析历史…</p>}
      {analysis ? <section className="sentiment-conclusion" aria-label="情绪结论">
        <div className="sentiment-section-heading"><h3>{stageTitle} <strong>{analysis.stage}</strong></h3><span>证据{analysis.sufficiency}</span></div>
        <p className="sentiment-summary">{analysis.summary}</p>
        <div className="sentiment-verdicts"><div><span>顶部风险</span><strong>{analysis.top_risk}</strong></div><div><span>底部候选</span><strong>{analysis.bottom_candidate}</strong></div><div><span>转折信号</span><strong>{analysis.turning_signal}</strong></div></div>
        <Refs ids={analysis.evidence_ids} onOpen={(id) => openEvidence(id, "analysis")} />
        <p className="sentiment-meta">分析于 {sentimentTime(analysis.created_at)} · {analysis.model} · 规则 {analysis.rule_version}</p>
        <div className="sentiment-conclusion-actions">
          {props.onAskAgent && <button type="button" className="btn" onClick={askAgent}>交给 Agent</button>}
        </div>
        <details className="sentiment-rationale"><summary>判断依据、反证与失效条件</summary><div className="sentiment-reasoning"><Notes title="支持证据" items={analysis.support} /><Notes title="反对证据" items={analysis.against} /><Notes title="失效条件" items={analysis.invalidation} /></div></details>
      </section> : !state.loading && <section className="sentiment-empty"><h3>先核对证据，再生成结论</h3><p>{code ? "点击「开始分析」，结合近 30 天消息、价格成交量和行业表现判断情绪阶段。数据不足会明确标注。" : "从自选列表选择股票，或在上方输入代码。"}</p></section>}
      {analysis && <section className="sentiment-followup">
        <h3>围绕本次分析追问</h3>
        <p>固定引用 {sentimentTime(analysis.created_at)} 的证据快照，新消息不会混入回答。</p>
        {state.followups.map((answer) => <article key={`${answer.question}-${answer.created_at}`}><h4>{answer.question}</h4><p>{answer.answer}</p><Refs ids={answer.evidence_ids} onOpen={(id) => openEvidence(id, "analysis")} /></article>)}
        <form className={`sentiment-followup-form${mobile ? " is-docked" : ""}`} onSubmit={(event) => {
          event.preventDefault();
          if (busy || state.asking || composer.isComposing()) return;
          void state.ask(question, llm).then((sent) => { if (sent) setQuestion(""); });
        }}>
          <label className="visually-hidden" htmlFor="sentiment-question">追问内容</label>
          <textarea ref={composer.textareaRef} onFocus={composer.onFocus} onBlur={composer.onBlur} onCompositionStart={composer.onCompositionStart} onCompositionEnd={composer.onCompositionEnd} id="sentiment-question" value={question} onChange={(event) => setQuestion(event.target.value)} onKeyDown={(event) => { if (!mobile && event.key === "Enter" && !event.shiftKey && !composer.isComposing(event) && !busy && !state.asking && question.trim()) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); } }} placeholder="例如：哪些证据会使当前阶段判断失效？" rows={2} maxLength={4000} />
          <button type="submit" className="btn" disabled={!question.trim() || state.asking || busy}><Send size={15} />{state.asking ? "正在回答…" : "追问"}</button>
        </form>
      </section>}
      {timelineSnapshot && <section className="sentiment-timeline">
        <div className="sentiment-section-heading">
          <h3>30 天情绪与市场表现</h3>
          <div className="sentiment-chart-controls">
            <div className="sentiment-chart-modes" role="group" aria-label="时间轴维度">{([
              { label: "消息", metric: "sentiment_balance", keys: ["sentiment_balance", "discussion_count"] },
              { label: "量价", metric: "close", keys: ["close", "volume"] },
              { label: "行业", metric: "industry_return", keys: ["industry_return", "industry_breadth"] },
            ] as const).map((group) => <button type="button" key={group.label} aria-pressed={(group.keys as readonly string[]).includes(chartMetric)} onClick={() => setChartMetric(group.metric)}>{group.label}</button>)}</div>
            <label>指标 <select value={chartMetric} onChange={(event) => setChartMetric(event.target.value as keyof SentimentTimelinePoint)}>
              <option value="sentiment_balance">情绪净值</option><option value="discussion_count">讨论热度</option><option value="close">收盘价</option><option value="volume">成交量</option><option value="industry_return">行业涨跌幅</option><option value="industry_breadth">行业上涨占比</option>
            </select></label>
          </div>
        </div>
        <Timeline points={timelineSnapshot.timeline} metric={chartMetric} selectedDate={selectedDate} onSelect={setSelectedDate} mobile={mobile} />
        <div className="sentiment-section-heading"><p>{selectedDate ? `${selectedDate} 的事件与讨论` : "选择日期后，只显示当天的证据；缺失值保留为空。日期按北京时间归类。"}</p>{selectedDate && <button className="btn" type="button" onClick={() => setSelectedDate(null)}>全部日期</button>}</div>
        <details><summary>查看逐日数据表</summary><div className="sentiment-table-scroll" tabIndex={0} role="region" aria-label="逐日数据"><table><thead><tr><th>日期</th><th>正面</th><th>负面</th><th>讨论</th><th>情绪净值</th><th>收盘价</th><th>成交量</th><th>行业涨跌幅</th><th>行业上涨占比</th></tr></thead><tbody>{timelineSnapshot.timeline.map((point) => <tr key={point.date} data-selected={point.date === selectedDate}><td><button type="button" onClick={() => setSelectedDate(point.date === selectedDate ? null : point.date)}>{point.date}</button></td>{(["positive", "negative", "discussion_count", "sentiment_balance", "close", "volume", "industry_return", "industry_breadth"] as const).map((key) => <td key={key}>{number(point[key])}</td>)}</tr>)}</tbody></table></div></details>
      </section>}
      {analysis && <section className="sentiment-dimensions" aria-label="三个分析维度">{(["messages", "price", "industry"] as const).map((key) => {
        const dimension = analysis.dimensions.find((item) => item.key === key);
        return <article key={key}><h3>{dimensionLabels[key]} <span>{dimension?.direction || "证据不足"}</span></h3><p>{dimension?.summary || "此维度暂无可验证结论。"}</p><Refs ids={dimension?.evidence_ids || []} onOpen={(id) => openEvidence(id, "analysis")} />{dimension && <details><summary>支持、反对与缺口</summary><Notes title="支持" items={dimension.support} /><Notes title="反对" items={dimension.against} /><Notes title="数据缺口" items={dimension.gaps} /></details>}</article>;
      })}</section>}
      {timelineSnapshot && <>{[
        { title: "事实事件", items: facts, empty: "此范围暂无可核验的事实事件。", hint: "公告与新闻优先；来源身份和全文覆盖单独标注。" },
        { title: "市场讨论", items: discussions, empty: "此范围暂无已采集讨论，不能据此判断讨论热度为零。", hint: "讨论只反映表达情绪，不作为公司事实。" },
      ].map((pool) => {
        const preview = mobile ? 3 : 5;
        const visible = expandedPools[pool.title] ? pool.items : pool.items.slice(0, preview);
        const rest = pool.items.length - visible.length;
        return <section className="sentiment-evidence-list" key={pool.title}><h3>{pool.title} <span>{pool.items.length}</span></h3><p>{pool.hint}</p>{visible.length ? visible.map((item) => <EvidenceRow key={item.id} item={item} onOpen={(id) => openEvidence(id, "timeline")} />) : <p className="sentiment-empty-inline">{pool.empty}</p>}{rest > 0 && <button type="button" className="btn" onClick={() => setExpandedPools((current) => ({ ...current, [pool.title]: true }))}>查看其余 {rest} 条</button>}</section>;
      })}</>}
      {history.length > 0 && <section className="sentiment-history"><h3>分析历史</h3><div>{history.map((item) => <button className="btn" type="button" key={item.analysis_id} disabled={busy} aria-pressed={analysis?.analysis_id === item.analysis_id} onClick={() => state.selectAnalysis(item)}>{sentimentTime(item.created_at)} · {item.stage}</button>)}</div></section>}
      <p className="sentiment-disclaimer">仅供研究，不构成投资建议。</p>
    </div>
    <EvidenceDrawer id={evidenceId} snapshot={evidenceSnapshot} fallback={evidenceSnapshot === frozen ? fresh : frozen} onClose={() => setEvidenceId(null)} inline={wide} />
  </div>;
}

function EvidenceRow({ item, onOpen }: { item: SentimentEvidence; onOpen: (id: string) => void }) {
  const notes = evidenceNotes(item);
  return <button type="button" className="sentiment-evidence-row" onClick={() => onOpen(item.id)}><span className="sentiment-evidence-id">{referenceLabel(item.id)}</span><span><strong>{item.title}</strong><span>{item.source_name} · {sourceHostLabel(item.source_verified)} · {item.coverage === "full_text" ? "全文" : "摘要"}{notes.length ? ` · ${notes.join(" · ")}` : ""}</span></span><time>{sentimentCivilDate(item.published_at) || "发布时间未知"}</time></button>;
}

function Timeline({ points, metric, selectedDate, onSelect, mobile }: { points: SentimentTimelinePoint[]; metric: keyof SentimentTimelinePoint; selectedDate: string | null; onSelect: (date: string | null) => void; mobile: boolean }) {
  const values = points.map((point) => point[metric]).filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  const bounds = chartBounds(values, metricIncludesZero(metric));
  const showZero = bounds.min < 0 && bounds.max > 0;
  if (!points.length) return <p className="sentiment-empty-inline">暂无逐日数据。可在「消息」页补充资料，或更新行情缓存。</p>;
  const coords = points.map((point, index) => {
    const raw = point[metric];
    const value = typeof raw === "number" && Number.isFinite(raw) ? raw : null;
    const x = ((index + 0.5) / points.length) * 100;
    const y = value == null ? null : 100 - chartMarkBottom(value, bounds);
    return { point, value, x, y };
  });
  const segments: string[] = [];
  let segment = "";
  for (const item of coords) {
    if (item.y == null) {
      if (segment) segments.push(segment);
      segment = "";
    } else segment += `${segment ? " " : ""}${item.x},${item.y}`;
  }
  if (segment) segments.push(segment);
  const chart = <div className="sentiment-line-plot">
    <svg className={mobile ? "sentiment-spark" : "sentiment-line-svg"} viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
      {showZero && <line className="sentiment-line-zero" x1="0" x2="100" y1={100 - chartMarkBottom(0, bounds)} y2={100 - chartMarkBottom(0, bounds)} />}
      {segments.map((pointsText) => <polyline key={pointsText} points={pointsText} />)}
    </svg>
    {coords.map((item) => item.y == null ? null : <span key={item.point.date} className="sentiment-line-dot" data-negative={Number(item.value) < 0} style={{ left: `${item.x}%`, top: `${item.y}%` }} />)}
    {!mobile && <div className="sentiment-line-hits">{coords.map((item) => <button type="button" key={item.point.date} className="sentiment-line-hit" style={{ left: `${item.x}%` }} data-bottom={item.value == null ? undefined : `${chartMarkBottom(item.value, bounds)}%`} aria-pressed={selectedDate === item.point.date} aria-label={`${item.point.date} ${number(item.value)}`} title={`${item.point.date}：${number(item.value)}`} onClick={() => onSelect(selectedDate === item.point.date ? null : item.point.date)} />)}</div>}
  </div>;
  if (mobile) {
    const dates = points.map((point) => point.date);
    const index = selectedDate ? Math.max(0, dates.indexOf(selectedDate)) : -1;
    const focused = index >= 0 ? coords[index] : null;
    return <div className="sentiment-scrubber-chart">
      {chart}
      <div className="sentiment-scrubber">
        <button type="button" className="btn" onClick={() => onSelect(dates[Math.max(0, (index < 0 ? dates.length - 1 : index) - 1)])} disabled={dates.length < 2 || index === 0}>上一日</button>
        <strong>{focused ? `${focused.point.date} · ${number(focused.value)}` : "全部日期"}</strong>
        <button type="button" className="btn" onClick={() => onSelect(dates[Math.min(dates.length - 1, index < 0 ? dates.length - 1 : index + 1)])} disabled={dates.length < 2 || index === dates.length - 1}>下一日</button>
        {selectedDate && <button type="button" className="btn" onClick={() => onSelect(null)}>全部日期</button>}
      </div>
    </div>;
  }
  return <div className="sentiment-line-chart" role="group" aria-label="逐日指标，点击日期筛选证据">
    {chart}
    <div className="sentiment-line-axis"><span>{coords[0]?.point.date.slice(5)}</span><span>{selectedDate ? selectedDate.slice(5) : coords[Math.floor(coords.length / 2)]?.point.date.slice(5)}</span><span>{coords[coords.length - 1]?.point.date.slice(5)}</span></div>
  </div>;
}

function EvidenceDrawer({ id, snapshot, fallback, onClose, inline }: { id: string | null; snapshot: SentimentSnapshot | null; fallback: SentimentSnapshot | null; onClose: () => void; inline: boolean }) {
  const evidence = snapshot?.evidence.find((item) => item.id === id) ?? fallback?.evidence.find((item) => item.id === id);
  const metricSnapshot = evidence ? snapshot?.evidence.some((item) => item.id === id) ? snapshot : fallback : snapshot ?? fallback;
  const metric = id === "M1" || id === "M2" || id === "M3";
  const title = id === "M1" ? "消息聚合指标" : id === "M2" ? "价格与成交量指标" : "行业指标";
  const entries = metric && metricSnapshot ? metricEntries(metricSnapshot, id as MetricReferenceId) : [];
  const url = safeHttpUrl(evidence?.provenance?.original_url) ?? safeHttpUrl(evidence?.url);
  const notes = evidence ? evidenceNotes(evidence) : [];
  const content = <>
    <header className="sentiment-section-heading"><h3>{id ? referenceLabel(id) : "证据"} · {metric ? title : "原始证据"}</h3><button type="button" className="btn" onClick={onClose} aria-label="关闭证据"><X size={18} /></button></header>
    {metric && metricSnapshot ? <>
      <p>固定快照：{metricSnapshot.snapshot_id}</p>
      <p>数据截至 {sentimentTime(metricSnapshot.cutoff)} · 规则 {metricSnapshot.rule_version}</p>
      <dl className="sentiment-metrics">{entries.map(([key, value]) => <div key={key}><dt>{metricLabels[key] || key}</dt><dd>{formatMetric(key, value)}</dd></div>)}</dl>
      {!entries.length && <p>此快照暂无该维度指标。</p>}
      <p>{id === "M1" ? `事实 ${metricSnapshot.coverage.facts} 条，讨论 ${metricSnapshot.coverage.discussions} 条。去重后统计，原文见事实事件与市场讨论。` : id === "M2" ? `已覆盖 ${metricSnapshot.coverage.price_days} 个行情日，逐日原始值见数据表。` : `固定行业：${metricSnapshot.industry || "未知"}；覆盖 ${metricSnapshot.coverage.industry_covered}/${metricSnapshot.coverage.industry_members} 家。`}</p>
    </> : evidence ? <>
      <h3>{evidence.title}</h3>
      <p>{evidence.source_name} · {evidence.source_tier} · {sourceHostLabel(evidence.source_verified)}</p>
      {evidence.provenance?.source_verification && <p>域名归属只说明主办方，不代表文章内容已经核验。</p>}
      <p>发布：{sentimentCivilDate(evidence.published_at) || "未知"}<br />首次采集：{sentimentTime(evidence.first_seen_at)}</p>
      <p>{evidence.pool === "fact" ? "事实事件" : "市场讨论"} · {evidence.coverage === "full_text" ? "全文覆盖" : "仅摘要覆盖"}{notes.length ? ` · ${notes.join(" · ")}` : ""}</p>
      <blockquote>{evidence.excerpt}</blockquote>
      {url && <a className="btn" href={url} target="_blank" rel="noopener noreferrer"><ExternalLink size={15} />查看原始来源</a>}
      <p>文档 {evidence.document_id}<br />事件 {evidence.event_id}</p>
    </> : <p>当前快照找不到此引用，无法核验证据。</p>}
  </>;
  if (inline) return id ? <aside className="sentiment-inspector" aria-label="情绪证据详情">{content}</aside> : null;
  return <Sheet open={!!id} onClose={onClose} label="情绪证据详情" className="sentiment-evidence-drawer">{content}</Sheet>;
}
