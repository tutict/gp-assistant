import { clampInt } from "../lib/format";
import type { FilterCriteria } from "./FilterBar";

interface CriteriaFieldsProps {
  criteria: FilterCriteria;
  onChange: (criteria: FilterCriteria) => void;
  idPrefix?: string;
}

const INDUSTRIES = [
  "",
  "银行",
  "证券",
  "保险",
  "房地产开发",
  "半导体",
  "消费电子",
  "医药生物",
  "化学制药",
  "中药",
  "医疗器械",
  "食品饮料",
  "白酒",
  "家用电器",
  "汽车整车",
  "零部件",
  "电力设备",
  "光伏",
  "风电",
  "有色金属",
  "钢铁",
  "煤炭",
  "石油",
  "化工",
  "建材",
  "建筑装饰",
  "计算机",
  "软件",
  "通信",
  "传媒",
  "国防军工",
  "航空航天",
  "机械设备",
  "环保",
  "农业",
  "纺织服装",
  "商贸零售",
  "社会服务",
];


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

  return (
    <>
      <section className="criteria-field-group criteria-field-group-primary" aria-label="基础范围">
        <header>
          <strong>基础范围</strong>
          <span>限定行业和结果规模</span>
        </header>

        <div className="criteria-field-grid">
          <div className="form-row">
            <label htmlFor={id("industry")}>行业</label>
            <select id={id("industry")} value={criteria.industry} onChange={(event) => update({ industry: event.target.value })}>
              {INDUSTRIES.map((industry) => (
                <option key={industry} value={industry}>{industry || "全部行业"}</option>
              ))}
            </select>
          </div>

          <div className="form-row">
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
          <div className="form-row">
            <label htmlFor={id("minRoe")}>最低 ROE (%)</label>
            <input id={id("minRoe")} type="number" step="0.1" value={criteria.minRoe} onChange={(event) => update({ minRoe: event.target.value })} placeholder="如 15" />
          </div>

          <div className="form-row">
            <label htmlFor={id("maxPe")}>最高 PE</label>
            <input id={id("maxPe")} type="number" step="0.1" value={criteria.maxPe} onChange={(event) => update({ maxPe: event.target.value })} placeholder="如 30" />
          </div>

          <div className="form-row">
            <label htmlFor={id("maxPb")}>最高 PB</label>
            <input id={id("maxPb")} type="number" step="0.1" value={criteria.maxPb} onChange={(event) => update({ maxPb: event.target.value })} placeholder="如 5" />
          </div>

          <div className="form-row">
            <label htmlFor={id("minMcap")}>最低市值 (亿)</label>
            <input id={id("minMcap")} type="number" step="1" value={criteria.minMcap} onChange={(event) => update({ minMcap: event.target.value })} placeholder="如 50" />
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

          <div className="form-row">
            <label htmlFor={id("sortDir")}>排序方向</label>
            <select id={id("sortDir")} value={criteria.sortDir} onChange={(event) => update({ sortDir: event.target.value })}>
              <option value="desc">降序</option>
              <option value="asc">升序</option>
            </select>
          </div>

          <div className="criteria-toggle-row">
            <label>
              <input type="checkbox" checked={criteria.includeSt} onChange={(event) => update({ includeSt: event.target.checked })} />
              <span>包含ST股票</span>
            </label>
            <label>
              <input type="checkbox" checked={criteria.requireInstitutionBuyRatio} onChange={(event) => update({ requireInstitutionBuyRatio: event.target.checked })} />
              <span>机构净买入</span>
            </label>
          </div>
        </div>
      </section>
    </>
  );
}
