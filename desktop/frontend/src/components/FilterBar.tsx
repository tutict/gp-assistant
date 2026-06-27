import { useState } from "react";
import type { ScreenCriteria } from "../types";
import { formatPercent, formatNumber, clampInt } from "../lib/format";

interface FilterCriteria {
  includeSt: boolean;
  requireInstitutionBuyRatio: boolean;
  minRoe: string;
  maxPe: string;
  maxPb: string;
  minMcap: string;
  industry: string;
  resultLimit: number;
  sortBy: string;
  sortDir: string;
}

interface FilterBarProps {
  criteria: FilterCriteria;
  onChange: (criteria: FilterCriteria) => void;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
}

const INDUSTRIES = [
  "", "银行", "证券", "保险", "房地产开发", "半导体", "消费电子",
  "医药生物", "化学制药", "中药", "医疗器械", "食品饮料", "白酒",
  "家用电器", "汽车整车", "零部件", "电力设备", "光伏", "风电",
  "有色金属", "钢铁", "煤炭", "石油", "化工", "建材", "建筑装饰",
  "计算机", "软件", "通信", "传媒", "国防军工", "航空航天",
  "机械设备", "环保", "农业", "纺织服装", "商贸零售", "社会服务",
];

const SORT_OPTIONS = [
  { value: "score", label: "综合评分" },
  { value: "market_cap", label: "市值" },
  { value: "pe", label: "市盈率" },
  { value: "pb", label: "市净率" },
  { value: "roe", label: "ROE" },
  { value: "change_pct", label: "涨跌幅" },
];

export function FilterBar({ criteria, onChange, open, onToggle, onClose }: FilterBarProps) {
  const update = (patch: Partial<FilterCriteria>) => onChange({ ...criteria, ...patch });

  const summary = [
    criteria.industry ? `行业 ${criteria.industry}` : "全部行业",
    criteria.minRoe ? `ROE ≥ ${formatPercent(Number(criteria.minRoe))}` : "",
    criteria.maxPe ? `PE ≤ ${formatNumber(criteria.maxPe)}` : "",
    criteria.maxPb ? `PB ≤ ${formatNumber(criteria.maxPb)}` : "",
    criteria.minMcap ? `市值 ≥ ${formatNumber(criteria.minMcap)} 亿` : "",
    "扣非净利润 > 0",
    "扣非净利润增长率 > 10%",
    criteria.requireInstitutionBuyRatio ? "机构净买入" : "",
    `返回 ${clampInt(criteria.resultLimit, 1, 200, 10)} 只`,
  ].filter(Boolean).join(" · ");

  return (
    <>
      <div className="research-context-bar">
        <button type="button" className="criteria-toggle" onClick={onToggle}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M3 6h18M6 12h12M10 18h4" />
          </svg>
          <span>筛选条件</span>
        </button>
        <span className="criteria-summary">{summary}</span>
      </div>

      {open && <div className="criteria-overlay" onClick={onClose} />}

      <aside className={`criteria-panel ${open ? "open" : ""}`}>
        <div className="criteria-panel-header">
          <h2>研究条件</h2>
          <button type="button" className="criteria-close" onClick={onClose} aria-label="关闭">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="6" y1="6" x2="18" y2="18" />
              <line x1="18" y1="6" x2="6" y2="18" />
            </svg>
          </button>
        </div>

        <div className="criteria-panel-body">
          <div className="form-row">
            <label htmlFor="industry">行业</label>
            <select
              id="industry"
              value={criteria.industry}
              onChange={(e) => update({ industry: e.target.value })}
            >
              {INDUSTRIES.map((ind) => (
                <option key={ind} value={ind}>{ind || "全部行业"}</option>
              ))}
            </select>
          </div>

          <div className="form-row">
            <label htmlFor="minRoe">最低 ROE (%)</label>
            <input
              id="minRoe"
              type="number"
              step="0.1"
              value={criteria.minRoe}
              onChange={(e) => update({ minRoe: e.target.value })}
              placeholder="如 15"
            />
          </div>

          <div className="form-row">
            <label htmlFor="maxPe">最高 PE</label>
            <input
              id="maxPe"
              type="number"
              step="0.1"
              value={criteria.maxPe}
              onChange={(e) => update({ maxPe: e.target.value })}
              placeholder="如 30"
            />
          </div>

          <div className="form-row">
            <label htmlFor="maxPb">最高 PB</label>
            <input
              id="maxPb"
              type="number"
              step="0.1"
              value={criteria.maxPb}
              onChange={(e) => update({ maxPb: e.target.value })}
              placeholder="如 5"
            />
          </div>

          <div className="form-row">
            <label htmlFor="minMcap">最低市值 (亿)</label>
            <input
              id="minMcap"
              type="number"
              step="1"
              value={criteria.minMcap}
              onChange={(e) => update({ minMcap: e.target.value })}
              placeholder="如 50"
            />
          </div>

          <div className="form-row">
            <label htmlFor="resultLimit">返回数量</label>
            <input
              id="resultLimit"
              type="number"
              min="1"
              max="200"
              value={criteria.resultLimit}
              onChange={(e) => update({ resultLimit: clampInt(e.target.value, 1, 200, 10) })}
            />
          </div>

          <div className="form-row">
            <label htmlFor="sortBy">排序字段</label>
            <select
              id="sortBy"
              value={criteria.sortBy}
              onChange={(e) => update({ sortBy: e.target.value })}
            >
              {SORT_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>{opt.label}</option>
              ))}
            </select>
          </div>

          <div className="form-row">
            <label htmlFor="sortDir">排序方向</label>
            <select
              id="sortDir"
              value={criteria.sortDir}
              onChange={(e) => update({ sortDir: e.target.value })}
            >
              <option value="desc">降序</option>
              <option value="asc">升序</option>
            </select>
          </div>

          <div className="form-row checkbox-row">
            <label>
              <input
                type="checkbox"
                checked={criteria.includeSt}
                onChange={(e) => update({ includeSt: e.target.checked })}
              />
              包含 ST 股票
            </label>
          </div>

          <div className="form-row checkbox-row">
            <label>
              <input
                type="checkbox"
                checked={criteria.requireInstitutionBuyRatio}
                onChange={(e) => update({ requireInstitutionBuyRatio: e.target.checked })}
              />
              机构净买入
            </label>
          </div>
        </div>
      </aside>
    </>
  );
}

export type { FilterCriteria };
