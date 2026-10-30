import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { LlmConnectionTestResult, LlmModelListResult, LlmModelOption, LlmProviderSettings, LlmSettings } from "../types";
import { activeLlmProvider, normalizeLlmSettings } from "../lib/contracts";
import { CredentialSaveError, type CredentialSnapshot, type LlmSettingsSetter, type LlmSettingsUpdate } from "../lib/llmCredentialStore";
import { sessionCredentialVersion } from "../lib/llmCredentialSession";
import { postJson } from "../lib/tauri";
import {
  draftProvider, isDraftDirty, llmDraftPayload, makeProviderDraft, newProviderDraft, normalizeLlmDiagnostic,
  patchProviderDraft, requestChanged, safeLlmEndpoint, sameSavedProvider, saveDraftUpdate, validateLlmProvider,
  type CredentialDraft, type LlmDiagnostic, type LlmProviderDraft,
} from "../lib/llmSettingsDraft";

export interface LlmConnectionState {
  state: "testing" | "success" | "error"; revision: number;
  result?: LlmConnectionTestResult; error?: LlmDiagnostic;
}
export interface LlmModelCatalog {
  models: LlmModelOption[]; loaded: boolean; loading: boolean; error?: LlmDiagnostic;
}
function initialDrafts(settings: LlmSettings | null) {
  return Object.fromEntries(normalizeLlmSettings(settings).providers!.map(provider =>
    [provider.id!, makeProviderDraft(provider, Boolean(settings))]));
}

export function useLlmSettingsDrafts(settings: LlmSettings | null, onChange: LlmSettingsSetter) {
  const [drafts, setDrafts] = useState<Record<string, LlmProviderDraft>>(() => initialDrafts(settings));
  const draftRef = useRef(drafts);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const [editingId, setEditingId] = useState(() => activeLlmProvider(settings)?.id || Object.keys(drafts)[0]);
  const [connections, setConnections] = useState<Record<string, LlmConnectionState>>({});
  const connectionRef = useRef(connections);
  const [catalogs, setCatalogs] = useState<Record<string, LlmModelCatalog>>({});
  const catalogRef = useRef(catalogs);
  const requestTokens = useRef<Record<string, number>>({});
  const modelTokens = useRef<Record<string, number>>({});
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [feedback, setFeedback] = useState<{ text: string; error: boolean }>({ text: "", error: false });
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const putDrafts = useCallback((next: Record<string, LlmProviderDraft>) => { draftRef.current = next; setDrafts(next); }, []);
  const putConnections = useCallback((next: Record<string, LlmConnectionState>) => { connectionRef.current = next; setConnections(next); }, []);
  const putCatalogs = useCallback((next: Record<string, LlmModelCatalog>) => { catalogRef.current = next; setCatalogs(next); }, []);
  const invalidate = useCallback((id: string, catalog = true) => {
    requestTokens.current[id] = (requestTokens.current[id] || 0) + 1;
    const nextConnections = { ...connectionRef.current }; delete nextConnections[id]; putConnections(nextConnections);
    if (catalog) {
      modelTokens.current[id] = (modelTokens.current[id] || 0) + 1;
      const nextCatalogs = { ...catalogRef.current }; delete nextCatalogs[id]; putCatalogs(nextCatalogs);
    }
  }, [putConnections, putCatalogs]);

  useEffect(() => {
    const saved = normalizeLlmSettings(settings).providers!;
    const next = { ...draftRef.current };
    for (const [id, draft] of Object.entries(next)) {
      if (isDraftDirty(draft)) continue;
      const provider = saved.find(item => item.id === id);
      if (!provider) { delete next[id]; invalidate(id); }
      else if (!sameSavedProvider(provider, draft.baseline) || sessionCredentialVersion(id) !== draft.credentialVersion) {
        const changedRequest = requestChanged(draft.provider, draftProvider(provider)) || sessionCredentialVersion(id) !== draft.credentialVersion;
        if (changedRequest) invalidate(id);
        next[id] = { ...makeProviderDraft(provider, Boolean(settings)), revision: draft.revision + Number(changedRequest), catalogRevision: draft.catalogRevision + Number(changedRequest) };
      }
    }
    for (const provider of saved) if (!next[provider.id!]) next[provider.id!] = makeProviderDraft(provider, Boolean(settings));
    putDrafts(next);
    setEditingId(current => next[current] ? current : Object.keys(next)[0]);
  }, [settings, invalidate, putDrafts]);

  const patch = useCallback((id: string, values: Partial<LlmProviderSettings>) => {
    if (busyRef.current || !draftRef.current[id]) return;
    const previous = draftRef.current[id];
    const next = patchProviderDraft(previous, values);
    if (next.revision !== previous.revision) invalidate(id, next.catalogRevision !== previous.catalogRevision);
    putDrafts({ ...draftRef.current, [id]: next });
    setFeedback({ text: "", error: false });
  }, [invalidate, putDrafts]);
  const credential = useCallback((id: string, value: CredentialDraft) => {
    if (busyRef.current || !draftRef.current[id]) return;
    const draft = draftRef.current[id];
    invalidate(id);
    putDrafts({ ...draftRef.current, [id]: { ...draft, credential: value, revision: draft.revision + 1, catalogRevision: draft.catalogRevision + 1 } });
    setFeedback({ text: "", error: false });
  }, [invalidate, putDrafts]);
  const add = useCallback((preset?: Partial<LlmProviderSettings>) => {
    if (busyRef.current) return;
    const draft = newProviderDraft(preset);
    putDrafts({ ...draftRef.current, [draft.provider.id!]: draft });
    setEditingId(draft.provider.id!);
    setFeedback({ text: "", error: false });
  }, [putDrafts]);
  const reset = useCallback(() => {
    const next = initialDrafts(settingsRef.current);
    for (const [id, draft] of Object.entries(draftRef.current)) {
      if (!isDraftDirty(draft) && next[id] && sameSavedProvider(next[id].provider, draft.provider)) {
        next[id] = { ...next[id], revision: draft.revision, catalogRevision: draft.catalogRevision };
      }
    }
    for (const [id, draft] of Object.entries(draftRef.current)) if (isDraftDirty(draft)) invalidate(id);
    putDrafts(next);
    setEditingId(normalizeLlmSettings(settingsRef.current).providers![0].id!);
    setFeedback({ text: "", error: false });
  }, [invalidate, putDrafts]);
  const reload = useCallback((id: string) => {
    if (busyRef.current) return;
    invalidate(id);
    const provider = normalizeLlmSettings(settingsRef.current).providers!.find(item => item.id === id);
    const next = { ...draftRef.current };
    if (provider) next[id] = makeProviderDraft(provider, Boolean(settingsRef.current));
    else delete next[id];
    putDrafts(next); setEditingId(current => next[current] ? current : Object.keys(next)[0]);
    setFeedback({ text: "", error: false });
  }, [invalidate, putDrafts]);

  const test = useCallback(async (id: string) => {
    const draft = draftRef.current[id];
    if (!draft || busyRef.current || connectionRef.current[id]?.state === "testing") return;
    const errors = validateLlmProvider(draft.provider, true);
    if (Object.keys(errors).length) {
      putConnections({ ...connectionRef.current, [id]: { state: "error", revision: draft.revision,
        error: { code: "invalid_config", message: Object.values(errors)[0]! } } }); return;
    }
    const token = requestTokens.current[id] = (requestTokens.current[id] || 0) + 1;
    putConnections({ ...connectionRef.current, [id]: { state: "testing", revision: draft.revision } });
    const valid = () => mounted.current && requestTokens.current[id] === token && draftRef.current[id]?.revision === draft.revision;
    try {
      const result = await postJson<LlmConnectionTestResult>("/api/llm/test", llmDraftPayload(draft), { timeoutMs: draft.provider.timeout! * 1000 + 5000 });
      if (!valid()) return;
      if (result.ok !== true) throw { code: "invalid_response", message: "未收到有效的连接测试结果，请重试。" };
      putConnections({ ...connectionRef.current, [id]: { state: "success", revision: draft.revision,
        result: { ...result, endpoint: safeLlmEndpoint(result.endpoint) } } });
    } catch (error) {
      if (valid()) putConnections({ ...connectionRef.current, [id]: { state: "error", revision: draft.revision,
        error: normalizeLlmDiagnostic(error, draft.credential.kind === "replace" ? draft.credential.key : draft.provider.api_key) } });
    }
  }, [putConnections]);
  const fetchModels = useCallback(async (id: string) => {
    const draft = draftRef.current[id];
    if (!draft || busyRef.current || catalogRef.current[id]?.loading) return;
    const errors = validateLlmProvider(draft.provider);
    if (!draft.provider.base_url?.trim() || Object.keys(errors).length) {
      putCatalogs({ ...catalogRef.current, [id]: { models: [], loaded: false, loading: false,
        error: { code: "invalid_config", message: Object.values(errors)[0] || "请先填写接口地址。" } } }); return;
    }
    const token = modelTokens.current[id] = (modelTokens.current[id] || 0) + 1;
    const previous = catalogRef.current[id];
    putCatalogs({ ...catalogRef.current, [id]: { models: previous?.models || [], loaded: previous?.loaded || false, loading: true } });
    const valid = () => mounted.current && modelTokens.current[id] === token && draftRef.current[id]?.catalogRevision === draft.catalogRevision;
    try {
      const result = await postJson<LlmModelListResult>("/api/llm/models", llmDraftPayload(draft), { timeoutMs: draft.provider.timeout! * 1000 + 5000 });
      if (!valid()) return;
      if (!Array.isArray(result.models)) throw { code: "invalid_response", message: "模型列表响应格式异常，可继续手动填写模型 ID。" };
      const seen = new Set<string>();
      const models = result.models.filter(item => typeof item?.id === "string" && item.id.trim() && !seen.has(item.id.trim()) && Boolean(seen.add(item.id.trim())))
        .map(item => ({ ...item, id: item.id.trim() }));
      putCatalogs({ ...catalogRef.current, [id]: { models, loaded: true, loading: false } });
    } catch (error) {
      if (valid()) putCatalogs({ ...catalogRef.current, [id]: { models: previous?.models || [], loaded: previous?.loaded || false, loading: false,
        error: normalizeLlmDiagnostic(error, draft.credential.kind === "replace" ? draft.credential.key : draft.provider.api_key) } });
    }
  }, [putCatalogs]);

  const commit = useCallback(async (update: LlmSettingsUpdate): Promise<CredentialSnapshot> => {
    let computed: LlmSettings | null | undefined;
    const result = await onChange(previous => { computed = typeof update === "function" ? update(previous) : update; return computed; });
    if (result?.error) throw new CredentialSaveError(result);
    if (result) return result;
    if (computed === undefined) throw new Error("保存入口未提交配置，请稍后重试。");
    return { settings: computed, error: "", migrationPending: false };
  }, [onChange]);
  const save = useCallback(async (id: string, activate = false) => {
    const draft = draftRef.current[id];
    if (!draft || busyRef.current) return;
    const active = activeLlmProvider(settingsRef.current);
    const appliesChanges = active?.id === id && (requestChanged(draft.provider, draft.baseline) || draft.credential.kind !== "keep");
    const errors = validateLlmProvider(draft.provider, activate || appliesChanges);
    const tested = connectionRef.current[id]?.state === "success" && connectionRef.current[id].revision === draft.revision;
    if (Object.keys(errors).length || ((activate || appliesChanges) && !tested)) {
      setFeedback({ text: Object.values(errors)[0] || "请先用当前草稿测试连接，通过后再启用或应用修改。", error: true }); return;
    }
    busyRef.current = true; setBusy(true); setFeedback({ text: "", error: false });
    try {
      const result = await commit(previous => {
        if (draftRef.current[id] !== draft) throw new Error("草稿已变化，请重新提交。");
        if (!tested && activeLlmProvider(previous)?.id === id
          && (requestChanged(draft.provider, draft.baseline) || draft.credential.kind !== "keep")) {
          throw new Error("连接已在其他入口启用，请先测试当前草稿后再保存。");
        }
        return saveDraftUpdate(previous, draft, activate);
      });
      if (!mounted.current) return;
      const provider = result.settings?.providers?.find(item => item.id === id);
      if (!provider) throw new Error("连接未保存，请重新载入配置后重试。");
      settingsRef.current = result.settings;
      putDrafts({ ...draftRef.current, [id]: { ...makeProviderDraft(provider, true), revision: draft.revision, catalogRevision: draft.catalogRevision } });
      setFeedback({ text: activate ? "已保存并启用此连接。" : active?.id === id ? "已保存，当前连接已更新。" : "已保存此连接，当前使用的连接保持不变。", error: false });
    } catch (error) {
      if (mounted.current) setFeedback({ text: error instanceof CredentialSaveError ? error.snapshot.error
        : normalizeLlmDiagnostic(error, draft.credential.kind === "replace" ? draft.credential.key : undefined).message, error: true });
    } finally { busyRef.current = false; if (mounted.current) setBusy(false); }
  }, [commit, putDrafts]);
  const remove = useCallback(async (id?: string) => {
    if (busyRef.current) return false;
    const draft = id ? draftRef.current[id] : undefined;
    if (id && (!draft || activeLlmProvider(settingsRef.current)?.id === id)) {
      setFeedback({ text: "请先启用其他连接，再删除当前使用的连接。", error: true }); return false;
    }
    busyRef.current = true; setBusy(true);
    try {
      if (!id || draft?.persisted) {
        const result = await commit(previous => {
          if (!id) return null;
          const current = normalizeLlmSettings(previous);
          const provider = current.providers!.find(item => item.id === id);
          if (!provider || !sameSavedProvider(provider, draft!.baseline)
            || sessionCredentialVersion(id) !== draft!.credentialVersion) throw new Error("连接已变化，请重新载入后删除。");
          if (activeLlmProvider(previous)?.id === id) throw new Error("当前连接已启用，请先切换到其他连接。");
          const providers = current.providers!.filter(item => item.id !== id);
          return providers.length ? { ...current, providers } : null;
        });
        settingsRef.current = result.settings;
      }
      if (!mounted.current) return false;
      if (!id) reset();
      else {
        invalidate(id);
        const next = { ...draftRef.current }; delete next[id];
        if (!Object.keys(next).length) Object.assign(next, initialDrafts(settingsRef.current));
        putDrafts(next); setEditingId(current => next[current] ? current : Object.keys(next)[0]);
      }
      setFeedback({ text: id ? "连接已删除。" : "所有连接已清空。", error: false });
      return true;
    } catch (error) {
      if (mounted.current) setFeedback({ text: error instanceof CredentialSaveError ? error.snapshot.error : normalizeLlmDiagnostic(error).message, error: true });
      return false;
    } finally { busyRef.current = false; if (mounted.current) setBusy(false); }
  }, [commit, invalidate, putDrafts, reset]);

  const editingDraft = drafts[editingId] || Object.values(drafts)[0];
  const active = useMemo(() => activeLlmProvider(settings), [settings]);
  const currentId = editingDraft?.provider.id || "";
  const connection = connections[currentId]?.revision === editingDraft?.revision ? connections[currentId] : undefined;
  const tested = connection?.state === "success" && connection.revision === editingDraft?.revision;
  const dirty = editingDraft ? isDraftDirty(editingDraft) : false;
  const needsTest = active?.id === currentId && Boolean(editingDraft && (requestChanged(editingDraft.provider, editingDraft.baseline) || editingDraft.credential.kind !== "keep"));
  const errors = editingDraft ? validateLlmProvider(editingDraft.provider) : {};
  return { drafts, editingId: currentId, editingDraft, select: setEditingId, active, connections,
    connection, catalogs, catalog: catalogs[currentId], tested, dirty, needsTest, errors, busy, feedback,
    unsavedCount: Object.values(drafts).filter(isDraftDirty).length,
    canSave: dirty && !busy && !Object.keys(errors).length && (!needsTest || tested),
    canActivate: active?.id !== currentId && tested && !busy && Boolean(editingDraft && !Object.keys(validateLlmProvider(editingDraft.provider, true)).length),
    patch, credential, add, reset, reload, test, fetchModels, save, remove };
}
