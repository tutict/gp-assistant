import { ArrowLeft, ChevronDown, Download, LoaderCircle, Plus, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import type { LlmModelListResult, LlmModelOption, LlmProviderSettings, LlmSettings } from "../../types";
import { activeLlmProvider, normalizeLlmSettings } from "../../lib/contracts";
import { postJson } from "../../lib/tauri";

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
  presentation?: "inline" | "dialog";
}

interface ProviderModelCatalog {
  models: LlmModelOption[];
  loaded: boolean;
  loading: boolean;
  error: string;
}

function DialogPortal({ children, enabled }: { children: ReactNode; enabled: boolean }) {
  if (!enabled || typeof document === "undefined") return children;
  return createPortal(children, document.body);
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

function normalizeModelOptions(models?: LlmModelOption[]): LlmModelOption[] {
  const seen = new Set<string>();
  return (models || []).flatMap((model) => {
    const id = String(model?.id || "").trim();
    if (!id || seen.has(id)) return [];
    seen.add(id);
    return [{
      id,
      name: model.name?.trim() || null,
      owned_by: model.owned_by?.trim() || null,
    }];
  });
}

function modelLoadError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error || "");
  return raw.replace(/^Error:\s*/i, "").trim().slice(0, 220) || "无法拉取模型列表，请检查连接配置。";
}

export function LlmSettingsPanel({ settings, onChange, presentation = "inline" }: LlmSettingsPanelProps) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<{ text: string; state: string }>({ text: "", state: "neutral" });
  const normalized = useMemo(() => normalizeLlmSettings(settings), [settings]);
  const active = activeLlmProvider(settings);
  const providers = normalized.providers || [];
  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const [modelCatalogs, setModelCatalogs] = useState<Record<string, ProviderModelCatalog>>({});
  const [modelMenuProviderId, setModelMenuProviderId] = useState<string | null>(null);
  const modelRequestTokens = useRef<Record<string, number>>({});
  const toggleButtonRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const isDialog = presentation === "dialog";
  const closePanel = useCallback(() => {
    setOpen(false);
    setModelMenuProviderId(null);
    if (isDialog) window.requestAnimationFrame(() => toggleButtonRef.current?.focus());
  }, [isDialog]);

  useEffect(() => {
    if (!open || !isDialog) return undefined;
    const previousBodyOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const handleDialogKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closePanel();
        return;
      }
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = Array.from(dialogRef.current.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
      )).filter((element) => element.getClientRects().length > 0);
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialogRef.current.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleDialogKeyDown);
    return () => {
      document.removeEventListener("keydown", handleDialogKeyDown);
      document.body.style.overflow = previousBodyOverflow;
    };
  }, [closePanel, isDialog, open]);
  const editingProvider = providers.find((provider) => provider.id === editingProviderId) || active || providers[0];
  const activeState = endpointState(active);
  const editingCatalog = editingProvider?.id ? modelCatalogs[editingProvider.id] : undefined;
  const availableModels = editingCatalog?.models || [];
  const modelMenuOpen = Boolean(
    editingProvider?.id
      && modelMenuProviderId === editingProvider.id
      && availableModels.length,
  );
  const configuredModelUnavailable = Boolean(
    editingCatalog?.loaded
      && editingProvider?.model
      && !availableModels.some((model) => model.id === editingProvider.model),
  );
  const modelHint = editingCatalog?.loading
    ? "正在从供应商拉取模型列表…"
    : editingCatalog?.error
      ? editingCatalog.error
      : configuredModelUnavailable
        ? "当前模型不在供应商返回的列表中，请重新选择。"
        : editingCatalog?.loaded
          ? "已拉取 " + availableModels.length + " 个模型，点击右侧箭头选择默认模型。"
          : "填写接口地址及所需密钥后，先拉取供应商模型列表。";
  const modelHintTone = editingCatalog?.error
    ? "error"
    : configuredModelUnavailable
      ? "warn"
      : editingCatalog?.loaded
        ? "success"
        : "neutral";

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

  const invalidateProviderModels = useCallback((id?: string) => {
    if (!id) return;
    modelRequestTokens.current[id] = (modelRequestTokens.current[id] || 0) + 1;
    setModelCatalogs((current) => {
      if (!current[id]) return current;
      const next = { ...current };
      delete next[id];
      return next;
    });
    setModelMenuProviderId((current) => current === id ? null : current);
  }, []);

  const updateProvider = useCallback((id: string | undefined, patch: Partial<LlmProviderSettings>) => {
    if (!id) return;
    if (["provider", "base_url", "api_key"].some((key) => Object.prototype.hasOwnProperty.call(patch, key))) {
      invalidateProviderModels(id);
    }
    commit({
      ...normalized,
      providers: providers.map((provider) => provider.id === id ? { ...provider, ...patch } : provider),
    });
  }, [commit, invalidateProviderModels, normalized, providers]);

  const fetchProviderModels = useCallback(async (provider: LlmProviderSettings) => {
    const id = provider.id;
    if (!id) return;
    const baseUrl = normalizeBaseUrl(provider.base_url || "");
    if (!baseUrl) {
      invalidateProviderModels(id);
      setModelCatalogs((current) => ({
        ...current,
        [id]: {
          models: [],
          loaded: false,
          loading: false,
          error: "请先填写供应商接口地址。",
        },
      }));
      return;
    }

    const requestToken = (modelRequestTokens.current[id] || 0) + 1;
    modelRequestTokens.current[id] = requestToken;
    setModelMenuProviderId((current) => current === id ? null : current);
    setModelCatalogs((current) => ({
      ...current,
      [id]: {
        models: current[id]?.models || [],
        loaded: current[id]?.loaded || false,
        loading: true,
        error: "",
      },
    }));

    const timeoutSeconds = Math.max(10, Math.min(120, Math.round(provider.timeout ?? 60)));
    try {
      const result = await postJson<LlmModelListResult>("/api/llm/models", {
        provider: provider.provider || "openai-compatible",
        base_url: baseUrl,
        api_key: provider.api_key || "",
        timeout_seconds: timeoutSeconds,
      }, { timeoutMs: (timeoutSeconds + 5) * 1000 });
      if (modelRequestTokens.current[id] !== requestToken) return;

      const models = normalizeModelOptions(result.models);
      const error = models.length ? "" : "供应商已响应，但没有返回可选择的模型。";
      setModelCatalogs((current) => ({
        ...current,
        [id]: {
          models,
          loaded: true,
          loading: false,
          error,
        },
      }));
      if (models.length) {
        setModelMenuProviderId(id);
        showStatus("已拉取 " + models.length + " 个模型");
      } else {
        showStatus("未返回模型", "neutral");
      }
    } catch (error) {
      if (modelRequestTokens.current[id] !== requestToken) return;
      setModelCatalogs((current) => ({
        ...current,
        [id]: {
          models: current[id]?.models || [],
          loaded: current[id]?.loaded || false,
          loading: false,
          error: modelLoadError(error),
        },
      }));
      showStatus("拉取失败", "neutral");
    }
  }, [invalidateProviderModels, showStatus]);

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
    invalidateProviderModels(id);
    const nextProviders = providers.filter((provider) => provider.id !== id);
    const activeProviderId = normalized.active_provider_id === id ? nextProviders[0]?.id : normalized.active_provider_id;
    commit({ active_provider_id: activeProviderId, providers: nextProviders });
    if (editingProviderId === id) setEditingProviderId(activeProviderId || null);
    showStatus("已删除", "neutral");
  }, [commit, editingProviderId, invalidateProviderModels, normalized.active_provider_id, providers, showStatus]);

  const clearAll = useCallback(() => {
    onChange(null);
    setEditingProviderId(null);
    setModelCatalogs({});
    setModelMenuProviderId(null);
    modelRequestTokens.current = {};
    showStatus("已清空", "neutral");
  }, [onChange, showStatus]);

  const save = useCallback(() => {
    commit(normalized);
    showStatus("已保存");
  }, [commit, normalized, showStatus]);

  return (
    <div className={`llm-settings-panel ${open ? "open" : ""} ${isDialog ? "dialog" : ""}`}>
      <div className="llm-settings-header">
        <button
          type="button"
          className="llm-settings-toggle"
          ref={toggleButtonRef}
          onClick={() => open ? closePanel() : setOpen(true)}
          aria-haspopup={isDialog ? "dialog" : undefined}
          aria-expanded={open}
        >
          <span>模型连接</span>
          <strong>{active?.name || "未配置"}</strong>
          <em>{active?.model || "选择一个兼容接口"}</em>
          <b className={`llm-endpoint-state ${activeState.tone}`}>{activeState.label}</b>
          {status.text && <span className={`llm-status ${status.state}`}>{status.text}</span>}
        </button>
      </div>

      <DialogPortal enabled={open && isDialog}>
      {open && isDialog && (
        <button type="button" className="llm-settings-modal-backdrop" onClick={closePanel} tabIndex={-1} aria-hidden="true" />
      )}

      {open && (
        <div
          ref={isDialog ? dialogRef : undefined}
          className={"llm-settings-body llm-switcher" + (isDialog ? " dialog" : "")}
          role={isDialog ? "dialog" : undefined}
          aria-modal={isDialog || undefined}
          aria-label={isDialog ? "模型连接配置" : undefined}
        >
          <div className="llm-switcher-summary">
            <button
              type="button"
              className="llm-sheet-back-btn"
              onClick={closePanel}
              aria-label={isDialog ? "关闭模型配置" : "返回"}
              title={isDialog ? "关闭模型配置" : "返回"}
              autoFocus={isDialog}
            >
              {isDialog ? <X size={17} aria-hidden="true" /> : <ArrowLeft size={17} aria-hidden="true" />}
            </button>
            <div>
              <span>当前 Endpoint</span>
              <strong>{active?.base_url || "尚未填写接口地址"}</strong>
            </div>
            <button type="button" className="llm-add-btn" onClick={addBlankProvider}><Plus size={15} aria-hidden="true" />新建连接</button>
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
                      setModelMenuProviderId(null);
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
                      <div
                        className="llm-provider-kind-grid"
                        role="radiogroup"
                        aria-label="供应商类型"
                      >
                        {PROVIDER_KINDS.map((kind) => {
                          const selected = (editingProvider.provider || "custom") === kind.id;
                          return (
                            <button
                              key={kind.id}
                              type="button"
                              role="radio"
                              aria-checked={selected}
                              className={`llm-provider-kind-option ${selected ? "active" : ""}`}
                              onClick={() => updateProvider(editingProvider.id, { provider: kind.id })}
                            >
                              <span>{kind.label}</span>
                            </button>
                          );
                        })}
                      </div>
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

                  <div className="form-row llm-model-field">
                    <label id="llmModelLabel" htmlFor="llmModel">默认模型</label>
                    <div
                      className={"llm-model-picker " + (modelMenuOpen ? "open" : "")}
                      onKeyDown={(event) => {
                        if (event.key === "Escape" && modelMenuOpen) {
                          event.stopPropagation();
                          setModelMenuProviderId(null);
                        }
                      }}
                    >
                      <button
                        id="llmModel"
                        type="button"
                        className={"llm-model-selection " + (configuredModelUnavailable ? "warn" : "")}
                        onClick={() => {
                          if (!availableModels.length) return;
                          setModelMenuProviderId(modelMenuOpen ? null : editingProvider.id || null);
                        }}
                        disabled={!availableModels.length}
                        aria-haspopup="listbox"
                        aria-expanded={modelMenuOpen}
                        aria-controls="llmModelList"
                        aria-labelledby="llmModelLabel llmModel"
                      >
                        <span>{editingProvider.model || "请先拉取模型列表"}</span>
                      </button>
                      <button
                        type="button"
                        className={"llm-model-action refresh " + (editingCatalog?.loading ? "loading" : "")}
                        onClick={() => void fetchProviderModels(editingProvider)}
                        disabled={editingCatalog?.loading}
                        aria-label={editingCatalog?.loaded ? "刷新供应商模型列表" : "拉取供应商模型列表"}
                        title={editingCatalog?.loaded ? "刷新模型列表" : "拉取模型列表"}
                      >
                        {editingCatalog?.loading
                          ? <LoaderCircle size={16} aria-hidden="true" />
                          : <Download size={16} aria-hidden="true" />}
                      </button>
                      <button
                        type="button"
                        className="llm-model-action toggle"
                        onClick={() => setModelMenuProviderId(modelMenuOpen ? null : editingProvider.id || null)}
                        disabled={!availableModels.length}
                        aria-label="展开模型列表"
                        aria-haspopup="listbox"
                        aria-expanded={modelMenuOpen}
                        aria-controls="llmModelList"
                      >
                        <ChevronDown size={16} aria-hidden="true" />
                      </button>

                      {modelMenuOpen && (
                        <div className="llm-model-menu" id="llmModelList" role="listbox" aria-label="供应商模型列表">
                          <div className="llm-model-menu-head" role="presentation">
                            <span>{providerKindLabel(editingProvider.provider)}</span>
                            <b>{availableModels.length} 个模型</b>
                          </div>
                          {availableModels.map((model) => {
                            const selected = model.id === editingProvider.model;
                            const description = model.name || model.owned_by || "";
                            return (
                              <button
                                key={model.id}
                                type="button"
                                className={"llm-model-option " + (selected ? "selected" : "")}
                                role="option"
                                aria-selected={selected}
                                onClick={() => {
                                  updateProvider(editingProvider.id, { model: model.id });
                                  setModelMenuProviderId(null);
                                  showStatus("已选择模型");
                                }}
                              >
                                <span>
                                  <strong>{model.id}</strong>
                                  {description && description !== model.id && <em>{description}</em>}
                                </span>
                                {selected && <b>当前</b>}
                              </button>
                            );
                          })}
                        </div>
                      )}
                    </div>
                    <p
                      className={"llm-model-hint " + modelHintTone}
                      role={modelHintTone === "error" ? "alert" : "status"}
                    >
                      {modelHint}
                    </p>
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
                <Plus size={14} aria-hidden="true" /> {preset.name}
              </button>
            ))}
          </div>

          <div className="form-actions">
            <button type="button" className="save-btn" onClick={save}>保存</button>
            <button type="button" className="clear-btn" onClick={clearAll}>清空</button>
          </div>
        </div>
      )}
      </DialogPortal>
    </div>
  );
}
