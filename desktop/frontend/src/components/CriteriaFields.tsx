import { clampInt } from "../lib/format";
import { MARKET_SCOPE_OPTIONS, normalizeMarketScope } from "../lib/screenScopeOptions";
import { ALL_INDUSTRY_OPTIONS, isLegacyBroadIndustry } from "../lib/screenIndustryOptions";
import type { FilterCriteria } from "./FilterBar";

interface CriteriaFieldsProps {
  criteria: FilterCriteria;
  onChange: (criteria: FilterCriteria) => void;
  idPrefix?: string;
}

const PROFILE_OPTIONS = [
  { value: "balanced", label: "综合平衡" },
  { value: "quality", label: "稳健质量" },
  { value: "trend", label: "趋势轮动" },
];
const SORT_OPTIONS = [
  { value: "score", label: "综合评分" },
  { value: "market_cap", label: "市值" },
  { value: "pe", label: "市盈率" },
  { value: "pb", label: "市净率" },
  { value: "roe", label: "净资产收益率" },
  { value: "change_pct", label: "涨跌幅" },
];
export function CriteriaFields({ criteria, onChange, idPrefix = "criteria" }: CriteriaFieldsProps) {
  const update = (patch: Partial<FilterCriteria>) => onChange({ ...criteria, ...patch });
  const id = (name: string) => `${idPrefix}-${name}`;
  const selectedIndustry = criteria.industry.trim();
  const selectedMarketScope = normalizeMarketScope(criteria.marketScope);
  const baseIndustryOptions = ALL_INDUSTRY_OPTIONS;
  const industryOptions = selectedIndustry && !baseIndustryOptions.includes(selectedIndustry)
    ? [selectedIndustry, ...baseIndustryOptions]
    : baseIndustryOptions;

  return (
    <>
      <section className="criteria-field-group criteria-field-group-primary" aria-label="基础范围">
        <header>
          <strong>基础范围</strong>
          <span>限定行业、市场范围和结果规模</span>
        </header>

        <div className="criteria-field-grid">
          <div className="form-row">
            <label htmlFor={id("industry")}>行业</label>
            <select id={id("industry")} value={selectedIndustry} onChange={(event) => update({ industry: event.target.value })}>
              {industryOptions.map((industry) => (
                <option key={industry || "all-industries"} value={industry}>
                  {isLegacyBroadIndustry(industry) ? `${industry}（大类）` : industry || "全部行业"}
                </option>
              ))}
            </select>
          </div>

          <div className="form-row">
            <label htmlFor={id("marketScope")}>股票池范围</label>
            <select id={id("marketScope")} value={selectedMarketScope} onChange={(event) => update({ marketScope: event.target.value })}>
              {MARKET_SCOPE_OPTIONS.map((option) => (
                <option key={option.value || "all-scopes"} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>

          <div className="form-row criteria-num-field">
            <label htmlFor={id("resultLimit")}>返回数量</label>
            <input id={id("resultLimit")} type="number" min="1" max="200" value={criteria.resultLimit} onChange={(event) => update({ resultLimit: clampInt(event.target.value, 1, 200, 10) })} />
          </div>
        </div>
      </section>

      <section className="criteria-field-group" aria-label="估值与质量">
        <header>
          <strong>估值与质量</strong>
          <span>为空则不限制</span>
        </header>

        <div className="criteria-field-grid">
          <div className="form-row criteria-num-field">
            <label htmlFor={id("minRoe")}>最低 ROE (%)</label>
            <input id={id("minRoe")} type="number" min="-100" max="100" step="0.1" value={criteria.minRoe} onChange={(event) => update({ minRoe: event.target.value })} placeholder="如 15" />
          </div>

          <div className="form-row criteria-num-field">
            <label htmlFor={id("maxPe")}>最高 PE</label>
            <input id={id("maxPe")} type="number" min="0.1" step="0.1" value={criteria.maxPe} onChange={(event) => update({ maxPe: event.target.value })} placeholder="如 30" />
          </div>

          <div className="form-row criteria-num-field">
            <label htmlFor={id("maxPb")}>最高 PB</label>
            <input id={id("maxPb")} type="number" min="0.1" step="0.1" value={criteria.maxPb} onChange={(event) => update({ maxPb: event.target.value })} placeholder="如 5" />
          </div>

          <div className="form-row criteria-num-field">
            <label htmlFor={id("minMcap")}>最低市值 (亿)</label>
            <input id={id("minMcap")} type="number" min="0" step="1" value={criteria.minMcap} onChange={(event) => update({ minMcap: event.target.value })} placeholder="如 50" />
          </div>
        </div>
      </section>

      <section className="criteria-field-group criteria-field-group-secondary" aria-label="排序与偏好">
        <header>
          <strong>排序与偏好</strong>
          <span>控制展示顺序和过滤口径</span>
        </header>

        <div className="criteria-field-grid">
          <div className="form-row">
            <label htmlFor={id("scoreProfile")}>策略画像</label>
            <select id={id("scoreProfile")} value={criteria.scoreProfile || "balanced"} onChange={(event) => update({ scoreProfile: event.target.value })}>
              {PROFILE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>

          <div className="form-row">
            <label htmlFor={id("sortBy")}>排序字段</label>
            <select id={id("sortBy")} value={criteria.sortBy} onChange={(event) => update({ sortBy: event.target.value })}>
              {SORT_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>

          <div className="form-row criteria-sort-dir-field">
            <label id={id("sortDir-label")}>排序方向</label>
            <div className="segmented" role="group" aria-labelledby={id("sortDir-label")}>
              <button
                type="button"
                className={criteria.sortDir === "desc" ? "active" : ""}
                aria-pressed={criteria.sortDir === "desc"}
                onClick={() => update({ sortDir: "desc" })}
              >
                降序
              </button>
              <button
                type="button"
                className={criteria.sortDir === "asc" ? "active" : ""}
                aria-pressed={criteria.sortDir === "asc"}
                onClick={() => update({ sortDir: "asc" })}
              >
                升序
              </button>
            </div>
          </div>

          <div className="criteria-toggle-row">
            <label className={`toggle-chip ${criteria.includeSt ? "active" : ""}`}>
              <input type="checkbox" checked={criteria.includeSt} onChange={(event) => update({ includeSt: event.target.checked })} />
              <span>包含 ST</span>
            </label>
            <label className={`toggle-chip ${criteria.requireInstitutionBuyRatio ? "active" : ""}`}>
              <input type="checkbox" checked={criteria.requireInstitutionBuyRatio} onChange={(event) => update({ requireInstitutionBuyRatio: event.target.checked })} />
              <span>机构净买入</span>
            </label>
          </div>
        </div>
      </section>
    </>
  );
}
