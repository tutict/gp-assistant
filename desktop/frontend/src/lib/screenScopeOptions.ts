export interface ScreenScopeOption {
  value: string;
  label: string;
}

export const MARKET_SCOPE_OPTIONS: ScreenScopeOption[] = [
  { value: "", label: "全部范围" },
  { value: "沪市A股", label: "沪市 A 股" },
  { value: "深市A股", label: "深市 A 股" },
  { value: "创业板", label: "创业板" },
  { value: "科创板", label: "科创板" },
  { value: "北交所", label: "北交所" },
];

const SUPPORTED_MARKET_SCOPE_VALUES = new Set(
  MARKET_SCOPE_OPTIONS.map((option) => option.value).filter(Boolean),
);

export function normalizeMarketScope(value: unknown): string {
  if (typeof value !== "string") return "";
  const trimmed = value.trim();
  return SUPPORTED_MARKET_SCOPE_VALUES.has(trimmed) ? trimmed : "";
}
