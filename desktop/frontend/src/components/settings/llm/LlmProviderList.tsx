import { ChevronDown, Plus } from "lucide-react";
import type { LlmProviderSettings } from "../../../types";
import type { LlmConnectionState } from "../../../hooks/useLlmSettingsDrafts";
import { isDraftDirty, LLM_PRESETS, providerKindLabel, type LlmProviderDraft } from "../../../lib/llmSettingsDraft";

interface Props {
  drafts: Record<string, LlmProviderDraft>; editingId: string; activeId?: string;
  connections: Record<string, LlmConnectionState>; disabled: boolean;
  onSelect: (id: string) => void; onAdd: (preset: Partial<LlmProviderSettings>) => void;
}
export function LlmProviderList({ drafts, editingId, activeId, connections, disabled, onSelect, onAdd }: Props) {
  return <aside className="llm-provider-sidebar" aria-label="模型连接列表">
    <div className="llm-list-heading"><strong>连接</strong><span>{Object.keys(drafts).length}</span></div>
    <p className="llm-list-hint">选择连接进行编辑</p>
    <div className="llm-provider-list">
      {Object.entries(drafts).map(([id, draft]) => {
        const provider = draft.provider;
        const connection = connections[id]?.revision === draft.revision ? connections[id] : undefined;
        const selected = editingId === id;
        return <button key={id} type="button" disabled={disabled}
          className={"llm-provider-card" + (selected ? " selected" : "")}
          aria-label={"编辑 " + (provider.name || "自定义连接")} aria-pressed={selected}
          onClick={() => onSelect(id)}>
          <span className="llm-provider-copy">
            <strong>{provider.name || "自定义连接"}</strong>
            <em>{providerKindLabel(provider.provider)} · {provider.model || "未选择模型"}</em>
          </span>
          <span className="llm-provider-badges">
            {activeId === id && <span className="llm-state-badge current">当前启用</span>}
            {isDraftDirty(draft) && <span className="llm-state-badge dirty">未保存</span>}
            <span className={"llm-state-badge " + (connection?.state || "untested")}>
              {connection?.state === "success" ? "测试通过" : connection?.state === "testing" ? "测试中"
                : connection?.state === "error" ? "测试失败" : "未测试"}
            </span>
          </span>
        </button>;
      })}
    </div>
    <details className="llm-presets">
      <summary>从常用预设新建<ChevronDown size={15} aria-hidden="true" /></summary>
      <div className="llm-preset-row">{LLM_PRESETS.map(preset =>
        <button key={preset.provider} type="button" className="llm-preset-btn" disabled={disabled} onClick={() => onAdd(preset)}>
          <Plus size={14} aria-hidden="true" />{preset.name}
        </button>)}</div>
    </details>
  </aside>;
}
