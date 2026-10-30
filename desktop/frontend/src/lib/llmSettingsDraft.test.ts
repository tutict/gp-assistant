import { beforeEach, describe, expect, it } from "vitest";
import { activeLlmProvider, buildLlmConfig, normalizeLlmSettings } from "./contracts";
import { clearSessionCredentials } from "./llmCredentialSession";
import { credentialDraftAuth, makeProviderDraft, normalizeLlmDiagnostic, patchProviderDraft, saveDraftUpdate, validateLlmProvider } from "./llmSettingsDraft";
beforeEach(clearSessionCredentials);
describe("LLM draft contracts", () => {
  it("persists the first untested connection without implicitly activating it", () => {
    const draft = makeProviderDraft({ id: "new", name: "New", base_url: "https://api.example/v1", model: "m" }, false, true);
    const settings = normalizeLlmSettings(saveDraftUpdate(null, draft, false));
    expect(settings.providers).toHaveLength(1);
    expect(activeLlmProvider(settings)).toBeUndefined();
    expect(buildLlmConfig(settings)).toBeUndefined();
    expect(activeLlmProvider({ providers: settings.providers })?.id).toBe("new");
  });
  it("keeps a stored credential unchanged unless replacing, removing or disabling remember is explicit", () => {
    const draft = makeProviderDraft({ id: "a", credential_ref: "stored", has_key: true, remember_key: true }, true);
    expect(credentialDraftAuth(draft)).toEqual({ credential_ref: "stored" });
    expect(credentialDraftAuth({ ...draft, credential: { kind: "replace", key: "new" } })).toEqual({ api_key: "new" });
    expect(credentialDraftAuth({ ...draft, credential: { kind: "remove" } })).toEqual({});
    expect(credentialDraftAuth(patchProviderDraft(draft, { remember_key: false }))).toEqual({});
  });
  it("allows incomplete inactive profiles and unauthenticated loopback services but rejects unsafe addresses and invalid parameters", () => {
    const base = makeProviderDraft({ id: "a", base_url: "", model: "" }, false).provider;
    expect(validateLlmProvider(base)).toEqual({});
    expect(validateLlmProvider(base, true)).toHaveProperty("model");
    expect(validateLlmProvider({ ...base, base_url: "http://127.0.0.1:11434/v1", model: "local" }, true)).toEqual({});
    expect(validateLlmProvider({ ...base, base_url: "http://public.example/v1" })).toHaveProperty("base_url");
    expect(validateLlmProvider({ ...base, base_url: "https://user:password@api.example" })).toHaveProperty("base_url");
    expect(validateLlmProvider({ ...base, timeout: 181 })).toHaveProperty("timeout");
    expect(validateLlmProvider({ ...base, custom_user_agent: "user\r\nheader" })).toHaveProperty("custom_user_agent");
  });
  it("accepts structured and legacy errors while removing secrets and URL query values", () => {
    const structured = normalizeLlmDiagnostic({ code: "auth", status: 401, message: "echo SECRET",
      endpoint: "https://api.example/v1?api_key=QUERYSECRET", elapsed_ms: 8 }, "SECRET");
    expect(structured.code).toBe("auth");
    expect(structured.status).toBe(401);
    expect(JSON.stringify(structured)).not.toContain("SECRET");
    expect(normalizeLlmDiagnostic("Error: HTTP 429 rate limited").code).toBe("rate_limited");
    expect(normalizeLlmDiagnostic("请求超时").code).toBe("timeout");
  });
  it("decodes HTTP JSON errors and keeps their transport status", () => {
    const error = Object.assign(new Error(JSON.stringify({ code: "auth", message: "密钥未通过", elapsed_ms: 8 })), { status: 401 });
    expect(normalizeLlmDiagnostic(error)).toMatchObject({ code: "auth", message: "密钥未通过", status: 401, elapsed_ms: 8 });
    expect(normalizeLlmDiagnostic({ code: "toString", message: "拒绝", status: 401 }).code).toBe("auth");
  });
  it("preserves a complete URL including trailing slashes in paths and query values", () => {
    const address = "https://api.example/custom/chat/?version=current/";
    const settings = saveDraftUpdate(null, makeProviderDraft({ id: "full", base_url: address, model: "m", endpoint_mode: "full_url" }, false, true), true);
    expect(buildLlmConfig(settings)?.base_url).toBe(address);
  });
});
