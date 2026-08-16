import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CriteriaFields } from "./CriteriaFields";
import type { FilterCriteria } from "./FilterBar";

const criteria: FilterCriteria = {
  includeSt: false,
  requireInstitutionBuyRatio: false,
  minRoe: "",
  maxPe: "",
  maxPb: "",
  minMcap: "",
  industry: "",
  marketScope: "",
  resultLimit: 10,
  sortBy: "score",
  sortDir: "desc",
  scoreProfile: "balanced",
};

describe("CriteriaFields", () => {
  it("offers only market scopes that match the current cached stock field", () => {
    const markup = renderToStaticMarkup(
      <CriteriaFields criteria={criteria} onChange={() => undefined} idPrefix="customScreen" />,
    );

    expect(markup).toContain("行业");
    expect(markup).toContain("股票池范围");
    expect(markup).toContain("科创板");
    expect(markup).toContain("北交所");
    expect(markup).toContain("影视院线");
    expect(markup).toContain("传媒（大类）");
    expect(markup).toContain("医药生物（大类）");
    expect(markup).toContain("食品饮料（大类）");
    expect(markup).toContain("社会服务（大类）");
  });
});
