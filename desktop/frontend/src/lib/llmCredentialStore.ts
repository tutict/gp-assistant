import { newSecureId } from "./secureId";
import { invoke } from "@tauri-apps/api/core";
import type { LlmProviderSettings, LlmSettings } from "../types";
import { normalizeLlmSettings, sanitizePersistedLlmSettings } from "./contracts";
import { sessionCredential, setSessionCredential } from "./llmCredentialSession";

export const LLM_SETTINGS_KEY = "stock-optimizer-llm-settings";
export interface CredentialStatus { credential_ref: string; has_key: boolean; }
export interface CredentialApi {
  put(reference: string, secret: string): Promise<CredentialStatus>;
  status(reference: string): Promise<CredentialStatus>;
  delete(reference: string): Promise<CredentialStatus>;
}
// Deliberately no read/export-secret IPC method.
export const nativeCredentials: CredentialApi = {
  put: (credentialRef, secret) => invoke("api_credential_put", { credentialRef, secret }),
  status: credentialRef => invoke("api_credential_status", { credentialRef }),
  delete: credentialRef => invoke("api_credential_delete", { credentialRef }),
};
export interface CredentialSnapshot { settings: LlmSettings | null; error: string; migrationPending: boolean; }
export type LlmSettingsUpdate = LlmSettings | null | ((previous: LlmSettings | null) => LlmSettings | null);
type Storage = Pick<globalThis.Storage, "getItem" | "setItem" | "removeItem">;
const MIGRATION_ERROR = "密钥迁移失败：本次会话仍可使用；旧的明文记录尚未移除。请重试迁移，成功前不要备份或共享应用数据。";
const SESSION_ERROR = "安全存储失败：密钥仅保留在本次会话；请重试保存，关闭后需重新输入。";
const DELETE_ERROR = "删除系统密钥失败，连接配置已保留；请重试清除。";
const PERSIST_ERROR = "连接设置写入失败；本次会话仍可使用，请勿在保存成功前关闭。";
const newReference = () => `gp-assistant.llm.${newSecureId()}`;

/** One instance per app. Serializes migration, save, replace and delete, including StrictMode replay. */
export function createLlmCredentialStore(storage: Storage, api: CredentialApi = nativeCredentials) {
  let snapshot: CredentialSnapshot = { settings: null, error: "", migrationPending: false };
  let pendingLegacy: LlmSettings | undefined;
  let initialized: Promise<CredentialSnapshot> | undefined;
  let tail: Promise<unknown> = Promise.resolve();
  const checked = (value: CredentialStatus, reference: string, expected: boolean) => {
    if (value.credential_ref !== reference || value.has_key !== expected) throw new Error("Credential verification failed");
  };
  async function apply(input: LlmSettings | null, migration = false): Promise<CredentialSnapshot> {
    let error = "";
    const previous = snapshot.settings;
    const next = input ? normalizeLlmSettings(input) : null;
    const staged: LlmProviderSettings[] = [];
    // Preserve the captured legacy secret in session memory *before* any async/native work.
    for (const provider of next?.providers || []) {
      if (provider.api_key?.trim()) setSessionCredential(provider.id!, provider.api_key.trim());
    }
    for (const source of next?.providers || []) {
      const provider = { ...source };
      const old = previous?.providers?.find(p => p.id === provider.id);
      const explicitKey = Object.hasOwn(source, "api_key");
      const secret = explicitKey ? source.api_key?.trim() : (!provider.credential_ref && provider.remember_key ? sessionCredential(provider.id) : undefined);
      delete provider.api_key;
      if (explicitKey && !secret) {
        delete provider.credential_ref; provider.has_key = false;
      } else if (secret && provider.remember_key) {
        let reference: string | undefined;
        try {
          reference = newReference();
          checked(await api.put(reference, secret), reference, true);
          checked(await api.status(reference), reference, true);
          provider.credential_ref = reference; provider.has_key = true;
        } catch {
          // Never persist a new plaintext key. Pending legacy bytes are handled separately below.
          delete provider.credential_ref;
          provider.remember_key = false; provider.has_key = true;
          error = SESSION_ERROR;
          // Best-effort remove an unverified new slot; never delete an old configured slot here.
          try { if (reference) await api.delete(reference); } catch { /* Never echo native errors/secrets. */ }
          if (old?.credential_ref) {
            // Preserve the prior verified slot if replacing it failed; draft key remains session-only.
            provider.credential_ref = old.credential_ref; provider.remember_key = true;
          }
        }
      } else if (!provider.remember_key && provider.credential_ref) {
        // Stored secrets cannot be exported into a JS session. Turning remember off removes it.
        delete provider.credential_ref; provider.has_key = Boolean(secret || sessionCredential(provider.id));
      } else if (provider.credential_ref && migration) {
        try { checked(await api.status(provider.credential_ref), provider.credential_ref, true); provider.has_key = true; }
        catch { provider.has_key = false; error = "系统密钥不可用，请重新输入或重试；连接引用已保留。"; }
      } else { provider.has_key = Boolean(provider.credential_ref || secret || sessionCredential(provider.id)); }
      staged.push(provider);
    }
    if (next) next.providers = staged;
    if (migration && pendingLegacy && error) {
      // Original storage is intentionally untouched until ALL remembered keys verify.
      return snapshot = { settings: next, error: MIGRATION_ERROR, migrationPending: true };
    }
    // Publish only secret-free metadata before deleting any previous working credential.
    // A quota failure must not strand the old persisted settings with a deleted key.
    try {
      storage.setItem(LLM_SETTINGS_KEY, JSON.stringify(sanitizePersistedLlmSettings(next)));
      if (migration) pendingLegacy = undefined;
    } catch {
      error = pendingLegacy ? MIGRATION_ERROR + " 设置写入失败。" : PERSIST_ERROR;
      return snapshot = { settings: next, error, migrationPending: Boolean(pendingLegacy) };
    }
    const nextRefs = new Set(staged.map(p => p.credential_ref).filter(Boolean));
    const deletedRefs = new Set<string>();
    for (const old of previous?.providers || []) {
      if (old.credential_ref && !nextRefs.has(old.credential_ref)) {
        try {
          checked(await api.delete(old.credential_ref), old.credential_ref, false);
          deletedRefs.add(old.credential_ref);
        } catch {
          if (migration) {
            // All legacy secrets have verified new references; do not roll back to session-only metadata.
            return snapshot = { settings: next, error: DELETE_ERROR, migrationPending: false };
          }
          const survivors = previous?.providers?.filter(p => !p.credential_ref || !deletedRefs.has(p.credential_ref));
          const retained = survivors?.length ? normalizeLlmSettings({ ...previous, providers: survivors }) : null;
          let deletionError = DELETE_ERROR;
          try { storage.setItem(LLM_SETTINGS_KEY, JSON.stringify(sanitizePersistedLlmSettings(retained))); }
          catch { deletionError += " " + PERSIST_ERROR; }
          return snapshot = { settings: retained, error: deletionError, migrationPending: false };
        }
      }
    }
    for (const old of previous?.providers || []) {
      if (!staged.some(p => p.id === old.id)) setSessionCredential(old.id!);
    }
    for (const provider of staged) {
      if (provider.credential_ref) setSessionCredential(provider.id!);
      // An explicit clear never leaves the former session key behind.
      if (input?.providers?.some(p => p.id === provider.id && Object.hasOwn(p, "api_key") && !p.api_key?.trim())) setSessionCredential(provider.id!);
    }
    return snapshot = { settings: next, error, migrationPending: Boolean(pendingLegacy) };
  }
  function initialize(): Promise<CredentialSnapshot> {
    if (!initialized) initialized = (async () => {
      let raw: string | null;
      try { raw = storage.getItem(LLM_SETTINGS_KEY); }
      catch { return snapshot = { settings: null, error: "无法读取连接设置，请重试。", migrationPending: false }; }
      let parsed: LlmSettings | null;
      try {
        parsed = raw ? JSON.parse(raw) : null;
        if (parsed !== null && (typeof parsed !== "object" || Array.isArray(parsed))) throw new Error("shape");
        if (parsed?.providers !== undefined && (!Array.isArray(parsed.providers) || parsed.providers.some(p => !p || typeof p !== "object"))) throw new Error("shape");
        const normalized = parsed ? normalizeLlmSettings(parsed) : null;
        if (normalized?.providers?.some(p => (p.api_key !== undefined && typeof p.api_key !== "string")
          || (p.credential_ref !== undefined && typeof p.credential_ref !== "string"))) throw new Error("shape");
        if (normalized?.providers?.some(p => p.remember_key && p.api_key)) pendingLegacy = parsed!;
      }
      catch { return snapshot = { settings: null, error: "连接设置损坏；未覆盖原始记录。", migrationPending: false }; }
      return apply(parsed, true);
    })();
    return initialized;
  }
  function set(update: LlmSettingsUpdate): Promise<CredentialSnapshot> {
    const work = tail.then(async () => {
      await initialize();
      if (pendingLegacy) return snapshot = { ...snapshot, error: MIGRATION_ERROR, migrationPending: true };
      return apply(typeof update === "function" ? update(snapshot.settings) : update);
    });
    tail = work.catch(() => undefined);
    return work;
  }
  function retryMigration(): Promise<CredentialSnapshot> {
    const work = tail.then(async () => {
      await initialize();
      return pendingLegacy ? apply(pendingLegacy, true) : snapshot;
    });
    tail = work.catch(() => undefined);
    return work;
  }
  return { initialize, set, retryMigration, getSnapshot: () => snapshot };
}
