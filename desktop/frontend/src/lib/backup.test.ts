import { beforeEach, describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { exportUserBackup, previewUserBackup, stageUserBackup, restoreEmptyUserBackup, mergeAgentUserBackup, readBackupFile, MAX_BACKUP_BYTES } from "./backup";

describe("encrypted user backup transport", () => {
  beforeEach(() => { invoke.mockReset(); });
  it("uses dedicated commands, never research sync routes", async () => {
    invoke.mockResolvedValue({ state: "preview_only", imported: false });
    await exportUserBackup("long passphrase");
    await previewUserBackup("YWJj", "long passphrase");
    await stageUserBackup("YWJj", "long passphrase");
    await restoreEmptyUserBackup("YWJj", "long passphrase");
    await mergeAgentUserBackup("YWJj", "long passphrase");
    expect(invoke.mock.calls).toEqual([
      ["api_user_backup_export", { payload: { passphrase: "long passphrase" } }],
      ["api_user_backup_preview", { payload: { blob_base64: "YWJj", passphrase: "long passphrase" } }],
      ["api_user_backup_stage", { payload: { blob_base64: "YWJj", passphrase: "long passphrase" } }],
      ["api_user_backup_restore_empty", { payload: { blob_base64: "YWJj", passphrase: "long passphrase" } }],
      ["api_user_backup_merge_agent", { payload: { blob_base64: "YWJj", passphrase: "long passphrase" } }],
    ]);
  });
  it("rejects oversize input before reading or invoking", async () => {
    const arrayBuffer = vi.fn();
    await expect(readBackupFile({ size: MAX_BACKUP_BYTES + 1, arrayBuffer } as unknown as File)).rejects.toThrow();
    expect(arrayBuffer).not.toHaveBeenCalled();
    await expect(previewUserBackup("x".repeat(Math.ceil(MAX_BACKUP_BYTES / 3) * 4 + 1), "long passphrase")).rejects.toThrow();
    expect(invoke).not.toHaveBeenCalled();
  });
  it("never propagates backend exception strings containing secrets", async () => {
    invoke.mockRejectedValue(new Error("Bearer secret-value passphrase=long passphrase"));
    await expect(exportUserBackup("long passphrase")).rejects.toThrow("备份操作失败");
    await expect(previewUserBackup("YWJj", "long passphrase")).rejects.not.toThrow("secret-value");
  });
  it("reads local files as bounded base64 without a filesystem permission", async () => {
    const file = { size: 3, arrayBuffer: async () => new Uint8Array([97, 98, 99]).buffer } as File;
    expect(await readBackupFile(file)).toBe("YWJj");
  });
});


