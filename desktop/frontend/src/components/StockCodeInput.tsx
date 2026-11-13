import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getJson } from "../lib/tauri";
import type { Ref } from "react";
import { hasMarketSuffix, inferMarketFromDigits, normalizeStockCode, sanitizeStockLookupInput, stockCodeDigits } from "../lib/format";
import type { StockItem } from "../types";

interface StockCodeInputProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  listMode?: boolean;
  resolveBareCode?: boolean;
  market?: "cn" | "hk";
  inputRef?: Ref<HTMLInputElement>;
  inputAriaLabel?: string;
  inputAriaInvalid?: boolean;
  inputAriaDescribedBy?: string;
  onCommit?: (value: string) => void;
}

const MARKET_OPTIONS = [
  { key: "SH", label: "上海", hint: "600 / 688" },
  { key: "SZ", label: "深圳", hint: "000 / 300" },
  { key: "BJ", label: "北京", hint: "8 / 4" },
];

export function StockCodeInput({
  id,
  value,
  onChange,
  placeholder,
  listMode = false,
  resolveBareCode = false,
  market = "cn",
  inputRef,
  inputAriaLabel,
  inputAriaInvalid,
  inputAriaDescribedBy,
  onCommit,
}: StockCodeInputProps) {
  const [suggestions, setSuggestions] = useState<StockItem[]>([]);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const requestId = useRef(0);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const restoringInputFocusRef = useRef(false);

  const lookupToken = useMemo(() => {
    if (!listMode) return sanitizeStockLookupInput(value);
    const parts = String(value || "").split(/[,，;；\s]+/);
    return sanitizeStockLookupInput(parts[parts.length - 1] || "");
  }, [listMode, value]);
  const digits = stockCodeDigits(lookupToken);
  const completeCode = hasMarketSuffix(lookupToken);
  const exactSuggestion = useMemo(() => {
    if (digits.length !== 6) return null;
    return suggestions.find((item) => stockCodeDigits(item.code) === digits) || null;
  }, [digits, suggestions]);
  const hasExactSuggestion = Boolean(exactSuggestion);
  const showStockSuggest = open && !completeCode && (suggestions.length > 0 || loading);
  const showMarketConfirm = market === "cn" && !resolveBareCode && open && digits.length === 6 && !hasMarketSuffix(lookupToken) && !loading && !hasExactSuggestion && suggestions.length === 0;
  const suggestedMarket = showMarketConfirm ? inferMarketFromDigits(digits) : "";

  useEffect(() => {
    if (typeof document === "undefined") return;
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, []);

  useEffect(() => {
    const query = lookupToken.trim();
    if (!query || query.length < 2 || completeCode) {
      setSuggestions([]);
      setLoading(false);
      if (completeCode) setOpen(false);
      return;
    }
    const current = ++requestId.current;
    setLoading(true);
    const timer = window.setTimeout(async () => {
      try {
        const params = new URLSearchParams({ q: query, limit: "6" });
        const items = market === "hk"
          ? (await getJson<{ items?: Array<{ code: string; name: string; name_zh?: string }> }>(`/api/hk/stock-search?${params}`)).items?.map((item) => ({
            code: normalizeStockCode(item.code, "HK"),
            name: item.name_zh || item.name || item.code,
            industry: "港股",
          })).filter((item) => Boolean(item.code)) || []
          : await getJson<StockItem[]>(`/api/stock-search?${params}`);
        if (current === requestId.current) {
          setSuggestions(items || []);
          setActiveIndex(0);
          setOpen(true);
        }
      } catch {
        if (current === requestId.current) setSuggestions([]);
      } finally {
        if (current === requestId.current) setLoading(false);
      }
    }, 180);
    return () => window.clearTimeout(timer);
  }, [completeCode, lookupToken, market]);

  const applyValue = useCallback((next: string) => {
    if (!listMode) {
      onChange(next);
      return;
    }
    const parts = String(value || "").split(/([,，;；\s]+)/);
    let lastTextIndex = -1;
    for (let index = parts.length - 1; index >= 0; index -= 1) {
      if (!/^[,，;；\s]+$/.test(parts[index] || "")) {
        lastTextIndex = index;
        break;
      }
    }
    if (lastTextIndex >= 0) parts[lastTextIndex] = next;
    else parts.push(next);
    onChange(parts.join(""));
  }, [listMode, onChange, value]);

  const chooseSuggestion = useCallback((item: StockItem) => {
    applyValue(item.code);
    setOpen(false);
    onCommit?.(item.code);
  }, [applyValue, onCommit]);

  const chooseMarket = useCallback((market: string) => {
    const code = normalizeStockCode(digits, market);
    if (code) applyValue(code);
    setOpen(false);
    if (code) onCommit?.(code);
  }, [applyValue, digits, onCommit]);

  return (
    <div className="stock-code-input" ref={wrapperRef} onKeyDown={(event) => {
      if (event.key !== "Escape" || (!showStockSuggest && !showMarketConfirm)) return;
      event.preventDefault();
      event.stopPropagation();
      requestId.current += 1;
      if (loading) setSuggestions([]);
      setLoading(false);
      setOpen(false);
      restoringInputFocusRef.current = true;
      wrapperRef.current?.querySelector<HTMLInputElement>("input")?.focus();
      restoringInputFocusRef.current = false;
    }}>
      <input
        ref={inputRef}
        id={id}
        type="text"
        value={value}
        onChange={(event) => {
          const next = event.target.value;
          const deletingResolvedSuffix = value.length > next.length && value.startsWith(next);
          const bareCodePattern = market === "hk" ? /^[0-9]{5}$/ : /^[0-9]{6}$/;
          const resolved = resolveBareCode && !listMode && !deletingResolvedSuffix && bareCodePattern.test(next.trim())
            ? normalizeStockCode(next, market === "hk" ? "HK" : undefined)
            : "";
          if (resolved) {
            requestId.current += 1;
            setSuggestions([]);
            setLoading(false);
            setOpen(false);
            onChange(resolved);
            return;
          }
          onChange(next);
          setOpen(true);
        }}
        onFocus={() => { if (!restoringInputFocusRef.current) setOpen(!completeCode); }}
        onKeyDown={(event) => {
          if (event.key === "Enter" && completeCode && !listMode) {
            event.preventDefault();
            setOpen(false);
            onCommit?.(value.trim());
            return;
          }
          if (!open || completeCode || !suggestions.length) return;
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setActiveIndex((index) => Math.min(index + 1, suggestions.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActiveIndex((index) => Math.max(index - 1, 0));
          } else if (event.key === "Enter") {
            event.preventDefault();
            chooseSuggestion(suggestions[activeIndex]);
          }
        }}
        placeholder={placeholder || "600519.SH"}
        autoComplete="off"
        aria-label={inputAriaLabel}
        aria-invalid={inputAriaInvalid}
        aria-describedby={inputAriaDescribedBy}
        aria-autocomplete="list"
        aria-expanded={open}
      />
      {showMarketConfirm && (
        <div className="market-confirm" role="group" aria-label="选择市场">
          {MARKET_OPTIONS.map((market) => (
            <button
              key={market.key}
              type="button"
              className={market.key === suggestedMarket ? "suggested" : ""}
              onClick={() => chooseMarket(market.key)}
            >
              <strong>{market.label}</strong>
              <span>{market.hint}</span>
            </button>
          ))}
        </div>
      )}
      {showStockSuggest && (
        <div className="stock-suggest" role="listbox">
          {loading && <div className="stock-suggest-empty">搜索中...</div>}
          {!loading && suggestions.map((item, index) => (
            <button
              key={`${item.code}-${index}`}
              type="button"
              className={index === activeIndex ? "active" : ""}
              role="option"
              aria-selected={index === activeIndex}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => chooseSuggestion(item)}
            >
              <strong>{item.name || item.code}</strong>
              <span>{item.code} {item.industry || ""}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
