import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getJson } from "../lib/tauri";
import { hasMarketSuffix, inferMarketFromDigits, normalizeStockCode, sanitizeStockLookupInput, stockCodeDigits } from "../lib/format";
import type { StockItem } from "../types";

interface StockCodeInputProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  listMode?: boolean;
}

const MARKET_OPTIONS = [
  { key: "SH", label: "上海", hint: "600 / 688" },
  { key: "SZ", label: "深圳", hint: "000 / 300" },
  { key: "BJ", label: "北京", hint: "8 / 4" },
];

export function StockCodeInput({ id, value, onChange, placeholder, listMode = false }: StockCodeInputProps) {
  const [suggestions, setSuggestions] = useState<StockItem[]>([]);
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const requestId = useRef(0);
  const wrapperRef = useRef<HTMLDivElement>(null);

  const lookupToken = useMemo(() => {
    if (!listMode) return sanitizeStockLookupInput(value);
    const parts = String(value || "").split(/[,，;；\s]+/);
    return sanitizeStockLookupInput(parts[parts.length - 1] || "");
  }, [listMode, value]);
  const digits = stockCodeDigits(lookupToken);
  const showMarketConfirm = digits.length === 6 && !hasMarketSuffix(lookupToken);
  const suggestedMarket = showMarketConfirm ? inferMarketFromDigits(digits) : "";

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, []);

  useEffect(() => {
    const query = lookupToken.trim();
    if (!query || query.length < 2) {
      setSuggestions([]);
      return;
    }
    const current = ++requestId.current;
    setLoading(true);
    const timer = window.setTimeout(async () => {
      try {
        const params = new URLSearchParams({ q: query, limit: "6" });
        const items = await getJson<StockItem[]>(`/api/stock-search?${params}`);
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
  }, [lookupToken]);

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
  }, [applyValue]);

  const chooseMarket = useCallback((market: string) => {
    const code = normalizeStockCode(digits, market);
    if (code) applyValue(code);
    setOpen(false);
  }, [applyValue, digits]);

  return (
    <div className="stock-code-input" ref={wrapperRef}>
      <input
        id={id}
        type="text"
        value={value}
        onChange={(event) => { onChange(event.target.value); setOpen(true); }}
        onFocus={() => setOpen(true)}
        onKeyDown={(event) => {
          if (!open || !suggestions.length) return;
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setActiveIndex((index) => Math.min(index + 1, suggestions.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActiveIndex((index) => Math.max(index - 1, 0));
          } else if (event.key === "Enter") {
            event.preventDefault();
            chooseSuggestion(suggestions[activeIndex]);
          } else if (event.key === "Escape") {
            setOpen(false);
          }
        }}
        placeholder={placeholder || "600519.SH"}
        autoComplete="off"
        aria-autocomplete="list"
        aria-expanded={open}
      />
      {open && showMarketConfirm && (
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
      {open && (suggestions.length > 0 || loading) && (
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