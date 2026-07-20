import type { VolatilitySnapshot } from "../types";
import { formatNumber } from "./format";

export interface VolatilityInterpretation {
  returnedCount: number;
  summary: string;
  evidence: string[];
  adjustments: string[];
}

export const VOLATILITY_INTERPRETATION_METHOD = [
  "ATR 占收盘：低于 1.5% 为偏低，1.5%–3% 为中等，3%–5% 为偏高，5% 以上为很高。",
  "通道位置：综合布林 %B、唐奇安和凯尔特纳位置的中位数；20/40/60/80 划分上下沿与中部，低于 0 或高于 100 表示越轨。",
  "Chaikin：(-3%, 3%) 内视为变化不大；[3%, 10%) 为扩张，(-10%, -3%] 为收缩；达到 ±10% 为明显变化。",
  "RVI：48–52（含端点）为方向均衡；大于 52 且小于 60，或大于 40 且小于 48，为略占优；达到 60 或不高于 40 为明显占优。",
] as const;

interface PositionReading {
  value: number;
  label: string;
}

export function buildVolatilityInterpretation(snapshot: VolatilitySnapshot): VolatilityInterpretation {
  const returnedCount = [
    snapshot.atr,
    snapshot.bollinger_bands,
    snapshot.donchian_channel,
    snapshot.keltner_channel,
    snapshot.chaikin_volatility,
    snapshot.rvi,
  ].filter(Boolean).length;

  const atrPercent = finiteNumber(snapshot.atr?.percent_of_close);
  const positions = positionReadings(snapshot);
  const representativePosition = median(positions.map((item) => item.value));
  const chaikinValue = finiteNumber(snapshot.chaikin_volatility?.value);
  const rviValue = finiteNumber(snapshot.rvi?.value);

  if (returnedCount === 0) {
    return {
      returnedCount,
      summary: "有效数据还不够，现在无法判断价格位置和波动状态。",
      evidence: ["这只股票缺少足够的有效历史日线，请先查看上方每个指标为什么不可用。"],
      adjustments: ["先补齐历史日线，再比较策略参数；不要根据缺失的数据调整规则。"],
    };
  }

  const summaryParts: string[] = [];
  const evidence: string[] = [];
  const adjustments: string[] = [];

  if (representativePosition != null) {
    const positionSummary = positionLabel(representativePosition, positions);
    summaryParts.push(`价格${positionSummary}`);
    evidence.push(`${positions.map((item) => `${item.label} ${formatPercent(item.value)}`).join("、")}；0% 附近代表区间底部，因此${positionSummary}。`);
  }

  if (rviValue != null) {
    summaryParts.push(rviLabel(rviValue));
  }

  if (chaikinValue != null) {
    summaryParts.push(chaikinLabel(chaikinValue));
  }

  if (atrPercent != null) {
    summaryParts.push(`近期一天的典型波动约占股价 ${formatPercent(atrPercent)}，当前波动${atrLabel(atrPercent)}`);
    evidence.unshift(`ATR${snapshot.atr?.period ?? 14} 为 ${formatNumber(snapshot.atr?.value)}，可以把它理解为近期一天的典型波动尺度，约占收盘价 ${formatPercent(atrPercent)}。`);
    adjustments.push(atrAdjustment(atrPercent));
  }

  const plainMeaning = combinedMeaning(representativePosition, rviValue, chaikinValue);
  if (plainMeaning) summaryParts.push(plainMeaning);

  const directionEvidence = directionAndRegimeEvidence(snapshot, rviValue, chaikinValue);
  if (directionEvidence) evidence.push(directionEvidence);

  const confirmationAdjustment = positionAdjustment(representativePosition, rviValue);
  if (confirmationAdjustment) adjustments.push(confirmationAdjustment);
  if (chaikinValue != null) adjustments.push(regimeAdjustment(chaikinValue));

  if (adjustments.length === 0) {
    adjustments.push("样本分层：先按当前可用指标划分波动状态，再比较各层的收益、回撤与误触发率。");
  }

  if (summaryParts.length === 0) {
    evidence.push("部分指标已经返回，但关键数值仍然缺失，所以无法判断价格位置、强弱方向或波动变化。");
    adjustments.unshift("先检查数据是否完整，补齐通道位置、ATR 占比、Chaikin 或 RVI 后再调整策略。");
  }

  return {
    returnedCount,
    summary: summaryParts.length
      ? `${summaryParts.join("；")}。`
      : "目前只能看到部分原始数字，还不足以得出可信的结论。",
    evidence,
    adjustments: adjustments.slice(0, 3),
  };
}

function positionReadings(snapshot: VolatilitySnapshot): PositionReading[] {
  const readings = [
    { value: finiteNumber(snapshot.bollinger_bands?.percent_b), label: "布林 %B" },
    { value: finiteNumber(snapshot.donchian_channel?.position_percent), label: "唐奇安位置" },
    { value: finiteNumber(snapshot.keltner_channel?.position_percent), label: "凯尔特纳位置" },
  ];
  return readings.filter((item): item is PositionReading => item.value != null);
}

function directionAndRegimeEvidence(
  snapshot: VolatilitySnapshot,
  rviValue: number | null,
  chaikinValue: number | null,
): string | null {
  const parts: string[] = [];
  if (rviValue != null) {
    parts.push(`RVI${snapshot.rvi?.period ?? 14} 为 ${formatNumber(rviValue)}，${rviLabel(rviValue)}`);
  }
  if (chaikinValue != null) {
    parts.push(`Chaikin ${snapshot.chaikin_volatility?.ema_period ?? 10}/${snapshot.chaikin_volatility?.roc_period ?? 10} 为 ${formatPercent(chaikinValue)}，${chaikinLabel(chaikinValue)}`);
  }
  return parts.length ? `${parts.join("；")}。` : null;
}

function atrLabel(value: number): string {
  if (value >= 5) return "很大";
  if (value >= 3) return "偏大";
  if (value >= 1.5) return "适中";
  return "较小";
}

function positionLabel(value: number, readings: PositionReading[] = []): string {
  const hasBelow = readings.some((item) => item.value < 0);
  const hasInside = readings.some((item) => item.value >= 0 && item.value <= 100);
  const hasAbove = readings.some((item) => item.value > 100);
  if (value < 0) {
    return hasBelow && hasInside ? "已经贴近近期价格区间底部，而且部分指标显示价格已落到近期常见范围之外" : "已经跌出近期价格区间的常见范围";
  }
  if (value <= 20) return "已经贴近近期价格区间底部";
  if (value < 40) return "位于近期价格区间的偏低位置";
  if (value <= 60) return "位于近期价格区间中部";
  if (value < 80) return "位于近期价格区间的偏高位置";
  if (value <= 100) return "已经贴近近期价格区间顶部";
  return hasAbove && hasInside ? "已经贴近近期价格区间顶部，而且部分指标显示价格已超出近期常见范围" : "已经突破近期价格区间的常见范围";
}

function rviLabel(value: number): string {
  if (value >= 60) return "最近上涨方向的波动明显更多，价格表现偏强";
  if (value > 52) return "最近上涨方向的波动略多";
  if (value >= 48) return "最近上涨和下跌方向的波动大致均衡";
  if (value > 40) return "最近下跌方向的波动略多";
  return "最近下跌方向的波动明显更多，价格表现偏弱";
}

function chaikinLabel(value: number): string {
  if (value >= 10) return "每天的高低价差明显变大，波动正在升温";
  if (value >= 3) return "每天的高低价差变大，波动正在升温";
  if (value > -3) return "每天的高低价差与前期接近，波动变化不大";
  if (value > -10) return "每天的高低价差变小，波动有所降温";
  return "每天的高低价差明显变小，波动正在降温";
}

function combinedMeaning(position: number | null, rvi: number | null, chaikin: number | null): string | null {
  if (position != null && position <= 20 && rvi != null && rvi < 48) {
    return chaikin != null && chaikin <= -3
      ? "整体更像弱势中的降温整理，不代表已经止跌"
      : "整体仍偏弱，低位本身不代表已经止跌";
  }
  if (position != null && position >= 80 && rvi != null && rvi > 52) {
    return chaikin != null && chaikin >= 3
      ? "整体更像强势中的加速波动，但是否能延续仍需用后续样本验证"
      : "整体偏强，但接近区间顶部不等于一定会继续上涨";
  }
  if (chaikin != null && chaikin <= -3) return "当前波动正在降温，策略信号可能会减少";
  if (chaikin != null && chaikin >= 3) return "当前波动正在升温，策略的收益和回撤都可能被放大";
  return null;
}

function atrAdjustment(value: number): string {
  if (value >= 3) {
    return "先统一风险：不要给所有股票使用同一个固定比例；按 ATR 分组，分别测试风险上限、退出距离和滑点。";
  }
  if (value < 1.5) {
    return "先检查退出条件：把固定比例和更小的 ATR 倍数分别回测，看看哪一种在低波动样本里更稳定。";
  }
  return "风险参数：用 ATR 倍数替代固定比例，再比较不同波动水平下的回撤差异。";
}

function positionAdjustment(position: number | null, rvi: number | null): string | null {
  if (position == null) return null;
  if (position <= 20 && rvi != null && rvi < 48) {
    return "不要把低位直接当成反转：给均值回归规则加上“重新回到正常区间”或“方向强弱指标（RVI）回到中线 50”条件，再比较误触发次数。";
  }
  if (position <= 20) {
    return "低位信号需要确认：对比“碰到底部”与“重新回到正常区间”两种条件，看哪一种更稳定。";
  }
  if (position >= 80 && rvi != null && rvi > 52) {
    return "如果研究突破策略：对比“碰到顶部”与“收盘越过顶部且 RVI 高于 50”，看哪一种在新样本里更可靠。";
  }
  if (position >= 80) {
    return "价格靠近顶部但方向不一致时，单独统计突破规则失败的次数，不要和强势突破样本混在一起。";
  }
  return "把价格在中部、底部和顶部的样本分开回测，避免所有位置共用同一个触发条件。";
}

function regimeAdjustment(value: number): string {
  if (value >= 3) {
    return "把波动变大的日子单独统计，分别看突破和均值回归的收益、回撤和换手。";
  }
  if (value <= -3) {
    return "把波动变小的日子单独统计，比较突破策略在平静期和活跃期的失败次数，再调整确认条件。";
  }
  return "把当前波动平稳的日子作为基准组，再和波动变大、变小的日子分别对照。";
}

function finiteNumber(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function median(values: number[]): number | null {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle];
}

function formatPercent(value: number): string {
  return `${formatNumber(value)}%`;
}
