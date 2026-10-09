import { invoke } from "@tauri-apps/api/core";

/** Matches the Rust wire limit, including the authenticated header and GCM tag. */
export const MAX_BACKUP_BYTES = 64 * 1024 * 1024;
const MAX_BASE64_BYTES = Math.ceil(MAX_BACKUP_BYTES / 3) * 4;
const FAILURE = "备份操作失败。请检查口令、文件格式、大小及磁盘空间后重试。";

export interface BackupStorePreview {
  id: string;
  relative_path: string;
  bytes: number;
  tables: number;
  rows: number;
  filtered_fields: number;
  omitted_tables: number;
  current: "absent" | "present_valid" | "present_unreadable";
  conflict: boolean;
  restore_to_empty: boolean;
  restore_status: string;
  merge_available: boolean;
  receipt_id: string | null;
}
export interface BackupExport {
  blob_base64: string;
  filename: string;
  stores: BackupStorePreview[];
  missing_stores: string[];
}
export interface BackupPreview {
  /** Only explicitly listed restored_stores are live; staging is not a restore. */
  state: "preview_only" | "imported" | "partially_imported";
  imported: boolean;
  staged: boolean;
  recovery_id: string | null;
  restored_stores: string[];
  restoration_note: string;
  created_at_epoch_ms: number;
  stores: BackupStorePreview[];
  missing_stores: string[];
}

function checkBlob(blob: string) {
  if (!blob || blob.length > MAX_BASE64_BYTES || !/^[A-Za-z0-9+/]*={0,2}$/.test(blob)) {
    throw new Error("备份文件为空、过大或格式无效（最大 64 MiB）。");
  }
}
function checkPassphrase(passphrase: string) {
  const size = new TextEncoder().encode(passphrase).byteLength;
  if (size < 12 || size > 1024 || !passphrase.trim()) {
    throw new Error("备份口令必须为 12–1024 个 UTF-8 字节。");
  }
}
async function call<T>(command: string, payload: Record<string, string>): Promise<T> {
  try {
    // No logging, storage, URL arguments, analytics, or generic request middleware.
    return await invoke<T>(command, { payload });
  } catch {
    // Never reflect backend exception text: it may originate in a credential-bearing field.
    throw new Error(FAILURE);
  }
}
export async function exportUserBackup(passphrase: string): Promise<BackupExport> {
  checkPassphrase(passphrase);
  return call("api_user_backup_export", { passphrase });
}
export async function previewUserBackup(blob: string, passphrase: string): Promise<BackupPreview> {
  checkBlob(blob); checkPassphrase(passphrase);
  return call("api_user_backup_preview", { blob_base64: blob, passphrase });
}
export async function stageUserBackup(blob: string, passphrase: string): Promise<BackupPreview> {
  checkBlob(blob); checkPassphrase(passphrase);
  return call("api_user_backup_stage", { blob_base64: blob, passphrase });
}
export async function restoreEmptyUserBackup(blob: string, passphrase: string): Promise<BackupPreview> {
  checkBlob(blob); checkPassphrase(passphrase);
  return call("api_user_backup_restore_empty", { blob_base64: blob, passphrase });
}
export async function mergeAgentUserBackup(blob: string, passphrase: string): Promise<BackupPreview> {
  checkBlob(blob); checkPassphrase(passphrase);
  return call("api_user_backup_merge_agent", { blob_base64: blob, passphrase });
}
export async function readBackupFile(file: File): Promise<string> {
  if (file.size === 0 || file.size > MAX_BACKUP_BYTES) throw new Error("备份文件为空或超过 64 MiB。");
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (!bytes.length || bytes.length > MAX_BACKUP_BYTES) throw new Error("备份文件大小无效。");
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 8192) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 8192)));
  }
  return btoa(chunks.join(""));
}
export function downloadUserBackup(blob: string, filename: string): void {
  checkBlob(blob);
  const binary = atob(blob);
  if (binary.length > MAX_BACKUP_BYTES) throw new Error("备份文件超过 64 MiB。");
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
  const anchor = document.createElement("a");
  try {
    anchor.href = url;
    // Do not trust a response filename as a path or extension.
    anchor.download = /^gp-user-backup-\d+\.gpbackup$/.test(filename) ? filename : "gp-user-backup.gpbackup";
    anchor.hidden = true;
    document.body.appendChild(anchor);
    anchor.click();
  } finally {
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
}


