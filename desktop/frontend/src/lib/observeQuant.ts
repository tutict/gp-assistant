import type { TrendIndicatorPoint } from "../types";

export type ObserveQuantTone = "positive" | "warning" | "negative" | "neutral";

export interface ObserveQuantConclusion {
  key: "fund_divergence" | "trend_efficiency" | "volatility_state" | "liquidity_risk";
  label: string;
  state: string;
  summary: string;
  tone: ObserveQuantTone;
}

export interface ObserveQuantResult {
  conclusions: ObserveQuantConclusion[];
  details: Array<[string, string]>;
}

type QuantCalculation = {
  conclusion: ObserveQuantConclusion;
  details: Array<[string, string]>;
};

const FUND_LOOKBACK = 5;
const TREND_WINDOW = 20;
const ATR_WINDOW = 14;
const LIQUIDITY_WINDOW = 20;

export function calculateObserveQuant(series: TrendIndicatorPoint[]): ObserveQuantResult {
  const fund = calculateFundDivergence(series);
  const trend = calculateTrendEfficiency(series);
  const volatility = calculateVolatilityState(series);
  const liquidity = calculateLiquidityRisk(series);
  return {
    conclusions: [fund.conclusion, trend.conclusion, volatility.conclusion, liquidity.conclusion],
    details: [...fund.details, ...trend.details, ...volatility.details, ...liquidity.details],
  };
}

function calculateFundDivergence(series: TrendIndicatorPoint[]): QuantCalculation {
  const samples: Array<{ endIndex: number; proxyDelta: number; priceReturnPct: number }> = [];
  for (let index = FUND_LOOKBACK; index < series.length; index += 1) {
    const currentProxy = fundProxyScore(series[index]);
    const previousProxy = fundProxyScore(series[index - FUND_LOOKBACK]);
    const currentClose = finite(series[index]?.close);
    const previousClose = finite(series[index - FUND_LOOKBACK]?.close);
    if (currentProxy == null || previousProxy == null || currentClose == null || previousClose == null || previousClose === 0) continue;
    samples.push({
      endIndex: index,
      proxyDelta: currentProxy - previousProxy,
      priceReturnPct: (currentClose / previousClose - 1) * 100,
    });
  }
  const current = samples.at(-1);
  if (!current || current.endIndex !== series.length - 1 || samples.length < 8) {
    return insufficientCalculation(
      "fund_divergence",
      "资金背离",
      "资金证据不足",
      "至少需要连续资金代理因子与价格数据。",
      [["资金背离公式", "Z(资金代理5日变化) − Z(价格5日涨跌)"]],
    );
  }
  const recent = samples.slice(-60);
  const proxyZ = zScore(current.proxyDelta, recent.map((item) => item.proxyDelta));
  const priceZ = zScore(current.priceReturnPct, recent.map((item) => item.priceReturnPct));
  const divergence = proxyZ - priceZ;
  const conclusion: ObserveQuantConclusion = divergence >= 1
    ? { key: "fund_divergence", label: "资金背离", state: "资金强于价格", summary: "资金代理改善速度高于价格表现。", tone: "positive" }
    : divergence <= -1
      ? { key: "fund_divergence", label: "资金背离", state: "价格强于资金", summary: "资金代理未能同步价格变化。", tone: "warning" }
      : { key: "fund_divergence", label: "资金背离", state: "资金价格同步", summary: "资金代理与价格暂未出现明显背离。", tone: "neutral" };
  return {
    conclusion,
    details: [
      ["资金背离分", formatSigned(divergence)],
      ["资金代理5日变化", formatSigned(current.proxyDelta)],
      ["价格5日涨跌", formatSignedPercent(current.priceReturnPct)],
      ["资金背离样本", `${recent.length} 个5日窗口`],
      ["资金背离公式", "Z(资金代理5日变化) − Z(价格5日涨跌)"],
    ],
  };
}

function calculateTrendEfficiency(series: TrendIndicatorPoint[]): QuantCalculation {
  const closes = series.map((point) => finite(point.close)).filter((value): value is number => value != null).slice(-TREND_WINDOW);
  if (closes.length < 8) {
    return insufficientCalculation(
      "trend_efficiency",
      "趋势效率",
      "趋势数据不足",
      "至少需要 8 个有效收盘价。",
      [["趋势效率公式", "|周期净位移| ÷ Σ|每日价格变化| × 100"]],
    );
  }
  const netMove = closes.at(-1)! - closes[0];
  let pathMove = 0;
  for (let index = 1; index < closes.length; index += 1) pathMove += Math.abs(closes[index] - closes[index - 1]);
  const efficiency = pathMove > 0 ? Math.abs(netMove) / pathMove * 100 : 0;
  const direction = netMove > 0 ? "up" : netMove < 0 ? "down" : "flat";
  const conclusion: ObserveQuantConclusion = efficiency >= 60
    ? direction === "up"
      ? { key: "trend_efficiency", label: "趋势效率", state: "上行趋势顺畅", summary: "价格路径集中，回撤噪声相对较少。", tone: "positive" }
      : direction === "down"
        ? { key: "trend_efficiency", label: "趋势效率", state: "下行趋势顺畅", summary: "下行路径集中，趋势风险仍在延续。", tone: "negative" }
        : { key: "trend_efficiency", label: "趋势效率", state: "方向尚未形成", summary: "周期内净位移有限。", tone: "neutral" }
    : efficiency >= 35
      ? { key: "trend_efficiency", label: "趋势效率", state: "趋势效率一般", summary: "方向存在，但路径仍有较多反复。", tone: "neutral" }
      : { key: "trend_efficiency", label: "趋势效率", state: "震荡噪声偏高", summary: "价格反复较多，单次突破可信度较低。", tone: "warning" };
  return {
    conclusion,
    details: [
      ["趋势效率 ER", `${formatNumber(efficiency)}%`],
      ["趋势净位移", formatSigned(netMove)],
      ["趋势路径波动", formatNumber(pathMove)],
      ["趋势计算窗口", `${closes.length} 日`],
      ["趋势效率公式", "|周期净位移| ÷ Σ|每日价格变化| × 100"],
    ],
  };
}

function calculateVolatilityState(series: TrendIndicatorPoint[]): QuantCalculation {
  const trueRanges: Array<number | null> = series.map((point, index) => {
    if (index === 0) return null;
    const high = finite(point.high);
    const low = finite(point.low);
    const previousClose = finite(series[index - 1]?.close);
    if (high == null || low == null || previousClose == null || high < low) return null;
    return Math.max(high - low, Math.abs(high - previousClose), Math.abs(low - previousClose));
  });
  const atrSeries: Array<{ endIndex: number; atr: number; atrPct: number }> = [];
  for (let index = ATR_WINDOW; index < series.length; index += 1) {
    const window = trueRanges.slice(index - ATR_WINDOW + 1, index + 1);
    const valid = window.filter((value): value is number => value != null);
    const close = finite(series[index]?.close);
    if (valid.length < ATR_WINDOW || close == null || close <= 0) continue;
    const atr = average(valid);
    atrSeries.push({ endIndex: index, atr, atrPct: atr / close * 100 });
  }
  const current = atrSeries.at(-1);
  if (!current || current.endIndex !== series.length - 1 || atrSeries.length < 5) {
    return insufficientCalculation(
      "volatility_state",
      "波动状态",
      "波动数据不足",
      "至少需要 19 日完整高低收盘价。",
      [["波动状态公式", "ATR14 ÷ 收盘价，并计算历史分位"]],
    );
  }
  const history = atrSeries.slice(-120).map((item) => item.atrPct);
  const percentile = percentileRank(history, current.atrPct);
  const conclusion: ObserveQuantConclusion = percentile <= 20
    ? { key: "volatility_state", label: "波动状态", state: "波动压缩", summary: "波幅处于历史低位，等待方向确认。", tone: "warning" }
    : percentile <= 70
      ? { key: "volatility_state", label: "波动状态", state: "波动常态", summary: "当前波幅处于常见区间。", tone: "neutral" }
      : percentile <= 90
        ? { key: "volatility_state", label: "波动状态", state: "波动扩张", summary: "价格振幅正在扩大，短线噪声上升。", tone: "warning" }
        : { key: "volatility_state", label: "波动状态", state: "高波动", summary: "波幅处于历史高位，回撤风险增大。", tone: "negative" };
  return {
    conclusion,
    details: [
      ["ATR14", formatNumber(current.atr)],
      ["ATR14占价格", `${formatNumber(current.atrPct)}%`],
      ["波动历史分位", `${formatNumber(percentile)}%`],
      ["波动分位样本", `${history.length} 个滚动窗口`],
      ["波动状态公式", "ATR14 ÷ 收盘价，并计算历史分位"],
    ],
  };
}

function calculateLiquidityRisk(series: TrendIndicatorPoint[]): QuantCalculation {
  const daily: Array<{ illiquidity: number; turnover: number } | null> = series.map((point, index) => {
    if (index === 0) return null;
    const close = finite(point.close);
    const previousClose = finite(series[index - 1]?.close);
    const volume = finite(point.volume);
    if (close == null || previousClose == null || previousClose <= 0 || volume == null || volume <= 0) return null;
    const turnover = close * volume;
    return { illiquidity: Math.abs(close / previousClose - 1) / turnover, turnover };
  });
  const rolling: Array<{ endIndex: number; scaledIlliquidity: number; averageTurnover: number }> = [];
  for (let index = LIQUIDITY_WINDOW; index < series.length; index += 1) {
    const window = daily.slice(index - LIQUIDITY_WINDOW + 1, index + 1).filter((value): value is { illiquidity: number; turnover: number } => value != null);
    if (window.length < LIQUIDITY_WINDOW) continue;
    rolling.push({
      endIndex: index,
      scaledIlliquidity: average(window.map((item) => item.illiquidity)) * 100_000_000,
      averageTurnover: average(window.map((item) => item.turnover)),
    });
  }
  const current = rolling.at(-1);
  if (!current || current.endIndex !== series.length - 1 || rolling.length < 5) {
    return insufficientCalculation(
      "liquidity_risk",
      "流动性风险",
      "流动性数据不足",
      "至少需要 25 日有效收盘价与成交量。",
      [["流动性公式", "20日均值(|日收益率| ÷ 成交额) × 10⁸"]],
    );
  }
  const history = rolling.slice(-120).map((item) => item.scaledIlliquidity);
  const percentile = percentileRank(history, current.scaledIlliquidity);
  const conclusion: ObserveQuantConclusion = percentile >= 80
    ? { key: "liquidity_risk", label: "流动性风险", state: "流动性风险偏高", summary: "较小成交可能带来更大的价格冲击。", tone: "negative" }
    : percentile >= 60
      ? { key: "liquidity_risk", label: "流动性风险", state: "流动性风险中等", summary: "价格冲击高于自身常态，需要关注成交承接。", tone: "warning" }
      : { key: "liquidity_risk", label: "流动性风险", state: "流动性风险较低", summary: "当前成交对价格波动的承载相对稳定。", tone: "neutral" };
  return {
    conclusion,
    details: [
      ["Amihud ILLIQ×10⁸", formatNumber(current.scaledIlliquidity, 6)],
      ["流动性风险分位", `${formatNumber(percentile)}%`],
      ["20日平均成交额", formatAmount(current.averageTurnover)],
      ["流动性分位样本", `${history.length} 个滚动窗口`],
      ["流动性公式", "20日均值(|日收益率| ÷ 成交额) × 10⁸"],
    ],
  };
}

function fundProxyScore(point: TrendIndicatorPoint | undefined): number | null {
  if (!point) return null;
  const candidates: Array<[number, number | null]> = [
    [0.35, pointNumber(point, "volume_price_heat")],
    [0.25, pointNumber(point, "accumulation_strength")],
    [0.20, pointNumber(point, "trend_heat")],
    [0.15, accumulationIndexScore(pointNumber(point, "accumulation_index"))],
    [0.05, pointNumber(point, "rebound_signal")],
  ];
  const available = candidates.filter((item): item is [number, number] => item[1] != null);
  if (available.length < 3) return null;
  const totalWeight = available.reduce((sum, [weight]) => sum + weight, 0);
  return available.reduce((sum, [weight, value]) => sum + weight * clamp(value, 0, 100), 0) / totalWeight;
}

function accumulationIndexScore(value: number | null): number | null {
  return value == null ? null : clamp(50 + value * 2, 0, 100);
}

function pointNumber(point: TrendIndicatorPoint, key: string): number | null {
  return finite(point[key]);
}

function insufficientCalculation(
  key: ObserveQuantConclusion["key"],
  label: string,
  state: string,
  summary: string,
  details: Array<[string, string]>,
): QuantCalculation {
  return { conclusion: { key, label, state, summary, tone: "neutral" }, details: [[`${label}原始值`, "证据不足"], ...details] };
}

function zScore(value: number, values: number[]): number {
  const mean = average(values);
  const variance = average(values.map((item) => (item - mean) ** 2));
  const deviation = Math.sqrt(variance);
  return deviation > 1e-9 ? (value - mean) / deviation : 0;
}

function percentileRank(values: number[], current: number): number {
  if (!values.length) return 50;
  const tolerance = 1e-12;
  const below = values.filter((value) => value < current - tolerance).length;
  const equal = values.filter((value) => Math.abs(value - current) <= tolerance).length;
  return (below + equal * 0.5) / values.length * 100;
}

function average(values: number[]): number {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : 0;
}

function finite(value: unknown): number | null {
  if (value == null || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function formatNumber(value: number, maximumFractionDigits = 2): string {
  return value.toLocaleString("zh-CN", { maximumFractionDigits, minimumFractionDigits: 0 });
}

function formatSigned(value: number): string {
  return `${value > 0 ? "+" : ""}${formatNumber(value)}`;
}

function formatSignedPercent(value: number): string {
  return `${value > 0 ? "+" : ""}${formatNumber(value)}%`;
}

function formatAmount(value: number): string {
  if (Math.abs(value) >= 100_000_000) return `${formatNumber(value / 100_000_000)} 亿`;
  if (Math.abs(value) >= 10_000) return `${formatNumber(value / 10_000)} 万`;
  return formatNumber(value);
}
