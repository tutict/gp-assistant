import type { FilterCriteria } from "../components/FilterBar";
import { normalizeMarketScope } from "./screenScopeOptions";

export const DEFAULT_FILTER_CRITERIA: FilterCriteria = {
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

export function sanitizeFilterCriteria(value: FilterCriteria): FilterCriteria {
  const source = value && typeof value === "object"
    ? { ...DEFAULT_FILTER_CRITERIA, ...value }
    : DEFAULT_FILTER_CRITERIA;
  const rawIndustry = typeof source.industry === "string" ? source.industry.trim() : "";
  const legacyMarketScope = normalizeMarketScope(rawIndustry);

  return {
    ...source,
    industry: legacyMarketScope ? "" : rawIndustry,
    marketScope: normalizeMarketScope(source.marketScope) || legacyMarketScope,
  };
}
