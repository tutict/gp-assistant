import { ChevronDown, Download, LoaderCircle, Search } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { LlmModelCatalog } from "../../../hooks/useLlmSettingsDrafts";

interface Props {
  model: string; catalog?: LlmModelCatalog; disabled: boolean; canFetch: boolean; error?: string;
  onChange: (model: string) => void; onFetch: () => void | Promise<void>;
}
export function LlmModelPicker({ model, catalog, disabled, canFetch, error, onChange, onFetch }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const listId = useId() + "-models";
  const models = catalog?.models || [];
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return models.filter(item => [item.id, item.name, item.owned_by].some(value => value?.toLowerCase().includes(needle)));
  }, [models, query]);
  const currentIndex = Math.min(activeIndex, Math.max(0, filtered.length - 1));
  useEffect(() => { if (open) searchRef.current?.focus(); }, [open]);
  useEffect(() => {
    if (open && typeof document !== "undefined") document.getElementById?.(listId + "-" + currentIndex)?.scrollIntoView?.({ block: "nearest" });
  }, [open, currentIndex, listId]);
  useEffect(() => { if (!models.length && !catalog?.loaded) setOpen(false); }, [models.length, catalog?.loaded]);
  const close = () => { setOpen(false); toggleRef.current?.focus(); };
  const select = (value: string) => { onChange(value); close(); };
  const unavailable = catalog?.loaded && model && !models.some(item => item.id === model);
  return <div className="llm-field llm-model-field">
    <label htmlFor="llmModel">模型 ID</label>
    <div className="llm-model-control">
      <input id="llmModel" type="text" value={model} disabled={disabled}
        aria-invalid={Boolean(error)} aria-describedby="llmModelHint"
        onChange={event => onChange(event.target.value)} placeholder="填写模型 ID，或从列表选择" />
      <button ref={toggleRef} type="button" className="llm-model-action" disabled={disabled || (!models.length && !catalog?.loaded)}
        aria-label="选择供应商模型" aria-expanded={open} aria-controls={listId}
        onClick={() => { setQuery(""); setActiveIndex(0); setOpen(current => !current); }}>
        <ChevronDown size={16} aria-hidden="true" />
      </button>
      <button type="button" className={"llm-model-action" + (catalog?.loading ? " loading" : "")}
        aria-label="拉取供应商模型列表" disabled={disabled || !canFetch || catalog?.loading} onClick={onFetch}>
        {catalog?.loading ? <LoaderCircle size={16} aria-hidden="true" /> : <Download size={16} aria-hidden="true" />}
      </button>
    </div>
    <p id="llmModelHint" className={"llm-model-hint" + (error || catalog?.error ? " error" : unavailable ? " warn" : "")}>
      {error || (catalog?.loading ? "正在拉取模型列表…" : catalog?.error ? catalog.error.message + " 可继续手动填写模型 ID。"
        : unavailable ? "当前模型不在列表中，可保留手动填写的 ID。"
        : catalog?.loaded ? "已拉取 " + models.length + " 个模型，可搜索或直接输入模型 ID。" : "可直接输入模型 ID，或从供应商拉取模型列表。")}
    </p>
    {open && <div className="llm-model-menu">
      <div className="llm-model-search">
        <Search size={15} aria-hidden="true" />
        <input ref={searchRef} id="llmModelSearch" type="search" role="combobox" aria-label="搜索模型"
          aria-autocomplete="list" aria-expanded="true" aria-controls={listId}
          aria-activedescendant={filtered.length ? listId + "-" + currentIndex : undefined}
          value={query} placeholder="搜索 ID、名称或供应商"
          onChange={event => { setQuery(event.target.value); setActiveIndex(0); }}
          onKeyDown={event => {
            if (["ArrowDown", "ArrowUp", "Home", "End", "Enter", "Escape"].includes(event.key)) {
              event.preventDefault(); event.stopPropagation();
            }
            if (event.key === "Escape") close();
            else if (event.key === "Enter" && filtered[currentIndex]) select(filtered[currentIndex].id);
            else if (event.key === "ArrowDown") setActiveIndex((currentIndex + 1) % Math.max(1, filtered.length));
            else if (event.key === "ArrowUp") setActiveIndex((currentIndex - 1 + filtered.length) % Math.max(1, filtered.length));
            else if (event.key === "Home") setActiveIndex(0);
            else if (event.key === "End") setActiveIndex(Math.max(0, filtered.length - 1));
          }} />
        <span>{filtered.length}</span>
      </div>
      <div className="llm-model-options" id={listId} role="listbox" aria-label="供应商模型">
        {filtered.map((item, index) => <button key={item.id} id={listId + "-" + index} type="button" role="option"
          aria-selected={item.id === model} tabIndex={-1}
          className={"llm-model-option" + (index === currentIndex ? " focused" : "") + (item.id === model ? " selected" : "")}
          onMouseMove={() => setActiveIndex(index)} onClick={() => select(item.id)}>
          <strong>{item.id}</strong>{(item.name || item.owned_by) && <span>{[item.name, item.owned_by].filter(Boolean).join(" · ")}</span>}
        </button>)}
        {!filtered.length && <p className="llm-model-empty">没有匹配的模型，可继续手动填写 ID。</p>}
      </div>
    </div>}
  </div>;
}
