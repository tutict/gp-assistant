import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useLlmCredentials } from "../../hooks/useLlmCredentials";
import { buildLlmConfig } from "../../lib/contracts";
import { createLlmCredentialStore, type LlmSettingsSetter } from "../../lib/llmCredentialStore";
import { clearSessionCredentials } from "../../lib/llmCredentialSession";
import type { LlmSettings } from "../../types";
const { postJsonMock } = vi.hoisted(() => ({ postJsonMock: vi.fn() }));
vi.mock("../../lib/tauri", () => ({ postJson: postJsonMock }));
vi.mock("react-dom", () => ({ createPortal: (children: unknown) => children }));
import { LlmSettingsPanel } from "./LlmSettingsPanel";

const SECRET = "synthetic-original-key";
const initial: LlmSettings = { active_provider_id: "a", providers: [
  { id: "a", name: "当前网关", base_url: "https://gateway.example/v1", model: "model-a", api_key: SECRET, remember_key: false },
  { id: "b", name: "备用网关", base_url: "https://backup.example/v1", model: "model-b", remember_key: false },
] };
function fixture(settings = initial) {
  let raw = JSON.stringify(settings);
  const vault = new Map<string, string>();
  const api = {
    put: vi.fn(async (credential_ref: string, key: string) => { vault.set(credential_ref, key); return { credential_ref, has_key: true }; }),
    status: vi.fn(async (credential_ref: string) => ({ credential_ref, has_key: vault.has(credential_ref) })),
    delete: vi.fn(async (credential_ref: string) => { vault.delete(credential_ref); return { credential_ref, has_key: false }; }),
  };
  const store = createLlmCredentialStore({ getItem: () => raw, setItem: (_key, value) => { raw = value; }, removeItem: () => { raw = "null"; } }, api);
  return { store, api, vault, raw: () => raw, saved: () => JSON.parse(raw) as LlmSettings | null, external: undefined as LlmSettingsSetter | undefined };
}
const renderers: ReactTestRenderer[] = [];
async function open(x: ReturnType<typeof fixture>) {
  await x.store.initialize();
  function Harness() {
    const state = useLlmCredentials(x.store);
    x.external = state.setLlmSettings;
    return <LlmSettingsPanel settings={state.llmSettings} onChange={state.setLlmSettings} />;
  }
  let renderer!: ReactTestRenderer;
  await act(async () => { renderer = create(<Harness />); });
  renderers.push(renderer);
  await act(async () => renderer.root.findByProps({ className: "llm-settings-toggle" }).props.onClick());
  return renderer;
}
function button(renderer: ReactTestRenderer, text: string) {
  return renderer.root.find(node => node.type === "button" && node.children.includes(text));
}
async function click(renderer: ReactTestRenderer, text: string) { await act(async () => { await button(renderer, text).props.onClick(); }); }
async function field(renderer: ReactTestRenderer, id: string, value: string | boolean) {
  await act(async () => renderer.root.findByProps({ id }).props.onChange({ target: typeof value === "boolean" ? { checked: value } : { value } }));
}
async function select(renderer: ReactTestRenderer, name: string) {
  await act(async () => renderer.root.findByProps({ "aria-label": "编辑 " + name }).props.onClick());
}
beforeEach(() => {
  clearSessionCredentials();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("window", { requestAnimationFrame: (callback: () => void) => callback(), setTimeout: vi.fn() });
  postJsonMock.mockResolvedValue({ ok: true, status: 200, elapsed_ms: 20, endpoint: "https://gateway.example/v1/chat/completions" });
});
afterEach(async () => { for (const renderer of renderers.splice(0)) await act(async () => renderer.unmount()); postJsonMock.mockReset(); vi.unstubAllGlobals(); });

describe("API drafts with the real credential queue", () => {
  it("saves only the selected provider, retains other drafts, and confirms discard on closing", async () => {
    const x = fixture(); const renderer = await open(x);
    await field(renderer, "llmProviderName", "名称草稿");
    await select(renderer, "备用网关");
    await field(renderer, "llmModel", "manual-model");
    await click(renderer, "保存");
    expect(x.saved()?.active_provider_id).toBe("a");
    expect(x.saved()?.providers?.find(p => p.id === "a")?.name).toBe("当前网关");
    expect(x.saved()?.providers?.find(p => p.id === "b")?.model).toBe("manual-model");
    await select(renderer, "名称草稿");
    expect(renderer.root.findByProps({ id: "llmProviderName" }).props.value).toBe("名称草稿");
    await act(async () => renderer.root.findByProps({ "aria-label": "关闭模型配置" }).props.onClick());
    expect(renderer.root.findByProps({ role: "alertdialog" })).toBeDefined();
    await click(renderer, "继续编辑");
    await act(async () => renderer.root.findByProps({ "aria-label": "关闭模型配置" }).props.onClick());
    await click(renderer, "放弃修改并关闭");
    expect(renderer.root.findAllByProps({ className: "llm-settings-body llm-switcher" })).toHaveLength(0);
    await act(async () => renderer.root.findByProps({ className: "llm-settings-toggle" }).props.onClick());
    expect(renderer.root.findByProps({ id: "llmProviderName" }).props.value).toBe("当前网关");
    expect(x.saved()?.providers?.find(p => p.id === "b")?.model).toBe("manual-model");
  });

  it("requires a current passing test before enabling and invalidates it on model edits", async () => {
    const x = fixture(); const renderer = await open(x);
    await select(renderer, "备用网关");
    expect(button(renderer, "启用此连接").props.disabled).toBe(true);
    await click(renderer, "启用此连接");
    expect(x.saved()?.active_provider_id).toBe("a");
    await click(renderer, "测试连接");
    expect(button(renderer, "启用此连接").props.disabled).toBe(false);
    await field(renderer, "llmModel", "edited-model");
    expect(button(renderer, "保存并启用").props.disabled).toBe(true);
    await click(renderer, "测试连接");
    await click(renderer, "保存并启用");
    expect(x.saved()?.active_provider_id).toBe("b");
    expect(x.saved()?.providers?.find(p => p.id === "b")?.model).toBe("edited-model");
  });

  it("retains a tested active-provider draft when secure saving fails and succeeds on retry", async () => {
    const x = fixture(); const renderer = await open(x);
    await field(renderer, "llmBaseUrl", "https://replacement.example/v1");
    await field(renderer, "llmApiKey", "synthetic-replacement");
    const remember = renderer.root.find(node => node.type === "input" && node.props.type === "checkbox" && node.parent?.props.className === "llm-remember-key");
    await act(async () => remember.props.onChange({ target: { checked: true } }));
    expect(button(renderer, "保存").props.disabled).toBe(true);
    await click(renderer, "保存");
    expect(buildLlmConfig(x.store.getSnapshot().settings)?.api_key).toBe(SECRET);
    await click(renderer, "测试连接");
    expect(postJsonMock.mock.calls[0][1]).toHaveProperty("api_key", "synthetic-replacement");
    x.api.put.mockRejectedValueOnce(new Error("synthetic-replacement"));
    await click(renderer, "保存");
    expect(renderer.root.findByProps({ id: "llmApiKey" }).props.value).toBe("synthetic-replacement");
    expect(buildLlmConfig(x.store.getSnapshot().settings)?.api_key).toBe(SECRET);
    expect(x.saved()?.providers?.[0].base_url).toBe("https://gateway.example/v1");
    expect(JSON.stringify(renderer.toJSON())).toContain("安全保存密钥失败");
    await click(renderer, "保存");
    expect(x.saved()?.providers?.[0].base_url).toBe("https://replacement.example/v1");
    expect(x.saved()?.providers?.[0].credential_ref).toBeTruthy();
    expect(x.raw()).not.toContain("synthetic-replacement");
    expect(renderer.root.findByProps({ id: "llmApiKey" }).props.value).toBe("");
  });

  it("tests remember-off with no OS reference and preserves the stored credential on delete failure", async () => {
    const x = fixture({ active_provider_id: "a", providers: [{ ...initial.providers![0], remember_key: true }] });
    const renderer = await open(x);
    const reference = x.saved()?.providers?.[0].credential_ref;
    const remember = renderer.root.find(node => node.type === "input" && node.props.type === "checkbox" && node.parent?.props.className === "llm-remember-key");
    await act(async () => remember.props.onChange({ target: { checked: false } }));
    await click(renderer, "测试连接");
    expect(postJsonMock.mock.calls[0][1]).not.toHaveProperty("credential_ref");
    expect(postJsonMock.mock.calls[0][1]).not.toHaveProperty("api_key");
    x.api.delete.mockRejectedValueOnce(new Error("delete failed"));
    await click(renderer, "保存");
    expect(x.saved()?.providers?.[0].credential_ref).toBe(reference);
    expect(remember.props.checked).toBe(false);
    await click(renderer, "保存");
    expect(x.saved()?.providers?.[0].credential_ref).toBeUndefined();
    expect(x.saved()?.providers?.[0].has_key).toBe(false);
  });

  it("searches the draft-authenticated model list, selects by keyboard and preserves manual IDs on failure", async () => {
    const x = fixture(); const renderer = await open(x);
    await field(renderer, "llmApiKey", "catalog-draft-key");
    postJsonMock.mockResolvedValueOnce({ models: [{ id: "alpha" }, { id: "beta-one", name: "Beta" }, { id: "beta-two", owned_by: "Beta" }] });
    await act(async () => renderer.root.findByProps({ "aria-label": "拉取供应商模型列表" }).props.onClick());
    expect(postJsonMock.mock.calls[0][1]).toHaveProperty("api_key", "catalog-draft-key");
    await act(async () => renderer.root.findByProps({ "aria-label": "选择供应商模型" }).props.onClick());
    await field(renderer, "llmModelSearch", "beta");
    expect(renderer.root.findAllByProps({ role: "option" })).toHaveLength(2);
    const keyboard = { preventDefault: vi.fn(), stopPropagation: vi.fn() };
    await act(async () => renderer.root.findByProps({ id: "llmModelSearch" }).props.onKeyDown({ key: "ArrowDown", ...keyboard }));
    await act(async () => renderer.root.findByProps({ id: "llmModelSearch" }).props.onKeyDown({ key: "Enter", ...keyboard }));
    expect(renderer.root.findByProps({ id: "llmModel" }).props.value).toBe("beta-two");
    await field(renderer, "llmModel", "manual-unlisted");
    postJsonMock.mockRejectedValueOnce({ code: "rate_limited", status: 429, message: "稍后重试" });
    await act(async () => renderer.root.findByProps({ "aria-label": "拉取供应商模型列表" }).props.onClick());
    expect(renderer.root.findByProps({ id: "llmModel" }).props.value).toBe("manual-unlisted");
    expect(x.saved()?.providers?.[0].model).toBe("model-a");
  });

  it("merges a save with other-provider changes and refuses overwriting external edits to the same provider", async () => {
    const x = fixture(); const renderer = await open(x);
    await field(renderer, "llmProviderName", "本地名称草稿");
    await act(async () => { await x.external!(previous => ({ ...previous, providers: previous!.providers!.map(p => p.id === "b" ? { ...p, name: "外部备用名称" } : p) })); });
    await click(renderer, "保存");
    expect(x.saved()?.providers?.find(p => p.id === "b")?.name).toBe("外部备用名称");
    await field(renderer, "llmProviderName", "第二次本地名称");
    await act(async () => { await x.external!(previous => ({ ...previous, providers: previous!.providers!.map(p => p.id === "a" ? { ...p, name: "其他入口名称" } : p) })); });
    await click(renderer, "保存");
    expect(x.saved()?.providers?.find(p => p.id === "a")?.name).toBe("其他入口名称");
    expect(renderer.root.findByProps({ id: "llmProviderName" }).props.value).toBe("第二次本地名称");
    expect(JSON.stringify(renderer.toJSON())).toContain("重新载入");
  });

  it("requires confirmation to clear connections and prevents deleting the active one", async () => {
    const x = fixture(); const renderer = await open(x);
    await click(renderer, "更多");
    expect(button(renderer, "删除此连接").props.disabled).toBe(true);
    await click(renderer, "清空所有连接");
    expect(x.saved()?.active_provider_id).toBe("a");
    await click(renderer, "取消");
    await click(renderer, "更多");
    await click(renderer, "清空所有连接");
    await click(renderer, "确认清空");
    expect(x.saved()).toBeNull();
    expect(buildLlmConfig(x.store.getSnapshot().settings)).toBeUndefined();
  });

  it("rejects deleting an inactive dirty draft whose session key was replaced elsewhere", async () => {
    const x = fixture({ ...initial, providers: initial.providers!.map(p => p.id === "b" ? { ...p, api_key: "old-backup-key" } : p) });
    const renderer = await open(x);
    await select(renderer, "备用网关");
    await field(renderer, "llmProviderName", "备用名称草稿");
    await act(async () => { await x.external!(previous => ({ ...previous, providers: previous!.providers!.map(p => p.id === "b" ? { ...p, api_key: "new-backup-key" } : p) })); });
    await click(renderer, "更多");
    await click(renderer, "删除此连接");
    await click(renderer, "确认删除");
    expect(x.saved()?.providers?.find(p => p.id === "b")).toBeDefined();
    expect(renderer.root.findByProps({ id: "llmProviderName" }).props.value).toBe("备用名称草稿");
    expect(JSON.stringify(renderer.toJSON())).toContain("重新载入");
  });

  it("retains a passing test when another entrypoint changes only the saved name", async () => {
    const x = fixture(); const renderer = await open(x);
    await select(renderer, "备用网关");
    await click(renderer, "测试连接");
    await act(async () => { await x.external!(previous => ({ ...previous, providers: previous!.providers!.map(p => p.id === "b" ? { ...p, name: "仅修改名称" } : p) })); });
    expect(renderer.root.findByProps({ id: "llmProviderName" }).props.value).toBe("仅修改名称");
    expect(button(renderer, "启用此连接").props.disabled).toBe(false);
  });
});
