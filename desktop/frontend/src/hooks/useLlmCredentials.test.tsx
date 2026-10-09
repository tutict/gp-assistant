import { StrictMode } from "react";
import { act, create } from "react-test-renderer";
import { afterEach, expect, it, vi } from "vitest";
import { useLlmCredentials } from "./useLlmCredentials";
import { createLlmCredentialStore, type CredentialApi } from "../lib/llmCredentialStore";
import { clearSessionCredentials } from "../lib/llmCredentialSession";

afterEach(() => { clearSessionCredentials(); vi.unstubAllGlobals(); });
it("migrates once under StrictMode, exposes readiness and a safe error", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const put = vi.fn(async () => { throw new Error("synthetic-secret"); });
  const api: CredentialApi = { put, status: vi.fn(), delete: vi.fn() };
  const writes: string[] = [];
  const store = createLlmCredentialStore({getItem: () => JSON.stringify({providers:[{id:"a",model:"m",api_key:"synthetic-secret",remember_key:true}]}),
    setItem: (_, value) => { writes.push(value); }, removeItem: vi.fn()}, api);
  let state!: ReturnType<typeof useLlmCredentials>;
  function Harness() { state = useLlmCredentials(store); return null; }
  let renderer!: ReturnType<typeof create>;
  await act(async () => { renderer = create(<StrictMode><Harness /></StrictMode>); });
  expect(state.credentialsReady).toBe(true);
  expect(state.credentialError).toMatch(/本次/);
  expect(put).toHaveBeenCalledTimes(1);
  expect(JSON.stringify(state.llmSettings)).not.toContain("synthetic-secret");
  expect(writes).toEqual([]);
  expect(state.credentialMigrationPending).toBe(true);
  await act(async () => renderer.unmount());
});
