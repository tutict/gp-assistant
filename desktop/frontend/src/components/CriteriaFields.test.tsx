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

    expect(markup).toContain("股票池范围");
    expect(markup).toContain("科创板");
    expect(markup).toContain("深市 A 股");
    expect(markup).not.toContain("传媒");
  });
});
