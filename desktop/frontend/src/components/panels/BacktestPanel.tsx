import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { AdaptiveScreenRequest, BacktestResult, EquityPoint, StockItem, VolatilitySnapshot, WatchlistItem } from "../../types";
import type { FilterCriteria } from "../FilterBar";
import { getJson, postJson } from "../../lib/tauri";
import { buildBacktestRequest, requireBacktestResult } from "../../lib/contracts";
import {
  currentSystemDateInputValue,
  formatMoney,
  formatNumber,
  formatPercent,
  formatSignedPercent,
  normalizeStockCode,
  shortBenchmarkLabel,
  shortRebalanceLabel,
} from "../../lib/format";
import { RawJson } from "../RawJson";
import { PanelFeedback } from "../ui/PanelFeedback";
import type { BacktestRouteRequest, BacktestSource } from "../../lib/viewNavigation";
import { ALL_INDUSTRY_OPTIONS, isLegacyBroadIndustry } from "../../lib/screenIndustryOptions";
import { MARKET_SCOPE_OPTIONS, normalizeMarketScope } from "../../lib/screenScopeOptions";
import {
  buildVolatilityInterpretation,
  VOLATILITY_INTERPRETATION_METHOD,
} from "../../lib/volatilityInterpretation";

interface BacktestPanelProps {
  criteria: FilterCriteria;
  watchlist: WatchlistItem[];
  preferredSource?: BacktestRouteRequest | null;
  onPreferredSourceConsumed?: (requestId: number) => void;
}

export function BacktestPanel({ criteria, watchlist, preferredSource, onPreferredSourceConsumed }: BacktestPanelProps) {
  const [source, setSource] = useState<BacktestSource>("criteria");
  const [start, setStart] = useState("2020-01-01");
  const [end, setEnd] = useState(currentSystemDateInputValue());
  const [topN, setTopN] = useState(10);
  const [rebalance, setRebalance] = useState("monthly");
  const [benchmark, setBenchmark] = useState("candidate_equal_weight");
  const [strategyMode, setStrategyMode] = useState("candidate_snapshot");
  const [adaptiveScreenSpec, setAdaptiveScreenSpec] = useState<AdaptiveScreenRequest | undefined>();
  const [workingCriteria, setWorkingCriteria] = useState<FilterCriteria>(() => ({ ...criteria }));
  const [costBps, setCostBps] = useState(10);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestInFlightRef = useRef(false);
  const requestVersionRef = useRef(0);
  const consumedPreferredRequestIdRef = useRef<number | null>(null);
  const watchlistSignature = watchlist.map((item) => item.code.toUpperCase()).join("|");
  const previousWatchlistSignatureRef = useRef(watchlistSignature);

  useEffect(() => {
    if (!preferredSource) return;
    if (consumedPreferredRequestIdRef.current === preferredSource.requestId) return;
    consumedPreferredRequestIdRef.current = preferredSource.requestId;
    const nextAdaptiveScreenSpec = preferredSource.source === "criteria"
      ? preferredSource.adaptiveScreenSpec
      : undefined;
    requestVersionRef.current += 1;
    setSource(preferredSource.source);
    setAdaptiveScreenSpec(nextAdaptiveScreenSpec);
    setWorkingCriteria(
      preferredSource.source === "criteria"
        ? { ...(preferredSource.criteriaSnapshot ?? criteria) }
        : { ...criteria },
    );
    setStrategyMode(nextAdaptiveScreenSpec ? "adaptive_swing_v1" : "candidate_snapshot");
    setResult(null);
    setError(null);
    onPreferredSourceConsumed?.(preferredSource.requestId);
  }, [criteria, onPreferredSourceConsumed, preferredSource]);

  useEffect(() => {
    if (previousWatchlistSignatureRef.current === watchlistSignature) return;
    previousWatchlistSignatureRef.current = watchlistSignature;
    if (source !== "watchlist") return;
    requestVersionRef.current += 1;
    setResult(null);
    setError(null);
  }, [source, watchlistSignature]);

  const selectSource = (nextSource: BacktestSource) => {
    if (nextSource === source) return;
    requestVersionRef.current += 1;
    setSource(nextSource);
    if (nextSource === "watchlist") {
      if (adaptiveScreenSpec) setWorkingCriteria({ ...criteria });
      setAdaptiveScreenSpec(undefined);
      setStrategyMode("candidate_snapshot");
    }
    setResult(null);
    setError(null);
  };

  const effectiveFilterCriteria = source === "criteria" ? workingCriteria : criteria;
  const updateWorkingCriteria = (patch: Partial<FilterCriteria>) => {
    setWorkingCriteria((current) => ({ ...current, ...patch }));
  };

  const run = async () => {
    if (requestInFlightRef.current) return;
    if (source === "watchlist" && watchlist.length === 0) {
      setResult(null);
      setError("自选股为空，请先从筛选结果中收藏股票。");
      return;
    }

    requestInFlightRef.current = true;
    const requestVersion = ++requestVersionRef.current;
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      const payload = buildBacktestRequest({
        source,
        criteria: effectiveFilterCriteria,
        watchlist,
        startDate: start,
        endDate: end,
        topN,
        rebalanceFrequency: rebalance,
        transactionCostBps: costBps,
        benchmark,
        strategyMode,
        adaptiveScreenSpec,
      });
      const data = await postJson<unknown>("/api/backtest", payload, { timeoutMs: 90_000 });
      const nextResult = requireBacktestResult(data);
      if (requestVersion === requestVersionRef.current) setResult(nextResult);
    } catch (err) {
      if (requestVersion === requestVersionRef.current) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      requestInFlightRef.current = false;
      setLoading(false);
    }
  };

  const effectiveCriteria = source === "criteria" ? adaptiveScreenSpec?.criteria : undefined;
  const criteriaScopeText = effectiveCriteria
    ? [effectiveCriteria.industry, effectiveCriteria.market_scope].filter(Boolean).join(" / ")
    : [effectiveFilterCriteria.industry, effectiveFilterCriteria.marketScope].filter(Boolean).join(" / ");
  const sourceText = source === "watchlist"
    ? `${Math.min(topN, watchlist.length)} / ${watchlist.length} 只`
    : (criteriaScopeText || "全部行业 / 全部范围");
  const selectedIndustry = effectiveFilterCriteria.industry.trim();
  const industryOptions = selectedIndustry && !ALL_INDUSTRY_OPTIONS.includes(selectedIndustry)
    ? [selectedIndustry, ...ALL_INDUSTRY_OPTIONS]
    : ALL_INDUSTRY_OPTIONS;
  const canEditCriteria = source === "criteria" && !adaptiveScreenSpec;

  return (
    <div className="panel-container">
      <div className="backtest-context">
        <div className="backtest-context-head">
          <span>{source === "watchlist" ? "自选股" : "当前条件"}</span>
          <strong>{sourceText}</strong>
        </div>
        <div className="backtest-context-controls">
          <div className="backtest-source-switch" role="group" aria-label="回测股票来源">
            <button
              type="button"
              className={`source-toggle ${source === "criteria" ? "active" : ""}`}
              aria-pressed={source === "criteria"}
              disabled={loading}
              onClick={() => selectSource("criteria")}
            >
              当前条件
            </button>
            <button
              type="button"
              className={`source-toggle ${source === "watchlist" ? "active" : ""}`}
              aria-pressed={source === "watchlist"}
              disabled={loading}
              onClick={() => selectSource("watchlist")}
            >
              自选股
            </button>
          </div>
          <button
            type="button"
            className="backtest-run-button"
            aria-label="运行回测"
            aria-disabled={loading}
            disabled={loading}
            onClick={run}
          >
            {loading ? "回测计算中..." : "运行回测"}
          </button>
        </div>
        <div className="backtest-param-strip">
          <span><b>持仓</b><strong>{topN}</strong></span>
          <span><b>区间</b><strong>{start}~{end}</strong></span>
          <span><b>调仓</b><strong>{shortRebalanceLabel(rebalance)}</strong></span>
          <span><b>成本</b><strong>{formatNumber(costBps)}bps</strong></span>
          <span><b>基准</b><strong>{shortBenchmarkLabel(benchmark)}</strong></span>
          <span><b>Mode</b><strong>{
            strategyMode === "walk_forward"
              ? "Walk-forward"
              : strategyMode === "adaptive_swing_v1" ? "Adaptive swing" : "Snapshot"
          }</strong></span>
        </div>
      </div>

      <div className="panel-controls backtest-controls">
        {canEditCriteria && <>
          <div className="form-row inline">
            <label htmlFor="btIndustry">行业（本页）</label>
            <select id="btIndustry" value={selectedIndustry} disabled={loading}
              onChange={(event) => updateWorkingCriteria({ industry: event.target.value })}>
              {industryOptions.map((industry) => (
                <option key={industry || "all-industries"} value={industry}>
                  {isLegacyBroadIndustry(industry) ? `${industry}（大类）` : industry || "全部行业"}
                </option>
              ))}
            </select>
          </div>
          <div className="form-row inline">
            <label htmlFor="btMarketScope">股票池范围（本页）</label>
            <select id="btMarketScope" value={normalizeMarketScope(effectiveFilterCriteria.marketScope)} disabled={loading}
              onChange={(event) => updateWorkingCriteria({ marketScope: event.target.value })}>
              {MARKET_SCOPE_OPTIONS.map((option) => (
                <option key={option.value || "all-scopes"} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>
        </>}
        <div className="form-row inline"><label htmlFor="btStart">开始</label><input id="btStart" type="date" value={start} disabled={loading} onChange={(e) => setStart(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btEnd">结束</label><input id="btEnd" type="date" value={end} disabled={loading} onChange={(e) => setEnd(e.target.value)} /></div>
        <div className="form-row inline"><label htmlFor="btTopN">持仓</label><input id="btTopN" type="number" min="1" max="100" value={topN} disabled={loading} onChange={(e) => setTopN(Number(e.target.value) || 10)} /></div>
        <div className="form-row inline">
          <label htmlFor="btRebalance">调仓</label>
          <select id="btRebalance" value={rebalance} disabled={loading} onChange={(e) => setRebalance(e.target.value)}>
            <option value="none">买入持有</option>
            <option value="monthly">月度调仓</option>
            <option value="quarterly">季度调仓</option>
          </select>
        </div>
        <div className="form-row inline">
          <label htmlFor="btBenchmark">基准</label>
          <select id="btBenchmark" value={benchmark} disabled={loading} onChange={(e) => setBenchmark(e.target.value)}>
            <option value="candidate_equal_weight">候选池等权</option>
            <option value="none">无</option>
          </select>
        </div>
        <div className="form-row inline"><label htmlFor="btStrategyMode">模式</label><select id="btStrategyMode" value={strategyMode} disabled={loading} onChange={(e) => {
          setStrategyMode(e.target.value);
          if (e.target.value !== "adaptive_swing_v1") setAdaptiveScreenSpec(undefined);
        }}><option value="candidate_snapshot">候选快照</option><option value="walk_forward">Walk-forward</option><option value="adaptive_swing_v1">自适应波段</option></select></div>
        <div className="form-row inline"><label htmlFor="btCostBps">成本</label><input id="btCostBps" type="number" min="0" max="500" value={costBps} disabled={loading} onChange={(e) => setCostBps(Number(e.target.value) || 0)} /></div>
      </div>

      <div className="panel-result">
        {error && <PanelFeedback
          kind="error"
          title="回测失败"
          description={strategyMode === "walk_forward"
            ? `${error} 请补齐历史因子快照，或切换到“候选快照”查看非严格历史组合表现。`
            : error}
        />}
        {loading && !result && !error && <PanelFeedback kind="loading" description="正在计算组合表现..." />}
        {result && !loading && <BacktestResultView result={result} watchlist={watchlist} />}
        {!result && !loading && !error && <PanelFeedback kind="empty" description="选择股票来源并设置参数后运行回测。" />}
      </div>
    </div>
  );
}

type SymbolNameLookup = Record<string, string>;

function symbolKey(symbol: string): string {
  return normalizeStockCode(symbol) || symbol.trim().toUpperCase();
}

function addSymbolName(target: SymbolNameLookup, symbol: string | undefined, name: string | null | undefined): void {
  const code = symbol?.trim();
  const displayName = name?.trim();
  if (!code || !displayName || displayName === code) return;
  target[symbolKey(code)] = displayName;
}

function symbolDisplayLabel(symbol: string, names: SymbolNameLookup): string {
  const name = names[symbolKey(symbol)]?.trim();
  return name && name !== symbol ? `${name}（${symbol}）` : symbol;
}

function useBacktestSymbolNames(result: BacktestResult, watchlist: WatchlistItem[]): SymbolNameLookup {
  const requestedSymbols = useMemo(() => {
    const symbols = new Set<string>();
    for (const symbol of result.symbols ?? []) if (symbol?.trim()) symbols.add(symbol.trim());
    for (const snapshot of result.volatility_snapshots ?? []) if (snapshot.symbol?.trim()) symbols.add(snapshot.symbol.trim());
    return [...symbols];
  }, [result.symbols, result.volatility_snapshots]);
  const providedNames = useMemo(() => {
    const names: SymbolNameLookup = {};
    for (const item of watchlist) addSymbolName(names, item.code, item.name);
    for (const snapshot of result.volatility_snapshots ?? []) addSymbolName(names, snapshot.symbol, snapshot.name);
    return names;
  }, [result.volatility_snapshots, watchlist]);
  const [fetchedNames, setFetchedNames] = useState<SymbolNameLookup>({});
  const symbolNames = useMemo(() => ({ ...fetchedNames, ...providedNames }), [fetchedNames, providedNames]);
  const requestedSignature = requestedSymbols.map(symbolKey).join("|");
  const knownSignature = Object.entries(symbolNames).map(([key, name]) => `${key}:${name}`).sort().join("|");

  useEffect(() => {
    const missingSymbols = requestedSymbols.filter((symbol) => !symbolNames[symbolKey(symbol)]);
    if (!missingSymbols.length) return;
    let disposed = false;
    void Promise.all(missingSymbols.map(async (symbol) => {
      try {
        const stock = await getJson<Partial<StockItem>>(`/api/stocks/${encodeURIComponent(symbol)}`, { timeoutMs: 5000 });
        const name = typeof stock.name === "string" ? stock.name.trim() : "";
        return name && name !== symbol ? [symbolKey(symbol), name] as const : null;
      } catch {
        return null;
      }
    })).then((entries) => {
      if (disposed) return;
      const nextEntries = entries.filter((entry): entry is readonly [string, string] => Boolean(entry));
      if (!nextEntries.length) return;
      setFetchedNames((current) => {
        const next = { ...current };
        for (const [key, name] of nextEntries) next[key] = name;
        return next;
      });
    });
    return () => {
      disposed = true;
    };
  }, [knownSignature, requestedSignature, requestedSymbols, symbolNames]);

  return symbolNames;
}

export function BacktestResultView({ result, watchlist = [] }: { result: BacktestResult; watchlist?: WatchlistItem[] }) {
  const metrics = result.metrics || {};
  const equityPointCount = result.equity_curve?.length ?? 0;
  const symbolNames = useBacktestSymbolNames(result, watchlist);
  return (
    <div className="backtest-result">
      <div className="metric-strip">
        <div className="metric metric-hero"><span>总收益</span><strong className={(metrics.total_return ?? 0) > 0 ? "positive" : (metrics.total_return ?? 0) < 0 ? "negative" : undefined}>{formatSignedPercent((metrics.total_return ?? 0) * 100)}</strong></div>
        <div className="metric"><span>年化收益</span><strong className={(metrics.annualized_return ?? 0) > 0 ? "positive" : (metrics.annualized_return ?? 0) < 0 ? "negative" : undefined}>{metrics.annualized_return != null ? formatSignedPercent(metrics.annualized_return * 100) : "--"}</strong></div>
        <div className="metric"><span>最大回撤</span><strong className={(metrics.max_drawdown ?? 0) < 0 ? "negative" : undefined}>{metrics.max_drawdown != null ? formatSignedPercent(metrics.max_drawdown * 100) : "--"}</strong></div>
        <div className="metric"><span>超额收益</span><strong className={(metrics.excess_return ?? 0) > 0 ? "positive" : (metrics.excess_return ?? 0) < 0 ? "negative" : undefined}>{metrics.excess_return != null ? formatSignedPercent(metrics.excess_return * 100) : "--"}</strong></div>
        <div className="metric"><span>Precision@N</span><strong>{metrics.precision_at_n != null ? formatPercent(metrics.precision_at_n * 100) : "--"}</strong></div>
      </div>

      {equityPointCount >= 2 ? (
        <section className="backtest-primary-chart">
          <EquityCurveChart
            portfolio={result.equity_curve}
            benchmark={result.benchmark_curve ?? []}
            symbolCount={metrics.num_stocks ?? result.symbols?.length ?? 0}
          />
        </section>
      ) : null}
      {equityPointCount === 1 ? (
        <p className="backtest-chart-empty">有效交易日不足，暂不绘制净值曲线。</p>
      ) : null}

      <section className="backtest-comparison">
        <div><span>股票数</span><strong>{metrics.num_stocks ?? result.symbols?.length ?? 0}</strong></div>
        <div><span>基准</span><strong>{result.benchmark_symbols?.length || "未启用"}</strong></div>
        <div><span>交易成本</span><strong>{formatMoney(metrics.total_transaction_cost)}</strong></div>
        <div><span>换手</span><strong>{formatNumber(metrics.total_turnover)}</strong></div>
        <div><span>调仓次数</span><strong>{metrics.rebalance_count ?? 0}</strong></div>
        <div><span>样本外折数</span><strong>{metrics.oos_fold_count ?? 0}</strong></div>
        <div><span>Mode</span><strong>{
          metrics.strategy_mode === "adaptive_swing_v1"
            ? "自适应波段"
            : metrics.strategy_mode === "walk_forward" ? "Walk-forward" : "Snapshot"
        }</strong></div>
      </section>

      {result.adaptive_release_gate && (
        <section className="backtest-holdings">
          <header>
            <span>adaptive_swing_v1 发布门槛</span>
            <strong>{result.adaptive_release_gate.passed ? "全部通过" : "暂不切换默认"}</strong>
          </header>
          {result.legacy_balanced_backtest && (
            <p>
              同样本旧 balanced 年化 {
                result.legacy_balanced_backtest.metrics.annualized_return != null
                  ? formatSignedPercent(result.legacy_balanced_backtest.metrics.annualized_return * 100)
                  : "--"
              } · 新版年化 {
                metrics.annualized_return != null
                  ? formatSignedPercent(metrics.annualized_return * 100)
                  : "--"
              }
            </p>
          )}
          <div className="backtest-fold-list">
            {result.adaptive_release_gate.checks.map((check) => (
              <div className="backtest-fold-row" key={check.key}>
                <span><b>{releaseGateLabel(check.key)}</b><small>{check.requirement}</small></span>
                <strong className={`backtest-gate-status ${check.passed ? "passed" : "failed"}`}>
                  {check.passed ? "通过" : check.actual == null ? "待采集" : "未通过"}
                </strong>
              </div>
            ))}
          </div>
        </section>
      )}

      <VolatilityDiagnostics
        snapshots={result.volatility_snapshots ?? []}
        emptyMessage={result.volatility_message}
        symbolNames={symbolNames}
      />

      {result.walk_forward_folds?.length ? (
        <section className="backtest-holdings">
          <header><span>样本外逐折结果</span><strong>{metrics.oos_fold_count ?? 0}</strong></header>
          <div className="backtest-fold-list">
            {result.walk_forward_folds.map((fold) => (
              <div className="backtest-fold-row" key={`${fold.selection_date}-${fold.evaluation_end_date || "pending"}`}>
                <span>
                  <b>
                    {fold.signal_date ? `${fold.signal_date} 信号 → ` : ""}
                    {fold.selection_date} 成交 → {fold.evaluation_end_date || "待评估"}
                  </b>
                  <small>入选 {fold.selected_symbols.length} · 精度分母 {fold.evaluated_selection_count}/{fold.eligible_symbol_count}</small>
                </span>
                <span>
                  <b>{fold.precision_at_n != null ? `Precision ${formatPercent(fold.precision_at_n * 100)}` : "Precision --"}</b>
                  <small>超额 {fold.average_excess_return != null ? formatSignedPercent(fold.average_excess_return * 100) : "--"}</small>
                </span>
              </div>
            ))}
          </div>
        </section>
      ) : null}

      {result.symbols?.length ? (
        <section className="backtest-holdings">
          <header><span>{result.strategy_mode === "walk_forward" ? "滚动入选标的" : "标的"}</span><strong>{result.symbols.length}</strong></header>
          <ul className="symbol-strip">
            {result.symbols.map((symbol) => (
              <li className="symbol-chip" key={symbol}>{symbolDisplayLabel(symbol, symbolNames)}</li>
            ))}
          </ul>
        </section>
      ) : null}

      {result.notes?.length ? <div className="notes">{result.notes.map((note) => <p key={note}>{note}</p>)}</div> : null}
      <RawJson result={result} />
    </div>
  );
}

function releaseGateLabel(key: string): string {
  return {
    annualized_return_delta: "年化收益差",
    max_drawdown_delta: "最大回撤差",
    precision_at_10_delta: "Precision@10 差",
    max_primary_industry_count: "主榜单行业数量",
    average_adjacent_jaccard: "相邻主榜 Jaccard",
    five_run_unique_coverage: "连续五次覆盖",
    first_run_millis: "首次运行耗时",
    cached_run_millis: "缓存运行耗时",
  }[key] || key;
}

function formatVolatilityPercent(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value)
    ? `${formatNumber(value)}%`
    : "无定义（零波动）";
}

function formatVolatilitySymbolLabel(snapshot: VolatilitySnapshot, names: SymbolNameLookup): string {
  const name = snapshot.name?.trim() || names[symbolKey(snapshot.symbol)]?.trim();
  return name && name !== snapshot.symbol ? `${name}（${snapshot.symbol}）` : snapshot.symbol;
}

const VolatilityDiagnostics = memo(function VolatilityDiagnostics({
  snapshots,
  emptyMessage,
  symbolNames,
}: {
  snapshots: VolatilitySnapshot[];
  emptyMessage?: string | null;
  symbolNames: SymbolNameLookup;
}) {
  const [selectedSymbol, setSelectedSymbol] = useState(() => snapshots[0]?.symbol ?? "");
  const snapshot = snapshots.find((item) => item.symbol === selectedSymbol) ?? snapshots[0];
  if (!snapshot) {
    return (
      <section className="backtest-volatility" aria-label="波动率快照">
        <header>
          <div>
            <span>波动率快照</span>
            <small>无可用区间末标的</small>
          </div>
        </header>
        <p className="volatility-empty">{emptyMessage || "波动率快照不可用。"}</p>
      </section>
    );
  }

  const unavailable = new Map((snapshot.unavailable ?? []).map((item) => [item.indicator, item.reason]));
  const reasonFor = (indicator: string) => unavailable.get(indicator) ?? "指标不可用";
  const atr = snapshot.atr;
  const bollinger = snapshot.bollinger_bands;
  const donchian = snapshot.donchian_channel;
  const keltner = snapshot.keltner_channel;
  const chaikin = snapshot.chaikin_volatility;
  const rvi = snapshot.rvi;
  const items = [
    {
      key: "atr",
      label: `ATR${atr?.period ?? 14}`,
      value: atr ? formatNumber(atr.value) : "--",
      detail: atr ? `占收盘 ${formatVolatilityPercent(atr.percent_of_close)}` : reasonFor("atr"),
      tone: undefined,
    },
    {
      key: "bollinger_bands",
      label: `布林带 ${bollinger?.period ?? 20}/${bollinger?.multiplier ?? 2}`,
      value: bollinger ? `中轨 ${formatNumber(bollinger.middle)}` : "--",
      detail: bollinger
        ? `%B ${formatVolatilityPercent(bollinger.percent_b)} · 带宽 ${formatVolatilityPercent(bollinger.bandwidth_percent)}`
        : reasonFor("bollinger_bands"),
      tone: undefined,
    },
    {
      key: "donchian_channel",
      label: `唐奇安通道 ${donchian?.period ?? 20}`,
      value: donchian ? `${formatNumber(donchian.lower)}–${formatNumber(donchian.upper)}` : "--",
      detail: donchian
        ? `位置 ${formatVolatilityPercent(donchian.position_percent)} · 宽度 ${formatVolatilityPercent(donchian.width_percent)}`
        : reasonFor("donchian_channel"),
      tone: undefined,
    },
    {
      key: "keltner_channel",
      label: `凯尔特纳通道 ${keltner?.ema_period ?? 20}/${keltner?.atr_period ?? 10}/${keltner?.multiplier ?? 2}`,
      value: keltner ? `${formatNumber(keltner.lower)}–${formatNumber(keltner.upper)}` : "--",
      detail: keltner
        ? `位置 ${formatVolatilityPercent(keltner.position_percent)} · 宽度 ${formatVolatilityPercent(keltner.width_percent)}`
        : reasonFor("keltner_channel"),
      tone: undefined,
    },
    {
      key: "chaikin_volatility",
      label: `Chaikin 波动率 ${chaikin?.ema_period ?? 10}/${chaikin?.roc_period ?? 10}`,
      value: chaikin ? formatVolatilityPercent(chaikin.value) : "--",
      detail: chaikin
        ? `${chaikin.ema_period} 日高低价差 EMA 相对 ${chaikin.roc_period} 日前`
        : reasonFor("chaikin_volatility"),
      tone: chaikin?.value == null || chaikin.value === 0
        ? undefined
        : chaikin.value > 0 ? "positive" : "negative",
    },
    {
      key: "rvi",
      label: `相对波动率指数 RVI${rvi?.period ?? 14}`,
      value: rvi ? formatNumber(rvi.value) : "--",
      detail: rvi ? "范围 0–100，50 为方向均衡线" : reasonFor("rvi"),
      tone: undefined,
    },
  ];
  const interpretation = buildVolatilityInterpretation(snapshot);
  const symbolLabel = formatVolatilitySymbolLabel(snapshot, symbolNames);

  return (
    <section className="backtest-volatility" aria-label="波动率快照">
      <header>
        <div>
          <span>波动率快照</span>
          <small>{symbolLabel} · {snapshot.date} · 收盘 {formatNumber(snapshot.close)}</small>
        </div>
        <label className="volatility-symbol-control">
          <span>波动率标的</span>
          <select
            aria-label="波动率标的"
            value={snapshot.symbol}
            onChange={(event) => setSelectedSymbol(event.target.value)}
          >
            {snapshots.map((item) => <option key={item.symbol} value={item.symbol}>{formatVolatilitySymbolLabel(item, symbolNames)}</option>)}
          </select>
        </label>
      </header>
      <div className="volatility-grid">
        {items.map((item) => (
          <div key={item.key}>
            <span>{item.label}</span>
            <strong className={`volatility-value${item.tone ? ` ${item.tone}` : ""}`}>{item.value}</strong>
            <small>{item.detail}</small>
          </div>
        ))}
      </div>
      <section className="volatility-interpretation" aria-label={`${symbolLabel} 波动率结论`}>
        <header>
          <strong>一句话看懂</strong>
          <span>已返回 {interpretation.returnedCount}/6 项指标</span>
        </header>
        <p className="volatility-interpretation-summary">{interpretation.summary}</p>
        <div className="volatility-interpretation-grid">
          <div>
            <h4>为什么这么说</h4>
            <ul>{interpretation.evidence.map((item) => <li key={item}>{item}</li>)}</ul>
          </div>
          <div>
            <h4>策略可以怎么改</h4>
            <ul>{interpretation.adjustments.map((item) => <li key={item}>{item}</li>)}</ul>
          </div>
        </div>
        <details className="volatility-interpretation-method">
          <summary>这些判断怎么算的</summary>
          <ul>{VOLATILITY_INTERPRETATION_METHOD.map((item) => <li key={item}>{item}</li>)}</ul>
        </details>
        <small className="volatility-interpretation-note">这里只说明回测结束时的状态，不会改动历史选股和调仓结果，也不是买卖建议。</small>
      </section>
    </section>
  );
});

const EQUITY_CHART = {
  width: 760,
  height: 210,
  left: 46,
  right: 748,
  top: 14,
  bottom: 190,
} as const;

interface NormalizedEquityPoint {
  date: string;
  value: number;
}

function normalizeEquityCurve(curve: EquityPoint[]): NormalizedEquityPoint[] {
  const valid = curve
    .map((point) => ({ date: point.date, equity: Number(point.equity) }))
    .filter((point) => point.date && Number.isFinite(point.equity));
  const base = valid[0]?.equity;
  if (!Number.isFinite(base) || base === 0) return [];
  return valid.map((point) => ({ date: point.date, value: (point.equity / base) * 100 }));
}

function curveReturn(curve: NormalizedEquityPoint[]): number | null {
  const last = curve[curve.length - 1]?.value;
  return Number.isFinite(last) ? last - 100 : null;
}

function EquityCurveChart({
  portfolio,
  benchmark,
  symbolCount,
}: {
  portfolio: EquityPoint[];
  benchmark: EquityPoint[];
  symbolCount: number;
}) {
  const chart = useMemo(() => {
    const portfolioSeries = normalizeEquityCurve(portfolio);
    const benchmarkSeries = normalizeEquityCurve(benchmark);
    const dates = Array.from(new Set([
      ...portfolioSeries.map((point) => point.date),
      ...benchmarkSeries.map((point) => point.date),
    ])).sort();
    const dateIndexes = new Map(dates.map((date, index) => [date, index]));
    const values = [
      100,
      ...portfolioSeries.map((point) => point.value),
      ...benchmarkSeries.map((point) => point.value),
    ];
    const rawMin = Math.min(...values);
    const rawMax = Math.max(...values);
    const visibleRange = Math.max(rawMax - rawMin, 4);
    const min = Math.max(0, rawMin - visibleRange * 0.12);
    const max = rawMax + visibleRange * 0.12;
    const plotWidth = EQUITY_CHART.right - EQUITY_CHART.left;
    const plotHeight = EQUITY_CHART.bottom - EQUITY_CHART.top;
    const xForDate = (date: string) => {
      const index = dateIndexes.get(date) ?? 0;
      return EQUITY_CHART.left + (index / Math.max(dates.length - 1, 1)) * plotWidth;
    };
    const yForValue = (value: number) => EQUITY_CHART.top + ((max - value) / (max - min)) * plotHeight;
    const pointsFor = (series: NormalizedEquityPoint[]) => series
      .map((point) => `${xForDate(point.date).toFixed(2)},${yForValue(point.value).toFixed(2)}`)
      .join(" ");
    const ticks = Array.from({ length: 5 }, (_, index) => max - ((max - min) * index) / 4);

    return {
      portfolioSeries,
      benchmarkSeries,
      dates,
      portfolioByDate: new Map(portfolioSeries.map((point) => [point.date, point.value])),
      benchmarkByDate: new Map(benchmarkSeries.map((point) => [point.date, point.value])),
      portfolioPoints: pointsFor(portfolioSeries),
      benchmarkPoints: pointsFor(benchmarkSeries),
      ticks,
      xForDate,
      yForValue,
    };
  }, [benchmark, portfolio]);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);

  if (chart.portfolioSeries.length < 2 || chart.dates.length < 2) return null;

  const updateActivePoint = (clientX: number, bounds: DOMRect) => {
    const chartX = ((clientX - bounds.left) / Math.max(bounds.width, 1)) * EQUITY_CHART.width;
    const ratio = Math.max(0, Math.min(
      1,
      (chartX - EQUITY_CHART.left) / (EQUITY_CHART.right - EQUITY_CHART.left),
    ));
    setActiveIndex(Math.round(ratio * (chart.dates.length - 1)));
  };
  const activeDate = activeIndex == null ? null : chart.dates[activeIndex];
  const activePortfolio = activeDate ? chart.portfolioByDate.get(activeDate) : undefined;
  const activeBenchmark = activeDate ? chart.benchmarkByDate.get(activeDate) : undefined;
  const activeX = activeDate ? chart.xForDate(activeDate) : 0;
  const tooltipAlignment = activeIndex != null && activeIndex <= chart.dates.length * 0.18
    ? "is-start"
    : activeIndex != null && activeIndex >= chart.dates.length * 0.82
      ? "is-end"
      : "";

  return (
    <div className="equity-chart">
      <header className="equity-chart-header">
        <div>
          <strong>组合净值曲线</strong>
          <small>{symbolCount > 0 ? `${symbolCount} 只股票组合` : "回测组合"} · 起点归一为 100</small>
        </div>
        <div className="equity-chart-legend" aria-label="净值曲线图例">
          <span><i className="is-portfolio" />组合净值 <b>{formatSignedPercent(curveReturn(chart.portfolioSeries))}</b></span>
          {chart.benchmarkSeries.length ? (
            <span><i className="is-benchmark" />候选池基准 <b>{formatSignedPercent(curveReturn(chart.benchmarkSeries))}</b></span>
          ) : null}
        </div>
      </header>
      <div
        className="equity-chart-plot"
        tabIndex={0}
        aria-label="使用左右方向键查看净值数据点"
        onBlur={() => setActiveIndex(null)}
        onFocus={() => setActiveIndex((current) => current ?? chart.dates.length - 1)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault();
          const step = event.key === "ArrowLeft" ? -1 : 1;
          setActiveIndex((current) => Math.max(0, Math.min(chart.dates.length - 1, (current ?? chart.dates.length - 1) + step)));
        }}
        onPointerDown={(event) => updateActivePoint(event.clientX, event.currentTarget.getBoundingClientRect())}
        onPointerMove={(event) => updateActivePoint(event.clientX, event.currentTarget.getBoundingClientRect())}
        onPointerLeave={() => setActiveIndex(null)}
      >
        <svg
          viewBox={`0 0 ${EQUITY_CHART.width} ${EQUITY_CHART.height}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={chart.benchmarkSeries.length ? "组合与基准净值曲线" : "组合净值曲线"}
        >
          <title>组合净值曲线，起点归一为 100</title>
          <g className="equity-chart-grid" aria-hidden="true">
            {chart.ticks.map((tick) => {
              const y = chart.yForValue(tick);
              return (
                <g key={tick.toFixed(4)}>
                  <line x1={EQUITY_CHART.left} x2={EQUITY_CHART.right} y1={y} y2={y} />
                  <text x="2" y={y + 4}>{tick.toFixed(chart.ticks[0] - chart.ticks[4] < 20 ? 1 : 0)}</text>
                </g>
              );
            })}
          </g>
          <line
            className="equity-chart-baseline"
            x1={EQUITY_CHART.left}
            x2={EQUITY_CHART.right}
            y1={chart.yForValue(100)}
            y2={chart.yForValue(100)}
          />
          {chart.benchmarkPoints ? (
            <polyline className="equity-chart-line is-benchmark" points={chart.benchmarkPoints} />
          ) : null}
          <polyline className="equity-chart-line is-portfolio" points={chart.portfolioPoints} />
          {activeDate ? (
            <g className="equity-chart-cursor" aria-hidden="true">
              <line x1={activeX} x2={activeX} y1={EQUITY_CHART.top} y2={EQUITY_CHART.bottom} />
              {activePortfolio != null ? <circle className="is-portfolio" cx={activeX} cy={chart.yForValue(activePortfolio)} r="4" /> : null}
              {activeBenchmark != null ? <circle className="is-benchmark" cx={activeX} cy={chart.yForValue(activeBenchmark)} r="3.5" /> : null}
            </g>
          ) : null}
        </svg>
        {activeDate ? (
          <div
            className={`equity-chart-tooltip ${tooltipAlignment}`}
            style={{ left: `${(activeX / EQUITY_CHART.width) * 100}%` }}
          >
            <strong>{activeDate}</strong>
            <span><i className="is-portfolio" />组合 {activePortfolio != null ? formatNumber(activePortfolio) : "--"}</span>
            {chart.benchmarkSeries.length ? (
              <span><i className="is-benchmark" />基准 {activeBenchmark != null ? formatNumber(activeBenchmark) : "--"}</span>
            ) : null}
          </div>
        ) : null}
      </div>
      <div className="chart-labels">
        <span>{chart.dates[0]}</span>
        <span>净值指数</span>
        <span>{chart.dates[chart.dates.length - 1]}</span>
      </div>
    </div>
  );
}
