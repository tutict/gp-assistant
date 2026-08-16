import { describe, expect, it } from "vitest";
import { sanitizeFilterCriteria } from "./screenCriteria";

describe("screen criteria persistence", () => {
  it("migrates legacy market labels out of industry", () => {
    const criteria = sanitizeFilterCriteria({
      includeSt: false,
      requireInstitutionBuyRatio: false,
      minRoe: "",
      maxPe: "",
      maxPb: "",
      minMcap: "",
      industry: "科创板",
      marketScope: "",
      resultLimit: 10,
      sortBy: "score",
      sortDir: "desc",
      scoreProfile: "balanced",
    });

    expect(criteria.industry).toBe("");
    expect(criteria.marketScope).toBe("科创板");
  });

  it("preserves a real industry and an explicit market scope", () => {
    const criteria = sanitizeFilterCriteria({
      includeSt: false,
      requireInstitutionBuyRatio: false,
      minRoe: "",
      maxPe: "",
      maxPb: "",
      minMcap: "",
      industry: "影视院线",
      marketScope: "北交所",
      resultLimit: 10,
      sortBy: "score",
      sortDir: "desc",
      scoreProfile: "balanced",
    });

    expect(criteria.industry).toBe("影视院线");
    expect(criteria.marketScope).toBe("北交所");
  });
});
