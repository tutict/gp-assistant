import type { CapitalEvidenceItem, CapitalEvidenceSeat } from "../types";

export type SeatBehaviorTone = "positive" | "negative" | "warning" | "neutral";

export interface SeatBehaviorView {
  key: string;
  name: string;
  typeLabel: string;
  directionLabel: string;
  amountLabel: string;
  tactic: string;
  explanation: string;
  stats: string;
  tone: SeatBehaviorTone;
}

type Direction = "buy" | "sell" | "both" | "unknown";

export function buildSeatBehaviorViews(item?: CapitalEvidenceItem | null): SeatBehaviorView[] {
  return (item?.seats || [])
    .map((seat, index) => buildSeatView(seat, index))
    .filter((seat): seat is SeatBehaviorView => Boolean(seat));
}

function buildSeatView(seat: CapitalEvidenceSeat, index: number): SeatBehaviorView | null {
  const name = text(seat.name);
  if (!name) return null;
  const buy = number(seat.buy_amount);
  const sell = number(seat.sell_amount);
  const net = number(seat.net_amount);
  const direction = getDirection(text(seat.direction), buy, sell);
  const behavior = inferBehavior(direction, number(seat.change_rate), text(seat.reason));
  return {
    key: text(seat.seat_code) || name + "-" + index,
    name,
    typeLabel: seatType(name),
    directionLabel: directionText(direction, net),
    amountLabel: amountText(direction, buy, sell),
    tactic: behavior.tactic,
    explanation: behavior.explanation,
    stats: stats(number(seat.three_day_activity_count), number(seat.three_day_rise_probability)),
    tone: behaviorTone(direction, net),
  };
}

function seatType(name: string): string {
  if (name.includes("机构专用")) return "机构席位";
  if (/沪股通专用|深股通专用/.test(name)) return "互联互通";
  if (/总部|证券自营/.test(name)) return "券商总部";
  return "营业部席位";
}

function getDirection(value: string, buy: number | null, sell: number | null): Direction {
  if (value === "buy" || value === "sell" || value === "both") return value;
  return buy != null && sell != null ? "both" : buy != null ? "buy" : sell != null ? "sell" : "unknown";
}

function directionText(direction: Direction, net: number | null): string {
  if (direction === "both") return net == null || Math.abs(net) < 0.01
    ? "双向近持平" : net > 0 ? "双向 · 净买" : "双向 · 净卖";
  return direction === "buy" ? "买方上榜" : direction === "sell" ? "卖方上榜" : "方向暂缺";
}

function amountText(direction: Direction, buy: number | null, sell: number | null): string {
  if (direction === "both") return "买 " + money(buy) + " / 卖 " + money(sell);
  if (direction === "buy") return money(buy);
  if (direction === "sell") return money(sell);
  return "金额暂缺";
}

function behaviorTone(direction: Direction, net: number | null): SeatBehaviorTone {
  if (direction === "buy") return "positive";
  if (direction === "sell") return "negative";
  if (direction === "both" && net != null) {
    return net > 0 ? "positive" : net < 0 ? "negative" : "warning";
  }
  return direction === "both" ? "warning" : "neutral";
}

function inferBehavior(direction: Direction, change: number | null, reason: string) {
  if (direction === "both") return {
    tactic: "日内双向换手",
    explanation: "同一公开席位进入买卖两侧榜单，常见于日内回转、分仓或做T；无法确认是否由量化程序执行。",
  };
  if (direction === "buy" && change != null && change >= 9.5) return {
    tactic: "打板 / 接力",
    explanation: "股价接近涨停且席位位于买方，形态接近打板或接力；不能据此确认账户的实际策略。",
  };
  if (direction === "buy" && change != null && change <= -3) return {
    tactic: "逆势低吸",
    explanation: "下跌日仍有公开席位买入，形态接近逆势低吸；后续是否形成承接仍需观察。",
  };
  if (direction === "buy" && reason.includes("换手率")) return {
    tactic: "高换手承接",
    explanation: "高换手上榜同时出现席位买入，可能是分歧中的承接或短线抢筹。",
  };
  if (direction === "buy") return {
    tactic: "顺势抢筹 / 试仓",
    explanation: "公开席位进入买方榜，形态更接近顺势抢筹或试仓，需看次日是否延续。",
  };
  if (direction === "sell" && change != null && change >= 5) return {
    tactic: "冲高兑现",
    explanation: "上涨日席位进入卖方榜，常见于冲高兑现；无法判断是否仍有剩余仓位。",
  };
  if (direction === "sell" && change != null && change <= -5) return {
    tactic: "弱势撤退",
    explanation: "下跌日席位进入卖方榜，形态接近止损或撤退，短线抛压需要继续观察。",
  };
  if (direction === "sell") return {
    tactic: "减仓兑现",
    explanation: "公开席位进入卖方榜，更接近减仓或兑现；单日榜单无法还原完整持仓。",
  };
  return { tactic: "等待确认", explanation: "公开席位方向数据不完整，暂不推断交易手法。" };
}

function stats(count: number | null, probability: number | null): string {
  const parts: string[] = [];
  if (count != null) parts.push("三日统计样本 " + decimal(count));
  if (probability != null) {
    const percent = Math.abs(probability) <= 1 ? probability * 100 : probability;
    parts.push("三日上涨概率 " + decimal(percent) + "%");
  }
  return parts.join(" · ");
}

function money(value: number | null): string {
  if (value == null) return "暂缺";
  if (Math.abs(value) >= 100_000_000) return decimal(value / 100_000_000) + " 亿";
  if (Math.abs(value) >= 10_000) return decimal(value / 10_000) + " 万";
  return decimal(value);
}

function decimal(value: number): string {
  return value.toFixed(2).replace(/\.00$/, "").replace(/(\.\d)0$/, "$1");
}

function number(value: unknown): number | null {
  if (value == null || value === "") return null;
  const parsed = typeof value === "number" ? value : Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}
