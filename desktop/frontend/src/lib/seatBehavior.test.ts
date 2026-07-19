import { describe, expect, it } from "vitest";
import { buildSeatBehaviorViews } from "./seatBehavior";

describe("buildSeatBehaviorViews", () => {
  it("classifies an institution seat and explains a two-sided listing cautiously", () => {
    const [view] = buildSeatBehaviorViews({
      category: "institution_lhb",
      seats: [{
        seat_code: "A",
        name: "机构专用",
        buy_amount: 100_000_000,
        sell_amount: 30_000_000,
        net_amount: 70_000_000,
        direction: "both",
      }],
    });

    expect(view.typeLabel).toBe("机构席位");
    expect(view.directionLabel).toBe("双向 · 净买");
    expect(view.amountLabel).toBe("买 1 亿 / 卖 3000 万");
    expect(view.tactic).toBe("日内双向换手");
    expect(view.explanation).toContain("无法确认是否由量化程序执行");
  });

  it("describes a near-limit-up buy seat as a pattern rather than an identity", () => {
    const [view] = buildSeatBehaviorViews({
      seats: [{
        seat_code: "B",
        name: "某证券股份有限公司上海营业部",
        buy_amount: 50_000_000,
        direction: "buy",
        change_rate: 9.92,
        three_day_rise_probability: 0.625,
      }],
    });

    expect(view.typeLabel).toBe("营业部席位");
    expect(view.tactic).toBe("打板 / 接力");
    expect(view.explanation).toContain("不能据此确认账户");
    expect(view.stats).toContain("62.5%");
  });

  it("uses the public sell direction and never invents a one-sided net amount", () => {
    const [view] = buildSeatBehaviorViews({
      seats: [{
        name: "某证券股份有限公司深圳营业部",
        sell_amount: 80_000_000,
        direction: "sell",
        change_rate: 6.2,
      }],
    });

    expect(view.directionLabel).toBe("卖方上榜");
    expect(view.amountLabel).toBe("8000 万");
    expect(view.tactic).toBe("冲高兑现");
    expect(view.tone).toBe("negative");
  });

  it("keeps every public seat instead of silently truncating the list", () => {
    const seats = Array.from({ length: 7 }, (_, index) => ({
      seat_code: String(index),
      name: `某证券营业部 ${index + 1}`,
      buy_amount: 1_000_000 + index,
      direction: "buy" as const,
    }));

    expect(buildSeatBehaviorViews({ seats })).toHaveLength(7);
  });
});
