import type { ScoreContribution, StockRowView } from "../types";
import { formatNumber, formatPrice, kdjRangeLabel, reasonLabel } from "./format";

const riskCodes = new Set([
  "kdj_overbought", "kdj_dead_cross", "white_exit", "cyan_watch", "swl_below_sws",
  "breakdown_ma20", "bearish_long_ma_stack", "volume_stall", "macd_bearish_divergence",
  "weak_final_score", "insufficient_history", "low_volume", "history_behind_quote",
  "signal_type:risk_warning",
]);

export interface SelectionEvidence {
  supports: string[];
  risks: string[];
  riskSummary: string[];
  verification: string[];
  context: string | null;
  historyBehindQuote: boolean;
  scoreBreakdown: ScoreContribution[];
}

function evidenceDate(value: unknown): string | null {
  const raw = String(value ?? "");
  const match = raw.match(/^(\d{4})-?(\d{2})-?(\d{2})/);
  if (!match) return null;
  const date = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
  return date.getUTCFullYear() === Number(match[1]) && date.getUTCMonth() + 1 === Number(match[2]) && date.getUTCDate() === Number(match[3])
    ? `${match[1]}-${match[2]}-${match[3]}` : null;
}

function selectionLineLabel(value: unknown): string {
  const raw = String(value ?? "").trim();
  const base = raw.match(/^Passed the current base screen with raw score ([\d.]+)\.$/);
  if (base) return `通过基础筛选，基础评分 ${base[1]}。`;
  const scores = raw.match(/^(\w+) signal with technical ([\d.]+), pattern ([\d.]+), quality ([\d.]+)\.$/);
  if (scores) return `${reasonLabel(`signal_type:${scores[1]}`)}：技术分 ${scores[2]}、形态分 ${scores[3]}、质量分 ${scores[4]}。`;
  const patterns = raw.match(/^Pattern signals: (.+)\.$/);
  if (patterns) {
    const labels: Record<string, string> = {
      "bottom accumulation": "底部吸筹", "swing opportunity": "波段机会",
      "rebound signal": "反弹信号", "trend-volume resonance": "趋势量能共振",
    };
    return `形态信号：${patterns[1].split(", ").map((code) => labels[code]).filter(Boolean).join("、")}。`;
  }
  const legacy: Record<string, string> = {
    "White-exit signal is active; short-buy setup has high invalidation risk.": "白线离场信号已触发，需重新核验短线趋势。",
    "Cyan-watch state means trend confirmation is insufficient.": "青线观察状态，趋势确认尚不充分。",
    "No major break or high-zone risk is active, but next-day price-volume confirmation is still required.": "暂未触发已配置的技术风险规则，仍需后续价格与成交量确认。",
    "Confirm the signal with next-day price-volume action before entry.": "观察下一交易日的价格与成交量，确认信号是否延续。",
    "Use trend-screen labels to separate breakout, pullback, continuation, and risk candidates.": "结合突破、回踩、趋势延续和风险预警状态继续核验。",
  };
  return legacy[raw] ?? reasonLabel(raw);
}

function uniqueLines(lines: unknown[]): string[] {
  return [...new Set(lines.map(selectionLineLabel).filter((line) => line && !line.startsWith("未识别原因")))];
}

export function selectionEvidence(item: StockRowView): SelectionEvidence {
  const signal = item.trendSignal;
  const quoteDate = evidenceDate(item.quoteTime);
  const historyDate = evidenceDate(signal?.date);
  const analysisEndDate = evidenceDate(item.analysisEndDate);
  const historyBehindQuote = Boolean(quoteDate && historyDate && historyDate < quoteDate && (!analysisEndDate || quoteDate <= analysisEndDate));
  const rawReasons = [...(item.reasonTags || []), ...(item.reasons || [])];
  const flagged = new Set(signal?.risk_flags || []);
  if (signal?.signal_type === "risk_warning") flagged.add("signal_type:risk_warning");
  for (const reason of rawReasons) if (riskCodes.has(reason)) flagged.add(reason);
  const kdjState = signal ? kdjRangeLabel(signal) : null;
  const kdjConflict = kdjState === "高低位信号分歧";
  const riskSummary = uniqueLines([
    ...(historyBehindQuote ? ["日线落后行情"] : []),
    ...(item.riskTags || []),
    ...[...flagged].filter((flag) => !kdjConflict || flag !== "kdj_overbought"),
    ...(kdjConflict ? ["KDJ高低位信号分歧"] : []),
  ]);
  const riskBasis = (item.explanation?.basis || []).map(selectionLineLabel).filter((line) => line.startsWith("风险预警："));
  const risks = uniqueLines([...(item.explanation?.risk_checks || []), ...riskBasis])
    .filter((line) => !riskSummary.length || !line.startsWith("暂未触发"));
  if (flagged.has("kdj_overbought") && !risks.some((line) => line.startsWith("KDJ"))) {
    const label = kdjConflict ? "KDJ高低位信号分歧" : "KDJ超买";
    const triggers = [["K", signal?.k, 80], ["D", signal?.d, 80], ["J", signal?.j, 100]] as const;
    const values = triggers.filter(([, value, threshold]) => typeof value === "number" && value >= threshold)
      .map(([name, value, threshold]) => `${name}=${formatNumber(value)}≥${threshold}`);
    risks.push(`${label}${historyDate ? `（${historyDate} 日线）` : ""}${values.length ? `：${values.join("、")}` : ""}，需观察高位回落风险。`);
  }
  for (const label of riskSummary) if (!risks.some((line) => line.startsWith(label))) risks.push(label);
  if (historyBehindQuote) {
    const index = risks.indexOf("日线落后行情");
    if (index >= 0) risks.splice(index, 1);
    risks.unshift(`日线落后行情：技术依据截至 ${historyDate}，行情截至 ${quoteDate}，历史指标待更新。`);
  }
  const supports = uniqueLines([
    ...rawReasons.filter((reason) => !riskCodes.has(reason) && !(kdjConflict && reason === "kdj_oversold")),
    ...(item.explanation?.basis || []).filter((line) => !signal || !line.startsWith("日线截至")),
  ]).filter((line) => !riskSummary.includes(line));
  const supportingLines = supports.filter((line) => !line.startsWith("风险预警："));
  const context = signal ? [
    `技术日线：${historyDate || "日期未知"} · 收盘 ${formatPrice(signal.close)} 元`,
    `K=${formatNumber(signal.k)} / D=${formatNumber(signal.d)} / J=${formatNumber(signal.j)}（${kdjState}）`,
    `行情快照：${quoteDate || "日期未知"}`,
  ].join(" · ") : quoteDate ? `行情快照：${quoteDate}` : null;
  const scoreLabels: Record<string, string> = {
    "Technical layer": "技术层", "Pattern layer": "形态层", "Quality soft score": "质量层",
  };
  const scoreBreakdown = (item.scoreBreakdown || [])
    .filter((part) => !signal || (part.key !== "base_score" && part.key !== "final_score"))
    .map((part) => ({ ...part, label: scoreLabels[part.label] || part.label }));
  return {
    supports: supportingLines, risks, riskSummary, context, historyBehindQuote, scoreBreakdown,
    verification: uniqueLines(item.explanation?.verification || []),
  };
}
