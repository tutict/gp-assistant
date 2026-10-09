import { beforeEach, describe, expect, it, vi } from "vitest";
import { createLlmCredentialStore, type CredentialApi, type CredentialStatus } from "./llmCredentialStore";
import { buildLlmConfig } from "./contracts";
import { clearSessionCredentials } from "./llmCredentialSession";

const SECRET = "synthetic-legacy-test-key";
function setup(remember = true, legacy = false) {
  let raw = JSON.stringify(legacy ? { api_key: SECRET, model: "m", remember_key: remember } : {
    providers: [{ id: "a", model: "m", api_key: SECRET, remember_key: remember }], active_provider_id: "a",
  });
  const writes: string[] = [];
  const storage = { getItem: () => raw, setItem: (_: string, value: string) => { writes.push(value); raw = value; }, removeItem: () => { raw = ""; } };
  const api: CredentialApi = {
    put: vi.fn(async (credential_ref: string) => ({ credential_ref, has_key: true })),
    status: vi.fn(async (credential_ref: string) => ({ credential_ref, has_key: true })),
    delete: vi.fn(async (credential_ref: string) => ({ credential_ref, has_key: false })),
  };
  return { storage, api, writes, raw: () => raw, store: createLlmCredentialStore(storage, api) };
}
beforeEach(clearSessionCredentials);
describe("OS credential migration", () => {
  it.each([false, true])("writes and verifies before replacing legacy plaintext (flat=%s)", async (legacy) => {
    const x = setup(true, legacy);
    let release!: () => void;
    x.api.put = vi.fn((credential_ref: string) => new Promise<CredentialStatus>(resolve => { release = () => resolve({credential_ref, has_key: true}); }));
    const pending = x.store.initialize();
    await Promise.resolve();
    expect(x.writes).toEqual([]);
    expect(x.raw()).toContain(SECRET);
    release();
    const result = await pending;
    expect(result.error).toBe("");
    expect(x.api.status).toHaveBeenCalled();
    expect(x.raw()).not.toContain(SECRET);
    expect(JSON.stringify(result.settings)).not.toContain(SECRET);
    expect(buildLlmConfig(result.settings)).toHaveProperty("credential_ref");
    expect(buildLlmConfig(result.settings)).not.toHaveProperty("api_key");
  });
  it.each(["put", "status"] as const)("keeps failed %s migration usable in memory with explicit redacted error", async (method) => {
    const x = setup();
    const original = x.raw();
    x.api[method] = vi.fn(async () => { throw new Error(SECRET); });
    const result = await x.store.initialize();
    expect(result.error).toMatch(/session|本次/);
    expect(result.error).not.toContain(SECRET);
    expect(buildLlmConfig(result.settings)?.api_key).toBe(SECRET);
    expect(JSON.stringify(result.settings)).not.toContain(SECRET);
    expect(x.raw()).toBe(original);
    expect(x.writes).toEqual([]);
    expect(result.migrationPending).toBe(true);
  });
  it("never stores a nonremembered key natively or in settings, while requests still work", async () => {
    const x = setup(false);
    const result = await x.store.initialize();
    expect(x.api.put).not.toHaveBeenCalled();
    expect(buildLlmConfig(result.settings)?.api_key).toBe(SECRET);
    expect(JSON.stringify(result.settings)).not.toContain(SECRET);
    expect(x.raw()).not.toContain(SECRET);
    clearSessionCredentials();
    expect(buildLlmConfig(result.settings)).toBeUndefined();
  });
  it("deletes removed credentials, and keeps the reference if deletion fails", async () => {
    const x = setup();
    const initial = await x.store.initialize();
    x.api.delete = vi.fn(async () => { throw new Error(SECRET); });
    const failed = await x.store.set(null);
    expect(failed.settings).toEqual(initial.settings);
    expect(failed.error).toBeTruthy();
    x.api.delete = vi.fn(async credential_ref => ({credential_ref, has_key: false}));
    const cleared = await x.store.set(null);
    expect(cleared.settings).toBeNull();
    expect(x.api.delete).toHaveBeenCalled();
  });
  it("serializes overlapping saves instead of resurrecting cleared keys", async () => {
    const x = setup(false); await x.store.initialize();
    const save = x.store.set({providers: [{id: "a", model: "m", api_key: "new-synthetic", remember_key: true}]});
    const clear = x.store.set(null);
    await save; expect((await clear).settings).toBeNull();
    expect(x.raw()).toBe("null");
  });
});

it("clear deletes a native reference and a nonremembered key can be remembered later", async () => {
  const x = setup(false); const initial = await x.store.initialize();
  const remembered = await x.store.set({ ...initial.settings, providers: initial.settings!.providers!.map(p => ({...p, remember_key: true})) });
  expect(x.api.put).toHaveBeenCalledWith(expect.any(String), SECRET);
  const cleared = await x.store.set({...remembered.settings, providers: remembered.settings!.providers!.map(p => ({...p, api_key: ""}))});
  expect(x.api.delete).toHaveBeenCalled();
  expect(buildLlmConfig(cleared.settings)).toBeUndefined();
});
it("retains a reference when native status fails on restart instead of stripping a configured model", async () => {
  const x = setup(); const initial = await x.store.initialize();
  x.api.status = vi.fn(async () => { throw new Error(SECRET); });
  const restarted = await createLlmCredentialStore(x.storage, x.api).initialize();
  expect(restarted.error).toBeTruthy();
  expect(restarted.settings?.providers?.[0].credential_ref).toBe(initial.settings?.providers?.[0].credential_ref);
});
it("keeps failed legacy migration byte-for-byte until retry verifies secure storage", async () => {
  const x = setup(); const original = x.raw();
  x.api.put = vi.fn(async () => { throw new Error(SECRET); });
  const result = await x.store.initialize();
  expect(result.error).toMatch(/旧.*明文/);
  expect(x.raw()).toBe(original);
  expect(buildLlmConfig(result.settings)?.api_key).toBe(SECRET);
  await x.store.set({ providers: [{ id: "a", model: "changed" }] });
  expect(x.raw()).toBe(original); // no later settings effect may erase pending legacy bytes
  x.api.put = vi.fn(async credential_ref => ({credential_ref, has_key: true}));
  const retry = await x.store.retryMigration();
  expect(retry.migrationPending).toBe(false);
  expect(retry.error).toBe("");
  expect(x.raw()).not.toContain(SECRET);
  expect(buildLlmConfig(retry.settings)).toHaveProperty("credential_ref");
});
it("new remembered-key failures remain session-only, never creating plaintext storage", async () => {
  const x = setup(false); await x.store.initialize();
  x.api.put = vi.fn(async () => { throw new Error(SECRET); });
  const result = await x.store.set({providers: [{id: "new", model: "m", api_key: "new-synthetic", remember_key: true}]});
  expect(result.error).toMatch(/本次/);
  expect(result.migrationPending).toBe(false);
  expect(x.raw()).not.toContain("new-synthetic");
  expect(buildLlmConfig(result.settings)?.api_key).toBe("new-synthetic");
});
it("keeps old plaintext intact when verified migration cannot persist its reference", async () => {
  const x = setup(); const original = x.raw();
  x.storage.setItem = () => { throw new Error("quota"); };
  const result = await x.store.initialize();
  expect(result.error).toBeTruthy();
  expect(result.migrationPending).toBe(true);
  expect(x.raw()).toBe(original);
});

it("does not delete a working credential until replacement metadata is durably saved", async () => {
  const x = setup(); const original = await x.store.initialize(); const oldBytes = x.raw();
  x.storage.setItem = () => { throw new Error("quota"); };
  const next = await x.store.set({...original.settings, providers: original.settings!.providers!.map(p => ({...p, api_key: "replacement-synthetic"}))});
  expect(next.error).toBeTruthy();
  expect(x.api.delete).not.toHaveBeenCalled();
  expect(x.raw()).toBe(oldBytes);
});
