import { Activity, ArrowLeft, ChevronDown, Download, Eye, EyeOff, LoaderCircle, Plus, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import type {
  LlmConnectionTestResult,
  LlmModelListResult,
  LlmModelOption,
  LlmProviderSettings,
  LlmSettings,
} from "../../types";
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
    api_format: "openai_chat",
    endpoint_mode: "base_url",
    json_mode: false,
  },
  {
    id: "zhipu",
    name: "GLM / 智谱",
    provider: "zhipu",
    base_url: "https://open.bigmodel.cn/api/paas/v4",
    model: "glm-4-flash",
    api_format: "openai_chat",
    endpoint_mode: "base_url",
    json_mode: false,
  },
  {
    id: "qwen",
    name: "通义千问",
    provider: "qwen",
    base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    model: "qwen-plus",
    api_format: "openai_chat",
    endpoint_mode: "base_url",
    json_mode: false,
  },
  {
    id: "openai-compatible",
    name: "OpenAI 兼容",
    provider: "openai-compatible",
    base_url: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    api_format: "openai_chat",
    endpoint_mode: "base_url",
    json_mode: false,
  },
  {
    id: "local",
    name: "本地模型",
    provider: "local",
    base_url: "http://127.0.0.1:11434/v1",
    model: "qwen2.5:7b",
    api_format: "openai_chat",
    endpoint_mode: "base_url",
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

interface ProviderConnectionState {
  state: "idle" | "testing" | "success" | "error";
  text: string;
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
  const [connectionStates, setConnectionStates] = useState<Record<string, ProviderConnectionState>>({});
  const [visibleApiKeys, setVisibleApiKeys] = useState<Record<string, boolean>>({});
  const [modelMenuProviderId, setModelMenuProviderId] = useState<string | null>(null);
  const modelRequestTokens = useRef<Record<string, number>>({});
  const connectionRequestTokens = useRef<Record<string, number>>({});
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
        ? "当前模型不在返回列表中，可保留手动 ID 或重新选择。"
        : editingCatalog?.loaded
          ? "已拉取 " + availableModels.length + " 个模型，点击右侧箭头选择默认模型。"
          : "可直接输入模型 ID，或从供应商拉取模型列表。";
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

  const invalidateProviderConnection = useCallback((id?: string) => {
    if (!id) return;
    connectionRequestTokens.current[id] = (connectionRequestTokens.current[id] || 0) + 1;
    setConnectionStates((current) => {
      if (!current[id]) return current;
      const next = { ...current };
      delete next[id];
      return next;
    });
  }, []);

  const updateProvider = useCallback((id: string | undefined, patch: Partial<LlmProviderSettings>) => {
    if (!id) return;
    if (["provider", "base_url", "api_key", "api_format", "endpoint_mode", "custom_user_agent"].some(
      (key) => Object.prototype.hasOwnProperty.call(patch, key),
    )) {
      invalidateProviderModels(id);
    }
    if (["provider", "base_url", "api_key", "model", "api_format", "endpoint_mode", "custom_user_agent"].some(
      (key) => Object.prototype.hasOwnProperty.call(patch, key),
    )) {
      invalidateProviderConnection(id);
    }
    commit({
      ...normalized,
      providers: providers.map((provider) => provider.id === id ? { ...provider, ...patch } : provider),
    });
  }, [commit, invalidateProviderConnection, invalidateProviderModels, normalized, providers]);

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
        api_format: provider.api_format || "openai_chat",
        endpoint_mode: provider.endpoint_mode || "base_url",
        custom_user_agent: provider.custom_user_agent || "",
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

  const testProviderConnection = useCallback(async (provider: LlmProviderSettings) => {
    const id = provider.id;
    if (!id) return;
    if (!provider.base_url?.trim() || !provider.model?.trim()) {
      setConnectionStates((current) => ({
        ...current,
        [id]: { state: "error", text: "请先填写接口地址和默认模型。" },
      }));
      return;
    }
    const timeoutSeconds = Math.max(5, Math.min(120, Math.round(provider.timeout ?? 60)));
    const requestToken = (connectionRequestTokens.current[id] || 0) + 1;
    connectionRequestTokens.current[id] = requestToken;
    setConnectionStates((current) => ({
      ...current,
      [id]: { state: "testing", text: "正在测试模型连接…" },
    }));
    try {
      const result = await postJson<LlmConnectionTestResult>("/api/llm/test", {
        base_url: provider.base_url.trim(),
        api_key: provider.api_key || "",
        model: provider.model.trim(),
        api_format: provider.api_format || "openai_chat",
        endpoint_mode: provider.endpoint_mode || "base_url",
        custom_user_agent: provider.custom_user_agent || "",
        timeout_seconds: timeoutSeconds,
      }, { timeoutMs: (timeoutSeconds + 5) * 1000 });
      if (connectionRequestTokens.current[id] !== requestToken) return;
      setConnectionStates((current) => ({
        ...current,
        [id]: {
          state: "success",
          text: `连接成功 · ${result.elapsed_ms ?? 0} ms`,
        },
      }));
      showStatus("连接成功");
    } catch (error) {
      if (connectionRequestTokens.current[id] !== requestToken) return;
      setConnectionStates((current) => ({
        ...current,
        [id]: { state: "error", text: modelLoadError(error) },
      }));
      showStatus("连接失败", "neutral");
    }
  }, [showStatus]);

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
      api_format: "openai_chat",
      endpoint_mode: "base_url",
      custom_user_agent: "",
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
    invalidateProviderConnection(id);
    const nextProviders = providers.filter((provider) => provider.id !== id);
    const activeProviderId = normalized.active_provider_id === id ? nextProviders[0]?.id : normalized.active_provider_id;
    commit({ active_provider_id: activeProviderId, providers: nextProviders });
    if (editingProviderId === id) setEditingProviderId(activeProviderId || null);
    showStatus("已删除", "neutral");
  }, [commit, editingProviderId, invalidateProviderConnection, invalidateProviderModels, normalized.active_provider_id, providers, showStatus]);

  const clearAll = useCallback(() => {
    onChange(null);
    setEditingProviderId(null);
    setModelCatalogs({});
    setConnectionStates({});
    setVisibleApiKeys({});
    setModelMenuProviderId(null);
    modelRequestTokens.current = {};
    connectionRequestTokens.current = {};
    showStatus("已清空", "neutral");
  }, [onChange, showStatus]);

  const save = useCallback(() => {
    commit(normalized);
    showStatus("已保存");
    closePanel();
  }, [closePanel, commit, normalized, showStatus]);

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
                    <div className="llm-editor-actions">
                      <button
                        type="button"
                        className="llm-test-btn"
                        onClick={() => void testProviderConnection(editingProvider)}
                        disabled={connectionStates[editingProvider.id || ""]?.state === "testing"}
                      >
                        {connectionStates[editingProvider.id || ""]?.state === "testing"
                          ? <LoaderCircle size={15} aria-hidden="true" />
                          : <Activity size={15} aria-hidden="true" />}
                        测试连接
                      </button>
                      <button
                        type="button"
                        className="clear-btn"
                        onClick={() => removeProvider(editingProvider.id)}
                        disabled={providers.length <= 1}
                      >
                        删除
                      </button>
                    </div>
                  </div>
                  {connectionStates[editingProvider.id || ""] && (
                    <p className={`llm-connection-state ${connectionStates[editingProvider.id || ""].state}`} role="status">
                      {connectionStates[editingProvider.id || ""].text}
                    </p>
                  )}

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
                        onChange={(event) => updateProvider(editingProvider.id, {
                          provider: event.target.value,
                          ...(event.target.value === "anthropic-compatible"
                            ? { api_format: "anthropic_messages" as const }
                            : {}),
                        })}
                      >
                        {PROVIDER_KINDS.map((kind) => (
                          <option key={kind.id} value={kind.id}>{kind.label}</option>
                        ))}
                      </select>
                    </div>
                  </div>

                  <div className="form-row llm-endpoint-field">
                    <div className="llm-field-label">
                      <label htmlFor="llmBaseUrl">API 请求地址</label>
                      <div className="llm-endpoint-mode" role="group" aria-label="请求地址模式">
                        <button
                          type="button"
                          className={(editingProvider.endpoint_mode || "base_url") === "base_url" ? "active" : ""}
                          aria-pressed={(editingProvider.endpoint_mode || "base_url") === "base_url"}
                          onClick={() => updateProvider(editingProvider.id, { endpoint_mode: "base_url" })}
                        >
                          基础地址
                        </button>
                        <button
                          type="button"
                          className={editingProvider.endpoint_mode === "full_url" ? "active" : ""}
                          aria-pressed={editingProvider.endpoint_mode === "full_url"}
                          onClick={() => updateProvider(editingProvider.id, { endpoint_mode: "full_url" })}
                        >
                          完整 URL
                        </button>
                      </div>
                    </div>
                    <input
                      id="llmBaseUrl"
                      type="url"
                      value={editingProvider.base_url || ""}
                      onChange={(event) => updateProvider(editingProvider.id, { base_url: normalizeBaseUrl(event.target.value) })}
                      placeholder={editingProvider.endpoint_mode === "full_url"
                        ? "https://api.example.com/custom/chat"
                        : "https://api.example.com/v1"}
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
                      <input
                        id="llmModel"
                        type="text"
                        className={"llm-model-selection " + (configuredModelUnavailable ? "warn" : "")}
                        value={editingProvider.model || ""}
                        onChange={(event) => updateProvider(editingProvider.id, { model: event.target.value })}
                        placeholder="输入模型 ID"
                        aria-controls="llmModelList"
                        aria-labelledby="llmModelLabel"
                      />
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

                  <div className="form-row llm-api-key-field">
                    <label htmlFor="llmApiKey">API 密钥</label>
                    <div className="llm-secret-input">
                      <input
                        id="llmApiKey"
                        type={visibleApiKeys[editingProvider.id || ""] ? "text" : "password"}
                        value={editingProvider.api_key || ""}
                        onChange={(event) => updateProvider(editingProvider.id, { api_key: event.target.value })}
                        placeholder={maskKey(editingProvider.api_key)}
                        autoComplete="off"
                      />
                      <button
                        type="button"
                        onClick={() => setVisibleApiKeys((current) => ({
                          ...current,
                          [editingProvider.id || ""]: !current[editingProvider.id || ""],
                        }))}
                        aria-label={visibleApiKeys[editingProvider.id || ""] ? "隐藏 API 密钥" : "显示 API 密钥"}
                        title={visibleApiKeys[editingProvider.id || ""] ? "隐藏密钥" : "显示密钥"}
                      >
                        {visibleApiKeys[editingProvider.id || ""]
                          ? <EyeOff size={16} aria-hidden="true" />
                          : <Eye size={16} aria-hidden="true" />}
                      </button>
                    </div>
                  </div>

                  <label className="llm-remember-key">
                    <input
                      type="checkbox"
                      checked={editingProvider.remember_key ?? false}
                      onChange={(event) => updateProvider(editingProvider.id, { remember_key: event.target.checked })}
                    />
                    记住 API 密钥
                  </label>

                  <details className="llm-advanced-settings">
                    <summary>
                      <span>高级选项</span>
                      <ChevronDown size={16} aria-hidden="true" />
                    </summary>
                    <div className="form-row">
                      <label htmlFor="llmApiFormat">上游协议</label>
                      <select
                        id="llmApiFormat"
                        value={editingProvider.api_format || "openai_chat"}
                        onChange={(event) => updateProvider(editingProvider.id, {
                          api_format: event.target.value as LlmProviderSettings["api_format"],
                        })}
                      >
                        <option value="openai_chat">OpenAI Chat Completions</option>
                        <option value="openai_responses">OpenAI Responses</option>
                        <option value="anthropic_messages">Anthropic Messages</option>
                      </select>
                    </div>

                    <div className="form-row">
                      <label htmlFor="llmCustomUserAgent">自定义 User-Agent</label>
                      <input
                        id="llmCustomUserAgent"
                        type="text"
                        value={editingProvider.custom_user_agent || ""}
                        onChange={(event) => updateProvider(editingProvider.id, { custom_user_agent: event.target.value })}
                        placeholder="使用应用默认值"
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
                    </div>
                  </details>
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
