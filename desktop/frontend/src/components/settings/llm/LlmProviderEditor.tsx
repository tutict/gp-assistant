import { ChevronDown, CircleAlert, CircleCheck, Eye, EyeOff, LoaderCircle } from "lucide-react";
import { useState } from "react";
import type { LlmProviderSettings } from "../../../types";
import type { LlmConnectionState, LlmModelCatalog } from "../../../hooks/useLlmSettingsDrafts";
import { LLM_DIAGNOSTIC_LABELS, LLM_PROVIDER_KINDS, type CredentialDraft, type LlmFieldErrors, type LlmProviderDraft } from "../../../lib/llmSettingsDraft";
import { LlmModelPicker } from "./LlmModelPicker";

interface Props {
  draft: LlmProviderDraft; errors: LlmFieldErrors; connection?: LlmConnectionState; catalog?: LlmModelCatalog;
  disabled: boolean; onPatch: (patch: Partial<LlmProviderSettings>) => void;
  onCredential: (value: CredentialDraft) => void; onFetch: () => void | Promise<void>;
}
export function LlmProviderEditor({ draft, errors, connection, catalog, disabled, onPatch, onCredential, onFetch }: Props) {
  const [visibleKey, setVisibleKey] = useState(false);
  const provider = draft.provider;
  const secret = draft.credential.kind === "replace" ? draft.credential.key : "";
  const result = connection?.result;
  const diagnostic = connection?.error;
  const fieldError = (field: keyof LlmProviderSettings) => errors[field] && <p className="llm-field-error" role="alert">{errors[field]}</p>;
  return <section className="llm-provider-editor" aria-label="连接配置编辑">
    <div className="llm-editor-head"><div><span>正在编辑</span><strong>{provider.name || "自定义连接"}</strong></div></div>
    <div className={"llm-connection-state " + (connection?.state || "idle")} role={diagnostic ? "alert" : "status"}>
      <strong>{connection?.state === "testing" ? <><LoaderCircle size={15} aria-hidden="true" />正在验证基础推理…</>
        : connection?.state === "success" ? <><CircleCheck size={16} aria-hidden="true" />基础推理测试通过</>
          : diagnostic ? <><CircleAlert size={16} aria-hidden="true" />{LLM_DIAGNOSTIC_LABELS[diagnostic.code]}</> : "尚未测试当前配置"}</strong>
      {diagnostic && <p>{diagnostic.message}</p>}
      {(result || diagnostic) && <div className="llm-diagnostic-meta">
        <span>{provider.api_format === "anthropic_messages" ? "Anthropic Messages" : provider.api_format === "openai_responses" ? "OpenAI Responses" : "OpenAI Chat"}</span>
        {(result?.status ?? diagnostic?.status) !== undefined && <span>HTTP {result?.status ?? diagnostic?.status}</span>}
        {(result?.elapsed_ms ?? diagnostic?.elapsed_ms) !== undefined && <span>{result?.elapsed_ms ?? diagnostic?.elapsed_ms} ms</span>}
        {(result?.endpoint || diagnostic?.endpoint) && <code>{result?.endpoint || diagnostic?.endpoint}</code>}
      </div>}
    </div>
    <fieldset className="llm-editor-fields" disabled={disabled}>
      <div className="llm-editor-row">
        <div className="llm-field"><label htmlFor="llmProviderKind">供应商类型</label>
          <select id="llmProviderKind" value={provider.provider || "custom"} onChange={event => onPatch({ provider: event.target.value })}>
            {LLM_PROVIDER_KINDS.map(item => <option key={item.id} value={item.id}>{item.label}</option>)}
          </select></div>
        <div className="llm-field"><label htmlFor="llmProviderName">连接名称</label>
          <input id="llmProviderName" type="text" value={provider.name || ""} onChange={event => onPatch({ name: event.target.value })} placeholder="例如公司网关 / 本地模型" /></div>
      </div>
      <div className="llm-field"><label htmlFor="llmApiFormat">接口协议</label>
        <select id="llmApiFormat" value={provider.api_format || "openai_chat"} onChange={event => onPatch({ api_format: event.target.value as LlmProviderSettings["api_format"] })}>
          <option value="openai_chat">OpenAI Chat Completions</option><option value="openai_responses">OpenAI Responses</option>
          <option value="anthropic_messages">Anthropic Messages</option>
        </select></div>
      <div className="llm-field">
        <div className="llm-field-label"><label htmlFor="llmBaseUrl">接口地址</label>
          <div className="llm-endpoint-mode" role="group" aria-label="接口地址模式">
            <button type="button" aria-pressed={provider.endpoint_mode !== "full_url"} onClick={() => onPatch({ endpoint_mode: "base_url" })}>基础地址</button>
            <button type="button" aria-pressed={provider.endpoint_mode === "full_url"} onClick={() => onPatch({ endpoint_mode: "full_url" })}>完整 URL</button>
          </div>
        </div>
        <input id="llmBaseUrl" type="url" value={provider.base_url || ""} aria-invalid={Boolean(errors.base_url)} aria-describedby="llmAddressHint"
          onChange={event => onPatch({ base_url: event.target.value })}
          placeholder={provider.endpoint_mode === "full_url" ? "https://api.example.com/v1/chat/completions" : "https://api.example.com/v1"} />
        {fieldError("base_url")}<p id="llmAddressHint" className="llm-field-hint">
          {provider.endpoint_mode === "full_url" ? "使用填写的完整推理接口地址。" : "自动拼接所选协议的推理路径。"}
        </p>
      </div>
      <div className="llm-field">
        <div className="llm-field-label"><label htmlFor="llmApiKey">API 密钥</label>
          <span className="llm-secret-status">{draft.credential.kind === "replace" ? "待替换" : draft.credential.kind === "remove" ? "待移除"
            : provider.credential_ref ? "系统安全存储" : provider.has_key || provider.api_key ? "本次会话已配置" : "未配置"}</span></div>
        <div className="llm-secret-input"><input id="llmApiKey" type={visibleKey ? "text" : "password"} value={secret}
          autoComplete="off" spellCheck={false} aria-describedby="llmCredentialHint"
          placeholder={provider.credential_ref ? "已保存在系统安全存储（输入以替换）" : provider.has_key || provider.api_key ? "已配置（输入以替换）" : "本地免鉴权服务可留空"}
          onChange={event => onCredential(event.target.value.trim() ? { kind: "replace", key: event.target.value } : { kind: "keep" })} />
          <button type="button" aria-label={visibleKey ? "隐藏 API 密钥" : "显示 API 密钥"} onClick={() => setVisibleKey(current => !current)}>
            {visibleKey ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
          </button></div>
        <div className="llm-credential-controls"><label className="llm-remember-key">
          <input type="checkbox" checked={provider.remember_key || false} onChange={event => onPatch({ remember_key: event.target.checked })} />记住 API 密钥
        </label>
          {draft.credential.kind === "keep" ? <button type="button" className="llm-text-action" onClick={() => onCredential({ kind: "remove" })}>清除密钥</button>
            : <button type="button" className="llm-text-action" onClick={() => onCredential({ kind: "keep" })}>撤销密钥修改</button>}
        </div>
        <p id="llmCredentialHint" className="llm-field-hint">
          {provider.credential_ref && !provider.remember_key ? "关闭记住后会删除系统密钥；需要鉴权时，请重新输入供本次会话使用。"
            : draft.credential.kind === "remove" ? "保存时移除密钥；测试将使用无密钥配置。"
            : "系统密钥不可回显，替换请重新输入；所有修改在保存后生效。"}
        </p>
      </div>
      <LlmModelPicker model={provider.model || ""} catalog={catalog} disabled={disabled} canFetch={Boolean(provider.base_url?.trim()) && !Object.keys(errors).length}
        error={errors.model} onChange={model => onPatch({ model })} onFetch={onFetch} />
      <details className="llm-advanced-settings">
        <summary><span>高级参数</span><ChevronDown size={16} aria-hidden="true" /></summary>
        <div className="llm-advanced-content">
          <div className="llm-editor-row">
            <div className="llm-field"><label htmlFor="llmTemperature">温度</label>
              <input id="llmTemperature" type="number" min="0" max={provider.api_format === "anthropic_messages" ? "1" : "2"} step="0.1"
                value={Number.isFinite(provider.temperature) ? provider.temperature : ""} aria-invalid={Boolean(errors.temperature)}
                onChange={event => onPatch({ temperature: event.target.value === "" ? NaN : Number(event.target.value) })} />
              {fieldError("temperature")}</div>
            <div className="llm-field"><label htmlFor="llmTimeout">超时（秒）</label>
              <input id="llmTimeout" type="number" min="10" max="180" step="1"
                value={Number.isFinite(provider.timeout) ? provider.timeout : ""} aria-invalid={Boolean(errors.timeout)}
                onChange={event => onPatch({ timeout: event.target.value === "" ? NaN : Number(event.target.value) })} />
              {fieldError("timeout")}</div>
          </div>
          <div className="llm-field"><label htmlFor="llmCustomUserAgent">自定义 User-Agent</label>
            <input id="llmCustomUserAgent" type="text" value={provider.custom_user_agent || ""} aria-invalid={Boolean(errors.custom_user_agent)}
              onChange={event => onPatch({ custom_user_agent: event.target.value })} placeholder="使用应用默认值" />
            {fieldError("custom_user_agent")}</div>
          <label className="llm-json-mode"><input type="checkbox" checked={provider.json_mode || false}
            onChange={event => onPatch({ json_mode: event.target.checked })} />JSON 模式</label>
        </div>
      </details>
    </fieldset>
  </section>;
}
