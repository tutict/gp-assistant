import type { LlmClientConfig, LlmProviderSettings, LlmSettings } from "../types";
import { llmProviderAuth, normalizeLlmSettings } from "./contracts";
import { sessionCredentialVersion } from "./llmCredentialSession";
import { newSecureId } from "./secureId";

export const LLM_PROVIDER_KINDS = [
  { id: "openai-compatible", label: "OpenAI 兼容" }, { id: "deepseek", label: "DeepSeek" },
  { id: "zhipu", label: "GLM / 智谱" }, { id: "qwen", label: "通义千问" },
  { id: "moonshot", label: "Moonshot" }, { id: "siliconflow", label: "硅基流动" },
  { id: "anthropic-compatible", label: "Anthropic 兼容" }, { id: "local", label: "本地模型" },
  { id: "custom", label: "自定义" },
] as const;
export const LLM_PRESETS = [
  { provider: "deepseek", name: "DeepSeek", base_url: "https://api.deepseek.com/v1", model: "deepseek-chat" },
  { provider: "zhipu", name: "GLM / 智谱", base_url: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash" },
  { provider: "qwen", name: "通义千问", base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus" },
  { provider: "openai-compatible", name: "OpenAI 兼容", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
  { provider: "local", name: "本地模型", base_url: "http://127.0.0.1:11434/v1", model: "qwen2.5:7b" },
] as const;

export type CredentialDraft = { kind: "keep" } | { kind: "replace"; key: string } | { kind: "remove" };
export interface LlmProviderDraft {
  provider: LlmProviderSettings;
  baseline: LlmProviderSettings;
  persisted: boolean;
  isNew: boolean;
  credential: CredentialDraft;
  credentialVersion: number;
  revision: number;
  catalogRevision: number;
}
export type LlmFieldErrors = Partial<Record<keyof LlmProviderSettings, string>>;
export const REQUEST_FIELDS = ["provider", "base_url", "model", "api_format", "endpoint_mode",
  "custom_user_agent", "temperature", "timeout", "json_mode", "remember_key", "credential_ref", "has_key"] as const;
const CATALOG_FIELDS = ["provider", "base_url", "api_format", "endpoint_mode", "custom_user_agent", "timeout", "remember_key", "credential_ref", "has_key"] as const;
export function providerKindLabel(kind?: string) {
  return LLM_PROVIDER_KINDS.find(item => item.id === kind)?.label || "自定义";
}
export function draftProvider(provider: LlmProviderSettings): LlmProviderSettings {
  return { ...provider, temperature: provider.temperature ?? 0.7,
    timeout: Math.min(180, Math.max(10, provider.timeout ?? 60)), json_mode: provider.json_mode ?? false,
    remember_key: provider.remember_key ?? false };
}
export function makeProviderDraft(provider: LlmProviderSettings, persisted: boolean, isNew = false): LlmProviderDraft {
  const copy = draftProvider(provider);
  return { provider: copy, baseline: copy, persisted, isNew, credential: { kind: "keep" },
    credentialVersion: sessionCredentialVersion(provider.id), revision: 0, catalogRevision: 0 };
}
export function newProviderDraft(preset?: Partial<LlmProviderSettings>) {
  return makeProviderDraft({ id: "connection-" + newSecureId(), name: "新连接", provider: "custom",
    base_url: "", model: "", api_format: "openai_chat", endpoint_mode: "base_url", custom_user_agent: "",
    ...preset }, false, true);
}
export function providerMetadata(provider: LlmProviderSettings) {
  return JSON.stringify([provider.id, provider.name ?? "", ...REQUEST_FIELDS.map(field => provider[field] ?? null)]);
}
export function sameSavedProvider(left: LlmProviderSettings, right: LlmProviderSettings) {
  return providerMetadata(draftProvider(left)) === providerMetadata(draftProvider(right)) && left.api_key === right.api_key;
}
export function isDraftDirty(draft: LlmProviderDraft) {
  return draft.isNew || draft.credential.kind !== "keep" || !sameSavedProvider(draft.provider, draft.baseline);
}
export function requestChanged(left: LlmProviderSettings, right: LlmProviderSettings) {
  return REQUEST_FIELDS.some(field => left[field] !== right[field]) || left.api_key !== right.api_key;
}
export function patchProviderDraft(draft: LlmProviderDraft, patch: Partial<LlmProviderSettings>): LlmProviderDraft {
  const provider = { ...draft.provider, ...patch };
  return { ...draft, provider, revision: draft.revision + Number(requestChanged(draft.provider, provider)),
    catalogRevision: draft.catalogRevision + Number(CATALOG_FIELDS.some(field => draft.provider[field] !== provider[field])) };
}
export function credentialDraftAuth(draft: LlmProviderDraft): Pick<LlmClientConfig, "api_key" | "credential_ref"> {
  if (draft.credential.kind === "replace") return draft.credential.key.trim() ? { api_key: draft.credential.key.trim() } : {};
  if (draft.credential.kind === "remove" || (draft.provider.credential_ref && !draft.provider.remember_key)) return {};
  return llmProviderAuth(draft.provider);
}
export function llmDraftPayload(draft: LlmProviderDraft) {
  const provider = draft.provider;
  return { provider: provider.provider || "custom", base_url: provider.base_url?.trim() || "",
    model: provider.model?.trim() || "", api_format: provider.api_format || "openai_chat",
    endpoint_mode: provider.endpoint_mode || "base_url", custom_user_agent: provider.custom_user_agent?.trim() || "",
    timeout_seconds: provider.timeout ?? 60, temperature: provider.temperature ?? 0.7,
    json_mode: provider.json_mode ?? false, ...credentialDraftAuth(draft) };
}
function privateHttpHost(host: string) {
  const name = host.toLowerCase().replace(/^\[|\]$/g, "");
  if (name === "localhost" || name === "::1" || /^(fc|fd)[a-f0-9]{2}:|^fe[89ab][a-f0-9]:/i.test(name)) return true;
  const octets = name.split(".").map(Number);
  if (octets.length !== 4 || octets.some(octet => !Number.isInteger(octet) || octet < 0 || octet > 255)) return false;
  return octets[0] === 127 || octets[0] === 10 || (octets[0] === 192 && octets[1] === 168)
    || (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31);
}
export function validateLlmProvider(provider: LlmProviderSettings, complete = false): LlmFieldErrors {
  const errors: LlmFieldErrors = {};
  const address = provider.base_url?.trim();
  if (!address && complete) errors.base_url = "请填写接口地址。";
  if (address) {
    try {
      const url = new URL(address);
      if (!["https:", "http:"].includes(url.protocol) || !url.hostname) errors.base_url = "请输入有效的 HTTP 或 HTTPS 地址。";
      else if (url.username || url.password) errors.base_url = "地址不能包含用户名或密码。";
      else if (url.protocol === "http:" && !privateHttpHost(url.hostname)) errors.base_url = "公网接口请使用 HTTPS；HTTP 仅适用于本机或私有局域网。";
      else if (provider.endpoint_mode === "full_url" && provider.api_format === "anthropic_messages"
        && (url.search || url.hash || !url.pathname.replace(/\/+$/, "").endsWith("/v1/messages"))) {
        errors.base_url = "Anthropic 完整 URL 须以 /v1/messages 结尾，且不包含查询参数或片段。";
      }
    } catch { errors.base_url = "请输入完整的接口地址，例如 https://api.example.com/v1。"; }
  }
  if (complete && !provider.model?.trim()) errors.model = "请填写模型 ID，或从列表中选择。";
  const maximumTemperature = provider.api_format === "anthropic_messages" ? 1 : 2;
  if (!Number.isFinite(provider.temperature) || provider.temperature! < 0 || provider.temperature! > maximumTemperature) {
    errors.temperature = "温度须在 0–" + maximumTemperature + " 之间。";
  }
  if (!Number.isInteger(provider.timeout) || provider.timeout! < 10 || provider.timeout! > 180) errors.timeout = "超时须为 10–180 秒的整数。";
  if (new TextEncoder().encode(provider.custom_user_agent || "").length > 256 || /[\r\n]/.test(provider.custom_user_agent || "")) {
    errors.custom_user_agent = "User-Agent 不能超过 256 字节或包含换行。";
  }
  return errors;
}
export function saveDraftUpdate(previous: LlmSettings | null, draft: LlmProviderDraft, activate: boolean): LlmSettings {
  const current = previous ? normalizeLlmSettings(previous) : { active_provider_id: "", providers: [] };
  const existing = current.providers?.find(provider => provider.id === draft.provider.id);
  if ((draft.persisted && (!existing || !sameSavedProvider(existing, draft.baseline)))
    || (!draft.persisted && existing) || sessionCredentialVersion(draft.provider.id) !== draft.credentialVersion) {
    throw new Error("此连接已在其他入口修改，请重新载入已保存配置后再编辑。");
  }
  const provider = { ...draft.provider, name: draft.provider.name?.trim() || "自定义连接",
    base_url: draft.provider.base_url?.trim() || "", model: draft.provider.model?.trim() || "" };
  // Omitting api_key retains an existing credential. Only an explicit action may remove it.
  if (draft.credential.kind === "replace") provider.api_key = draft.credential.key.trim();
  else if (draft.credential.kind === "remove") provider.api_key = "";
  else if (!draft.provider.api_key) delete provider.api_key;
  return { active_provider_id: activate ? provider.id : current.active_provider_id,
    providers: existing ? current.providers!.map(item => item.id === provider.id ? provider : item) : [...current.providers!, provider] };
}

export type LlmDiagnosticCode = "invalid_config" | "credential" | "auth" | "not_found" | "rate_limited"
  | "timeout" | "network" | "upstream" | "invalid_response";
export interface LlmDiagnostic {
  code: LlmDiagnosticCode; message: string; status?: number; endpoint?: string; elapsed_ms?: number; api_format?: string;
}
export const LLM_DIAGNOSTIC_LABELS: Record<LlmDiagnosticCode, string> = {
  invalid_config: "配置需要调整", credential: "密钥不可用", auth: "鉴权未通过", not_found: "地址或模型未找到",
  rate_limited: "请求受限", timeout: "请求超时", network: "网络连接失败", upstream: "供应商服务异常",
  invalid_response: "响应或协议不匹配",
};
export function safeLlmEndpoint(value?: string) {
  if (!value) return "";
  try { const url = new URL(value); url.username = ""; url.password = ""; url.search = ""; url.hash = ""; return url.toString(); }
  catch { return "接口地址不可用"; }
}
export function normalizeLlmDiagnostic(error: unknown, secret?: string): LlmDiagnostic {
  const original = error && typeof error === "object" ? error as Record<string, unknown> : undefined;
  let source: unknown = error instanceof Error ? error.message : error;
  if (typeof source === "string") { try { source = JSON.parse(source); } catch { /* Legacy native string. */ } }
  let record = source && typeof source === "object" ? source as Record<string, unknown> : undefined;
  if (record?.error && typeof record.error === "object") record = { ...record, ...record.error as Record<string, unknown> };
  let message = typeof record?.message === "string" ? record.message : typeof record?.error === "string" ? record.error
    : typeof record?.detail === "string" ? record.detail : typeof source === "string" ? source : "请求失败，请检查配置后重试。";
  if (secret?.trim()) message = message.split(secret.trim()).join("[已隐藏]");
  message = message.replace(/https?:\/\/[^\s"'<>]+/g, value => safeLlmEndpoint(value)).replace(/^Error:\s*/i, "").slice(0, 320);
  const status = typeof record?.status === "number" ? record.status : typeof original?.status === "number" ? original.status
    : Number(message.match(/\b(400|401|403|404|405|408|415|422|429|5\d\d)\b/)?.[1]) || undefined;
  const code = typeof record?.code === "string" && Object.hasOwn(LLM_DIAGNOSTIC_LABELS, record.code) ? record.code as LlmDiagnosticCode
    : status === 401 || status === 403 ? "auth" : status === 404 ? "not_found" : status === 429 ? "rate_limited"
    : status === 400 || status === 405 || status === 415 || status === 422 ? "invalid_config"
    : status === 408 || status === 504 ? "timeout" : status && status >= 500 ? "upstream"
    : /timeout|timed out|超时/i.test(message) ? "timeout" : /protocol|JSON|协议|格式/i.test(message) ? "invalid_response" : "network";
  return { code, message, status, endpoint: safeLlmEndpoint(typeof record?.endpoint === "string" ? record.endpoint : undefined),
    elapsed_ms: typeof record?.elapsed_ms === "number" ? record.elapsed_ms : undefined,
    api_format: typeof record?.api_format === "string" ? record.api_format : undefined };
}
