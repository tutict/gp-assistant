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
      summary: "这只股票的历史行情还不完整，现在还看不出它是在高位、低位还是容易大涨大跌。",
      evidence: ["需要更多历史行情，才能判断价格位置和最近的涨跌幅度。"],
      adjustments: ["先补齐历史行情，再下结论；数据不全时不要急着改策略。"],
    };
  }

  const evidence: string[] = [];
  const adjustments: string[] = [];

  if (representativePosition != null) {
    evidence.push(`参考最近一段时间的高低价范围，${positionLabel(representativePosition, positions)}。`);
  }

  if (atrPercent != null) {
    evidence.unshift(`最近一天股价通常波动约 ${formatPercent(atrPercent)}，波动幅度${atrLabel(atrPercent)}。`);
    adjustments.push(atrAdjustment(atrPercent));
  }

  const directionEvidence = directionAndRegimeEvidence(rviValue, chaikinValue);
  if (directionEvidence) evidence.push(directionEvidence);

  const confirmationAdjustment = positionAdjustment(representativePosition, rviValue);
  if (confirmationAdjustment) adjustments.push(confirmationAdjustment);
  if (chaikinValue != null) adjustments.push(regimeAdjustment(chaikinValue));

  if (adjustments.length === 0) {
    adjustments.push("把价格在低位、中间和高位的情况分开看，再比较哪种情况更稳定。");
  }

  if (representativePosition == null && atrPercent == null && rviValue == null && chaikinValue == null) {
    evidence.push("目前只拿到少量数据，还不足以判断价格位置、涨跌方向或波动大小。");
    adjustments.unshift("先补齐历史行情，再调整策略；不要用不完整的数据做决定。");
  }

  return {
    returnedCount,
    summary: plainSummary(representativePosition, rviValue, chaikinValue, atrPercent),
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
  rviValue: number | null,
  chaikinValue: number | null,
): string | null {
  const parts: string[] = [];
  if (rviValue != null) {
    parts.push(rviLabel(rviValue));
  }
  if (chaikinValue != null) {
    parts.push(chaikinLabel(chaikinValue));
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
    return hasBelow && hasInside ? "价格靠近近期低位，而且已经低于部分常见范围" : "价格已经低于近期常见范围";
  }
  if (value <= 20) return "价格靠近近期低位";
  if (value < 40) return "价格偏低";
  if (value <= 60) return "价格处在近期高低价的中间";
  if (value < 80) return "价格偏高";
  if (value <= 100) return "价格靠近近期高位";
  return hasAbove && hasInside ? "价格靠近近期高位，而且已经高于部分常见范围" : "价格已经高于近期常见范围";
}

function rviLabel(value: number): string {
  if (value >= 60) return "最近上涨更占优势，走势偏强";
  if (value > 52) return "最近上涨略占优势";
  if (value >= 48) return "最近涨跌大致相当";
  if (value > 40) return "最近下跌略占优势";
  return "最近下跌更占优势，走势偏弱";
}

function chaikinLabel(value: number): string {
  if (value >= 10) return "每天的涨跌幅度明显变大，行情更容易大起大落";
  if (value >= 3) return "每天的涨跌幅度变大，行情开始变得活跃";
  if (value > -3) return "每天的涨跌幅度和前期差不多，行情比较平稳";
  if (value > -10) return "每天的涨跌幅度变小，行情正在变平静";
  return "每天的涨跌幅度明显变小，行情明显变平静";
}

function plainSummary(
  position: number | null,
  rvi: number | null,
  chaikin: number | null,
  atrPercent: number | null,
): string {
  if (position != null && position <= 20 && rvi != null && rvi < 48) {
    return chaikin != null && chaikin <= -3
      ? "价格在近期低位，最近下跌更明显，行情正在变平静，但还不能说明已经止跌。"
      : "价格在近期低位，最近下跌更明显，暂时还不能说明已经止跌。";
  }
  if (position != null && position >= 80 && rvi != null && rvi > 52) {
    return chaikin != null && chaikin >= 3
      ? "价格在近期高位，最近上涨更明显，但行情也更容易大起大落。"
      : "价格在近期高位，最近上涨更明显，但不代表一定会继续上涨。";
  }
  const parts: string[] = [];
  if (position != null) parts.push(positionLabel(position));
  if (rvi != null) parts.push(rviLabel(rvi));
  if (chaikin != null) parts.push(chaikinLabel(chaikin));
  if (atrPercent != null) parts.push(`最近一天通常波动约 ${formatPercent(atrPercent)}，幅度${atrLabel(atrPercent)}`);
  if (!parts.length) return "目前只能看到少量数字，还不足以判断这只股票最近的状态。";
  return `${parts.join("，")}。`;
}

function atrAdjustment(value: number): string {
  if (value >= 3) {
    return "最近波动较大，先把每次承担的风险调小，并单独检查退出位置，避免一次大波动造成过大损失。";
  }
  if (value < 1.5) {
    return "最近波动较小，可以分别测试更近和更远的退出位置，避免把正常的小波动误判成失败。";
  }
  return "把最近的波动大小放进风险和退出设置里，再比较哪种设置更稳定。";
}

function positionAdjustment(position: number | null, rvi: number | null): string | null {
  if (position == null) return null;
  if (position <= 20 && rvi != null && rvi < 48) {
    return "价格在低位时，先等它重新回到正常范围再判断是否转强，别因为价格低就当成要反弹。";
  }
  if (position <= 20) {
    return "价格在低位时，等它重新回到正常范围再判断，减少过早判断。";
  }
  if (position >= 80 && rvi != null && rvi > 52) {
    return "价格接近高位时，等它真正突破并站稳再判断，避免刚碰到高位就追进去。";
  }
  if (position >= 80) {
    return "价格接近高位时，把突破成功和失败分开记录，不要混在一起看。";
  }
  return "把价格在低位、中间和高位的情况分开统计，避免一套条件套所有情况。";
}

function regimeAdjustment(value: number): string {
  if (value >= 3) {
    return "把波动变大的日子单独看，比较这时上涨和回落哪一种更容易出现。";
  }
  if (value <= -3) {
    return "把行情变平静的日子单独看，确认策略是不是因为信号变少而失效。";
  }
  return "把现在这种平稳行情作为基准，再和剧烈波动的行情比较。";
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
