import { describe, expect, it } from "vitest";
import { selectionEvidence } from "./selectionEvidence";
import type { StockRowView } from "../types";

const row: StockRowView = {
  code: "688279.SH", name: "峰岹科技", quoteTime: "20261030161451",
  trendSignal: { code: "688279.SH", date: "2026-09-23", close: 215.42, k: 82.43, d: 73.38, j: 100.53,
    kdj_overbought: true, kdj_oversold: false, risk_flags: ["kdj_overbought"] },
};

describe("trend evidence dates", () => {
  it("keeps a risk-warning classification and its score summary out of supporting evidence", () => {
    const evidence = selectionEvidence({ ...row, reasons: ["signal_type:risk_warning", "quality_soft_bonus"],
      trendSignal: { ...row.trendSignal!, date: "2026-10-30", k: 60.13, d: 65.41, j: 49.57,
        kdj_overbought: false, signal_type: "risk_warning", risk_flags: ["weak_final_score"] },
      explanation: { basis: ["风险预警：技术分 27.03、形态分 7.20、质量分 81.00。"] },
    });
    expect(evidence.supports.every((line) => !line.startsWith("风险预警"))).toBe(true);
    expect(evidence.risks.some((line) => line.includes("风险预警：技术分 27.03"))).toBe(true);
  });

  it("distinguishes an outdated current screen from a deliberately historical screen", () => {
    expect(selectionEvidence({ ...row, analysisEndDate: "20261101" }).historyBehindQuote).toBe(true);
    const historical = selectionEvidence({ ...row, analysisEndDate: "20260923" });
    expect(historical.historyBehindQuote).toBe(false);
    expect(historical.risks.join(" ")).not.toContain("历史指标待更新");
    expect(historical.context).toContain("2026-09-23");
    expect(historical.context).toContain("2026-10-30");
  });
});
