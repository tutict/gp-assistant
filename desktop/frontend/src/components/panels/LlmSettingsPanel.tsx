import { useCallback, useState } from "react";
import type { LlmSettings } from "../../types";
import { escapeHtml } from "../../lib/format";

function normalizeBaseUrl(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return "";
  return trimmed.replace(/\/+$/, "");
}

interface LlmSettingsPanelProps {
  settings: LlmSettings | null;
  onChange: (settings: LlmSettings | null) => void;
}

export function LlmSettingsPanel({ settings, onChange }: LlmSettingsPanelProps) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<{ text: string; state: string }>({ text: "", state: "neutral" });

  const current = settings || {
    api_key: "",
    base_url: "",
    model: "",
    temperature: 0.7,
    timeout: 60,
    json_mode: false,
    remember_key: false,
  };

  const update = useCallback((patch: Partial<LlmSettings>) => {
    onChange({ ...current, ...patch });
  }, [current, onChange]);

  const save = useCallback(() => {
    setStatus({ text: "已保存", state: "success" });
    setTimeout(() => setStatus({ text: "", state: "neutral" }), 2000);
  }, []);

  const clear = useCallback(() => {
    onChange(null);
    setStatus({ text: "已清除", state: "neutral" });
    setTimeout(() => setStatus({ text: "", state: "neutral" }), 2000);
  }, [onChange]);

  return (
    <div className={`llm-settings-panel ${open ? "open" : ""}`}>
      <div className="llm-settings-header">
        <button
          type="button"
          className="llm-settings-toggle"
          onClick={() => setOpen(!open)}
        >
          LLM 设置 {status.text && <span className={`llm-status ${status.state}`}>{status.text}</span>}
        </button>
      </div>

      {open && (
        <div className="llm-settings-body">
          <div className="form-row">
            <label htmlFor="llmApiKey">API Key</label>
            <input
              id="llmApiKey"
              type="password"
              value={current.api_key || ""}
              onChange={(e) => update({ api_key: e.target.value })}
              placeholder="sk-..."
            />
          </div>

          <div className="form-row">
            <label htmlFor="llmBaseUrl">Base URL</label>
            <input
              id="llmBaseUrl"
              type="url"
              value={current.base_url || ""}
              onChange={(e) => update({ base_url: normalizeBaseUrl(e.target.value) })}
              placeholder="https://api.openai.com/v1"
            />
          </div>

          <div className="form-row">
            <label htmlFor="llmModel">模型</label>
            <input
              id="llmModel"
              type="text"
              value={current.model || ""}
              onChange={(e) => update({ model: e.target.value })}
              placeholder="gpt-4o-mini"
            />
          </div>

          <div className="form-row">
            <label htmlFor="llmTemperature">温度</label>
            <input
              id="llmTemperature"
              type="number"
              step="0.1"
              min="0"
              max="2"
              value={current.temperature ?? 0.7}
              onChange={(e) => update({ temperature: Number(e.target.value) })}
            />
          </div>

          <div className="form-row">
            <label htmlFor="llmTimeout">超时 (秒)</label>
            <input
              id="llmTimeout"
              type="number"
              min="10"
              max="300"
              value={current.timeout ?? 60}
              onChange={(e) => update({ timeout: Number(e.target.value) })}
            />
          </div>

          <div className="form-row checkbox-row">
            <label>
              <input
                type="checkbox"
                checked={current.json_mode ?? false}
                onChange={(e) => update({ json_mode: e.target.checked })}
              />
              JSON 模式
            </label>
          </div>

          <div className="form-row checkbox-row">
            <label>
              <input
                type="checkbox"
                checked={current.remember_key ?? false}
                onChange={(e) => update({ remember_key: e.target.checked })}
              />
              记住 API Key
            </label>
          </div>

          <div className="form-actions">
            <button type="button" className="save-btn" onClick={save}>保存</button>
            <button type="button" className="clear-btn" onClick={clear}>清除</button>
          </div>
        </div>
      )}
    </div>
  );
}
