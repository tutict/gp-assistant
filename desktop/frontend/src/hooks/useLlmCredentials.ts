import { useCallback, useEffect, useState } from "react";
import { createLlmCredentialStore, type CredentialSnapshot, type LlmSettingsUpdate } from "../lib/llmCredentialStore";

let sharedStore: ReturnType<typeof createLlmCredentialStore> | undefined;
function appStore() { return sharedStore ??= createLlmCredentialStore(window.localStorage); }

/** Replaces BOTH App's useLocalStorage and mirrored LLM useState. Do not sanitize on read. */
export function useLlmCredentials(store = appStore()) {
  const [snapshot, setSnapshot] = useState<CredentialSnapshot>(() => store.getSnapshot());
  const [ready, setReady] = useState(false);
  useEffect(() => {
    let alive = true;
    void store.initialize().then(value => { if (alive) { setSnapshot(value); setReady(true); } });
    return () => { alive = false; };
  }, [store]);
  const setLlmSettings = useCallback(async (update: LlmSettingsUpdate) => {
    const next = await store.set(update);
    setSnapshot(next);
    if (next.error) throw new Error(next.error);
  }, [store]);
  const retryCredentialMigration = useCallback(async () => {
    const next = await store.retryMigration();
    setSnapshot(next);
  }, [store]);
  return { llmSettings: snapshot.settings, setLlmSettings, credentialError: snapshot.error,
    credentialsReady: ready, credentialMigrationPending: snapshot.migrationPending, retryCredentialMigration };

}
