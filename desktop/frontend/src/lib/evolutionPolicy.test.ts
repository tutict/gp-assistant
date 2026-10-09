import { describe, expect, it } from "vitest";
import { buildSentimentParameterProposal, type SentimentProposalInput } from "./evolutionPolicy";

const baseInput: SentimentProposalInput = {
  stage: "降温",
  marketRegime: "range",
  currentCriteria: {
    min_roe: 10,
    max_pe: 35,
    max_pb: 3,
    min_market_cap_billion: 50,
    min_deducted_net_profit_growth_rate: 10,
    limit: 20,
    include_st: false,
    market_scope: "沪深A股",
  },
  evidenceIds: ["E12", "E18"],
  evidenceSummary: "估值风险增加，市场热度下降。",
  backtest: {
    period: "60d",
    baseline: { returned: 20, maxDrawdown: 0.18, concentration: 0.42 },
    candidate: { returned: 14, maxDrawdown: 0.13, concentration: 0.31 },
  },
};

describe("sentiment parameter governance", () => {
  it("creates a bounded proposal and never changes protected fields", () => {
    const proposal = buildSentimentParameterProposal(baseInput);

    expect(proposal.stage).toBe("降温");
    expect(proposal.changes.some((change) => change.field === "max_pe")).toBe(true);
    expect(proposal.changes.every((change) => Math.abs(change.relativeChange) <= 0.2)).toBe(true);
    expect(proposal.protectedFields).toContain("include_st");
    expect(proposal.protectedFields).toContain("market_scope");
    expect(proposal.evidenceIds).toEqual(["E12", "E18"]);
    expect(proposal.requiresConfirmation).toBe(true);
  });

  it("uses a conservative no-evidence policy", () => {
    const proposal = buildSentimentParameterProposal({
      ...baseInput,
      stage: "证据不足",
      evidenceIds: [],
      evidenceSummary: "当前证据覆盖不足。",
    });

    expect(proposal.templateId).toBe("insufficient-evidence-v1");
    expect(proposal.changes).toHaveLength(0);
    expect(proposal.canSaveStrategy).toBe(false);
  });
});
