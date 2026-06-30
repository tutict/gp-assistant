import { useCallback, useMemo, useState } from "react";
import type { LlmProviderSettings, LlmSettings } from "../../types";
import { activeLlmProvider, normalizeLlmSettings } from "../../lib/contracts";

const PROVIDER_KINDS = [
  { id: "openai-compatible", label: "OpenAI 兼容", hint: "适用于多数 /v1/chat/completions 网关" },
  { id: "deepseek", label: "DeepSeek", hint: "DeepSeek 官方或兼容代理" },
  { id: "zhipu", label: "GLM / 智谱", hint: "智谱 BigModel OpenAI 兼容接口" },
  { id: "qwen", label: "通义千问", hint: "DashScope 兼容模式" },
  { id: "moonshot", label: "Moonshot", hint: "Kimi / Moonshot 兼容接口" },
  { id: "siliconflow", label: "硅基流动", hint: "聚合模型平台" },
  { id: "anthropic-compatible", label: "Anthropic 兼容", hint: "Claude 或中转服务" },
  { id: "local", label: "本地模型", hint: "Ollama、LM Studio、vLLM 等" },
  { id: "custom", label: "自定义", hint: "公司网关或私有部署" },
] as const;

const PROVIDER_PRESETS = [
  {
    id: "deepseek",
    name: "DeepSeek",
    provider: "deepseek",
    base_url: "https://api.deepseek.com/v1",
    model: "deepseek-chat",
    json_mode: false,
  },
  {
    id: "zhipu",
    name: "GLM / 智谱",
    provider: "zhipu",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    model: "glm-4-flash",
    json_mode: false,
  },
  {
    id: "qwen",
    name: "通义千问",
    provider: "qwen",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    model: "qwen-plus",
    json_mode: false,
  },
  {
    id: "openai-compatible",
    name: "OpenAI 兼容",
    provider: "openai-compatible",
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    json_mode: false,
  },
  {
    id: "local",
    name: "本地模型",
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

function providerKindLabel(provider?: string): string {
  return PROVIDER_KINDS.find((kind) => kind.id === provider)?.label || "自定义";
}

function endpointState(provider?: LlmProviderSettings): { label: string; tone: "ready" | "warn" } {
  if (!provider?.base_url || !provider?.model) return { label: "待补全", tone: "warn" };
  return { label: "可启用", tone: "ready" };
}

export function LlmSettingsPanel({ settings, onChange }: LlmSettingsPanelProps) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<{ text: string; state: string }>({ text: "", state: "neutral" });
  const normalized = useMemo(() => normalizeLlmSettings(settings), [settings]);
  const active = activeLlmProvider(settings);
  const providers = normalized.providers || [];
  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const editingProvider = providers.find((provider) => provider.id === editingProviderId) || active || providers[0];
  const activeState = endpointState(active);

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

  const addBlankProvider = useCallback(() => {
    const nextProvider: LlmProviderSettings = {
      id: providerId("custom"),
      name: "新连接",
      provider: "custom",
      base_url: "",
      model: "",
      temperature: 0.7,
      timeout: 60,
      json_mode: false,
      remember_key: false,
    };
    commit({ active_provider_id: nextProvider.id, providers: [...providers, nextProvider] });
    setEditingProviderId(nextProvider.id || null);
    setOpen(true);
    showStatus("已新建");
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
    showStatus("已清空", "neutral");
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
          <em>{active?.model || "选择一个兼容接口"}</em>
          <b className={`llm-endpoint-state ${activeState.tone}`}>{activeState.label}</b>
          {status.text && <span className={`llm-status ${status.state}`}>{status.text}</span>}
        </button>
      </div>

      {open && (
        <div className="llm-settings-body llm-switcher">
          <div className="llm-switcher-summary">
            <div>
              <span>当前 Endpoint</span>
              <strong>{active?.base_url || "尚未填写接口地址"}</strong>
            </div>
            <button type="button" className="llm-add-btn" onClick={addBlankProvider}>+ 新建连接</button>
          </div>

          <div className="llm-switcher-grid">
            <div className="llm-provider-list" aria-label="模型连接列表">
              {providers.map((provider) => {
                const selected = provider.id === normalized.active_provider_id;
                const state = endpointState(provider);
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
                    <span className="llm-provider-mark">{providerKindLabel(provider.provider).slice(0, 1)}</span>
                    <span className="llm-provider-copy">
                      <strong>{provider.name || provider.provider || "自定义连接"}</strong>
                      <em>{providerKindLabel(provider.provider)} · {provider.model || "未选择模型"}</em>
                    </span>
                    <b className={state.tone}>{selected ? "启用中" : state.label}</b>
                  </button>
                );
              })}
            </div>

            <div className="llm-provider-editor">
              {editingProvider && (
                <>
                  <div className="llm-editor-head">
                    <div>
                      <span>连接配置</span>
                      <strong>{editingProvider.name || "自定义连接"}</strong>
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

                  <div className="llm-editor-row">
                    <div className="form-row">
                      <label htmlFor="llmProviderName">名称</label>
                      <input
                        id="llmProviderName"
                        type="text"
                        value={editingProvider.name || ""}
                        onChange={(event) => updateProvider(editingProvider.id, { name: event.target.value })}
                        placeholder="例如 GLM / DeepSeek / 公司网关"
                      />
                    </div>

                    <div className="form-row">
                      <label htmlFor="llmProviderKind">供应商类型</label>
                      <select
                        id="llmProviderKind"
                        value={editingProvider.provider || "custom"}
                        onChange={(event) => updateProvider(editingProvider.id, { provider: event.target.value })}
                      >
                        {PROVIDER_KINDS.map((kind) => (
                          <option key={kind.id} value={kind.id}>{kind.label}</option>
                        ))}
                      </select>
                    </div>
                  </div>

                  <div className="form-row">
                    <label htmlFor="llmBaseUrl">接口地址</label>
                    <input
                      id="llmBaseUrl"
                      type="url"
                      value={editingProvider.base_url || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { base_url: normalizeBaseUrl(event.target.value) })}
                      placeholder="https://api.example.com/v1"
                    />
                  </div>

                  <div className="form-row">
                    <label htmlFor="llmModel">模型 ID</label>
                    <input
                      id="llmModel"
                      type="text"
                      value={editingProvider.model || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { model: event.target.value.trim() })}
                      placeholder="deepseek-chat / glm-4-flash / qwen-plus"
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

                  <div className="llm-editor-row compact">
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
