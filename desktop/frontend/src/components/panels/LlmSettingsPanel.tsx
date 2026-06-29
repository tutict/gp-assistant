import { useCallback, useMemo, useState } from "react";
import type { LlmProviderSettings, LlmSettings } from "../../types";
import { activeLlmProvider, normalizeLlmSettings } from "../../lib/contracts";

const PROVIDER_PRESETS = [
  {
    id: "openai",
    name: "OpenAI",
    provider: "openai",
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    json_mode: false,
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    provider: "deepseek",
    base_url: "https://api.deepseek.com/v1",
    model: "deepseek-chat",
    json_mode: false,
  },
  {
    id: "anthropic",
    name: "Anthropic 兼容",
    provider: "anthropic",
    base_url: "https://api.anthropic.com/v1",
    model: "claude-3-5-sonnet-latest",
    json_mode: false,
  },
  {
    id: "local",
    name: "本地兼容",
    provider: "local",
    base_url: "http://127.0.0.1:11434/v1",
    model: "qwen2.5:7b",
    json_mode: false,
  },
] as const;

interface LlmSettingsPanelProps {
  settings: LlmSettings | null;
  onChange: (settings: LlmSettings | null) => void;
}

function providerId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

function normalizeBaseUrl(url: string): string {
  const trimmed = url.trim();
  if (!trimmed) return "";
  return trimmed.replace(/\/+$/, "");
}

function maskKey(value?: string): string {
  if (!value) return "未配置";
  if (value.length <= 10) return "已配置";
  return `${value.slice(0, 5)}...${value.slice(-4)}`;
}

export function LlmSettingsPanel({ settings, onChange }: LlmSettingsPanelProps) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<{ text: string; state: string }>({ text: "", state: "neutral" });
  const normalized = useMemo(() => normalizeLlmSettings(settings), [settings]);
  const active = activeLlmProvider(settings);
  const providers = normalized.providers || [];
  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const editingProvider = providers.find((provider) => provider.id === editingProviderId) || active || providers[0];

  const commit = useCallback((next: LlmSettings) => {
    onChange(next);
  }, [onChange]);

  const showStatus = useCallback((text: string, state = "success") => {
    setStatus({ text, state });
    window.setTimeout(() => setStatus({ text: "", state: "neutral" }), 2000);
  }, []);

  const activateProvider = useCallback((id?: string) => {
    if (!id) return;
    commit({ ...normalized, active_provider_id: id });
    showStatus("已切换");
  }, [commit, normalized, showStatus]);

  const updateProvider = useCallback((id: string | undefined, patch: Partial<LlmProviderSettings>) => {
    if (!id) return;
    commit({
      ...normalized,
      providers: providers.map((provider) => provider.id === id ? { ...provider, ...patch } : provider),
    });
  }, [commit, normalized, providers]);

  const addPreset = useCallback((preset: typeof PROVIDER_PRESETS[number]) => {
    const nextProvider: LlmProviderSettings = {
      ...preset,
      id: providerId(preset.id),
      temperature: 0.7,
      timeout: 60,
      remember_key: false,
    };
    commit({
      active_provider_id: nextProvider.id,
      providers: [...providers, nextProvider],
    });
    setEditingProviderId(nextProvider.id || null);
    setOpen(true);
    showStatus("已添加");
  }, [commit, providers, showStatus]);

  const removeProvider = useCallback((id?: string) => {
    if (!id || providers.length <= 1) return;
    const nextProviders = providers.filter((provider) => provider.id !== id);
    const activeProviderId = normalized.active_provider_id === id ? nextProviders[0]?.id : normalized.active_provider_id;
    commit({ active_provider_id: activeProviderId, providers: nextProviders });
    if (editingProviderId === id) setEditingProviderId(activeProviderId || null);
    showStatus("已删除", "neutral");
  }, [commit, editingProviderId, normalized.active_provider_id, providers, showStatus]);

  const clearAll = useCallback(() => {
    onChange(null);
    setEditingProviderId(null);
    showStatus("已清除", "neutral");
  }, [onChange, showStatus]);

  const save = useCallback(() => {
    commit(normalized);
    showStatus("已保存");
  }, [commit, normalized, showStatus]);

  return (
    <div className={`llm-settings-panel ${open ? "open" : ""}`}>
      <div className="llm-settings-header">
        <button
          type="button"
          className="llm-settings-toggle"
          onClick={() => setOpen(!open)}
        >
          <span>模型连接</span>
          <strong>{active?.name || "未配置"}</strong>
          {status.text && <span className={`llm-status ${status.state}`}>{status.text}</span>}
        </button>
      </div>

      {open && (
        <div className="llm-settings-body llm-switcher">
          <div className="llm-switcher-grid">
            <div className="llm-provider-list" aria-label="模型连接列表">
              {providers.map((provider) => {
                const selected = provider.id === normalized.active_provider_id;
                return (
                  <button
                    key={provider.id}
                    type="button"
                    className={`llm-provider-card ${selected ? "active" : ""}`}
                    onClick={() => {
                      activateProvider(provider.id);
                      setEditingProviderId(provider.id || null);
                    }}
                  >
                    <span>
                      <strong>{provider.name || provider.provider || "自定义"}</strong>
                      <em>{provider.model || "未选择模型"}</em>
                    </span>
                    <b>{selected ? "启用中" : "启用"}</b>
                  </button>
                );
              })}
            </div>

            <div className="llm-provider-editor">
              {editingProvider && (
                <>
                  <div className="llm-editor-head">
                    <div>
                      <span>当前连接</span>
                      <strong>{editingProvider.name || "自定义"}</strong>
                    </div>
                    <button
                      type="button"
                      className="clear-btn"
                      onClick={() => removeProvider(editingProvider.id)}
                      disabled={providers.length <= 1}
                    >
                      删除
                    </button>
                  </div>

                  <div className="form-row">
                    <label htmlFor="llmProviderName">名称</label>
                    <input
                      id="llmProviderName"
                      type="text"
                      value={editingProvider.name || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { name: event.target.value })}
                      placeholder="例如 OpenAI / DeepSeek / 公司网关"
                    />
                  </div>

                  <div className="form-row">
                    <label htmlFor="llmBaseUrl">接口地址</label>
                    <input
                      id="llmBaseUrl"
                      type="url"
                      value={editingProvider.base_url || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { base_url: normalizeBaseUrl(event.target.value) })}
                      placeholder="https://api.openai.com/v1"
                    />
                  </div>

                  <div className="form-row">
                    <label htmlFor="llmModel">模型</label>
                    <input
                      id="llmModel"
                      type="text"
                      value={editingProvider.model || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { model: event.target.value })}
                      placeholder="gpt-4o-mini"
                    />
                  </div>

                  <div className="form-row">
                    <label htmlFor="llmApiKey">API 密钥</label>
                    <input
                      id="llmApiKey"
                      type="password"
                      value={editingProvider.api_key || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { api_key: event.target.value })}
                      placeholder={maskKey(editingProvider.api_key)}
                    />
                  </div>

                  <div className="llm-editor-row">
                    <div className="form-row">
                      <label htmlFor="llmTemperature">温度</label>
                      <input
                        id="llmTemperature"
                        type="number"
                        step="0.1"
                        min="0"
                        max="2"
                        value={editingProvider.temperature ?? 0.7}
                        onChange={(event) => updateProvider(editingProvider.id, { temperature: Number(event.target.value) })}
                      />
                    </div>

                    <div className="form-row">
                      <label htmlFor="llmTimeout">超时 (秒)</label>
                      <input
                        id="llmTimeout"
                        type="number"
                        min="10"
                        max="300"
                        value={editingProvider.timeout ?? 60}
                        onChange={(event) => updateProvider(editingProvider.id, { timeout: Number(event.target.value) })}
                      />
                    </div>
                  </div>

                  <div className="llm-flags">
                    <label>
                      <input
                        type="checkbox"
                        checked={editingProvider.json_mode ?? false}
                        onChange={(event) => updateProvider(editingProvider.id, { json_mode: event.target.checked })}
                      />
                      JSON 模式
                    </label>
                    <label>
                      <input
                        type="checkbox"
                        checked={editingProvider.remember_key ?? false}
                        onChange={(event) => updateProvider(editingProvider.id, { remember_key: event.target.checked })}
                      />
                      记住 API 密钥
                    </label>
                  </div>
                </>
              )}
            </div>
          </div>

          <div className="llm-preset-row">
            {PROVIDER_PRESETS.map((preset) => (
              <button key={preset.id} type="button" className="llm-preset-btn" onClick={() => addPreset(preset)}>
                + {preset.name}
              </button>
            ))}
          </div>

          <div className="form-actions">
            <button type="button" className="save-btn" onClick={save}>保存</button>
            <button type="button" className="clear-btn" onClick={clearAll}>清空</button>
          </div>
        </div>
      )}
    </div>
  );
}
