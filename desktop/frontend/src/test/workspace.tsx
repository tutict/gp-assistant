import { createWorkspaceStore, type WorkspaceSnapshot, type WorkspaceTransport } from "../lib/workspaceStore";
import type { LegacyStorage } from "../lib/workspaceSchema";
/** Synthetic storage only. Tests must explicitly provide their localStorage double for migration. */
export function workspaceTestHarness(legacy?: LegacyStorage, initial: Record<string, unknown> = {}) {
  let snapshot: WorkspaceSnapshot = { schemaVersion: 1, revision: 0, values: initial };
  const commits: Record<string, unknown>[] = [];
  const transport: WorkspaceTransport = {
    load: async () => structuredClone(snapshot),
    commit: async (revision, changes) => {
      if (revision !== snapshot.revision) throw new Error("revision conflict");
      const values = { ...snapshot.values, ...structuredClone(changes) };
      for (const key of Object.keys(values)) if (values[key] === null) delete values[key];
      snapshot = { schemaVersion: 1, revision: revision + 1, values }; commits.push(changes); return snapshot.revision;
    },
  };
  return { store: createWorkspaceStore(transport, legacy), transport, commits, snapshot: () => structuredClone(snapshot) };
}
