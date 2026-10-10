import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ exportUserBackup: vi.fn(), previewUserBackup: vi.fn(), stageUserBackup: vi.fn(), restoreEmptyUserBackup: vi.fn(), mergeAgentUserBackup: vi.fn(), readBackupFile: vi.fn(), downloadUserBackup: vi.fn() }));
vi.mock("../../lib/backup", () => api);
import { BackupPanel } from "./BackupPanel";
const preview = { state: "preview_only", imported: false, staged: false, recovery_id: null, restored_stores: [], restoration_note: "", created_at_epoch_ms: 1, stores: [{ id: "watchlist", relative_path: "watchlist/watchlist.sqlite", bytes: 8192, tables: 1, rows: 1, filtered_fields: 0, omitted_tables: 0, current: "present_valid", conflict: true, restore_to_empty: false, restore_status: "preserved_current", merge_available: false, receipt_id: null }], missing_stores: ["client_state"] };
let renderer: ReactTestRenderer;
const button = (label: string) => renderer.root.findByProps({ "aria-label": label });
const password = () => renderer.root.findByProps({ "aria-label": "备份口令" });
async function mount() { await act(async () => { renderer = create(<BackupPanel />); }); }
async function enterPassword() { await act(async () => password().props.onChange({ currentTarget: { value: "long passphrase" } })); }
async function selectFile() { await act(async () => renderer.root.findByProps({ type: "file" }).props.onChange({ currentTarget: { files: [{ name: "backup.gpbackup" }], value: "file" } })); }
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  api.readBackupFile.mockResolvedValue("encrypted-base64");
  api.previewUserBackup.mockResolvedValue(preview);
  api.stageUserBackup.mockResolvedValue({ ...preview, staged: true, recovery_id: "recovery-123" });
  api.exportUserBackup.mockResolvedValue({ blob_base64: "encrypted", filename: "backup.gpbackup", stores: preview.stores });
});
afterEach(async () => { if (renderer) await act(async () => renderer.unmount()); vi.unstubAllGlobals(); });
describe("BackupPanel", () => {
  it("separates backup and restore tasks while keeping detailed recovery rules on demand", async () => {
    await mount();
    const text = JSON.stringify(renderer.toJSON());
    expect(text).toContain("导出备份");
    expect(text).toContain("从备份恢复");
    expect(text).toContain("研究、情绪和客户端状态目前仅可隔离暂存，不能恢复到应用");
    const details = renderer.root.findByProps({ className: "backup-policy-disclosure" });
    expect(details.props.open).not.toBe(true);
    expect(details.findByType("summary").props.children).toBe("查看备份范围与恢复规则");
  });

  it("offers transactional recovery for an explicitly pristine initialized database", async () => {
    api.previewUserBackup.mockResolvedValueOnce({ ...preview, stores: [{ ...preview.stores[0], restore_to_empty: true, restore_status: "eligible_pristine_watchlist_v2" }] });
    await mount(); await selectFile(); await enterPassword();
    await act(async () => button("解密并预览").props.onClick());
    expect(JSON.stringify(renderer.toJSON())).toContain("全新初始化库（尚未使用）");
    await enterPassword();
    await act(async () => renderer.root.findByProps({ "aria-label": "确认恢复缺失库" }).props.onChange({ currentTarget: { checked: true } }));
    expect(button("恢复缺失或全新库").props.disabled).toBe(false);
  });
  it("requires separate agent-merge confirmation and reports partial recovery precisely", async () => {
    const agent = { ...preview.stores[0], id: "agent_ledger", relative_path: "agent/agent-runs.sqlite", merge_available: true };
    api.previewUserBackup.mockResolvedValueOnce({ ...preview, stores: [agent, preview.stores[0]] });
    api.mergeAgentUserBackup.mockResolvedValueOnce({ ...preview, state: "partially_imported", imported: true, staged: true, restored_stores: ["agent_ledger"], stores: [{ ...agent, merge_available: false }, preview.stores[0]] });
    await mount(); await selectFile(); await enterPassword();
    await act(async () => button("解密并预览").props.onClick());
    await enterPassword();
    expect(button("安全合并代理记录").props.disabled).toBe(true);
    await act(async () => renderer.root.findByProps({ "aria-label": "确认代理记录合并" }).props.onChange({ currentTarget: { checked: true } }));
    await act(async () => button("安全合并代理记录").props.onClick());
    expect(api.mergeAgentUserBackup).toHaveBeenCalledWith("encrypted-base64", "long passphrase");
    expect(JSON.stringify(renderer.toJSON())).toContain("已合并：agent_ledger");
    expect(JSON.stringify(renderer.toJSON())).toContain("其他库未恢复");
  });
  it("restores only a missing supported store after a separate explicit confirmation", async () => {
    const missing = { ...preview, stores: [{ ...preview.stores[0], current: "absent", conflict: false, restore_to_empty: true }] };
    api.previewUserBackup.mockResolvedValueOnce(missing);
    api.restoreEmptyUserBackup.mockResolvedValueOnce({ ...missing, state: "imported", imported: true, staged: true, recovery_id: "recovery-123", restored_stores: ["watchlist"] });
    await mount(); await selectFile(); await enterPassword();
    await act(async () => button("解密并预览").props.onClick());
    await enterPassword();
    expect(button("恢复缺失或全新库").props.disabled).toBe(true);
    await act(async () => renderer.root.findByProps({ "aria-label": "确认恢复缺失库" }).props.onChange({ currentTarget: { checked: true } }));
    await act(async () => button("恢复缺失或全新库").props.onClick());
    expect(api.restoreEmptyUserBackup).toHaveBeenCalledWith("encrypted-base64", "long passphrase");
    expect(JSON.stringify(renderer.toJSON())).toContain("已恢复：watchlist");
    expect(password().props.value).toBe("");
  });
  it("requires preview and explicit confirmation; never reports staging as restoration", async () => {
    await mount();
    expect(button("暂存恢复数据").props.disabled).toBe(true);
    await selectFile(); await enterPassword();
    await act(async () => button("解密并预览").props.onClick());
    expect(api.stageUserBackup).not.toHaveBeenCalled();
    expect(password().props.value).toBe("");
    expect(JSON.stringify(renderer.toJSON())).toContain("当前数据库存在，保留当前数据");
    await enterPassword();
    await act(async () => renderer.root.findByProps({ "aria-label": "确认仅隔离暂存" }).props.onChange({ currentTarget: { checked: true } }));
    await act(async () => button("暂存恢复数据").props.onClick());
    expect(api.stageUserBackup).toHaveBeenCalledWith("encrypted-base64", "long passphrase");
    expect(JSON.stringify(renderer.toJSON())).toContain("尚未恢复到应用");
    expect(password().props.value).toBe("");
    expect(button("暂存恢复数据").props.disabled).toBe(true);
  });
  it("clears the passphrase on failure and never renders raw exception details", async () => {
    api.exportUserBackup.mockRejectedValue(new Error("sensitive-value"));
    await mount(); await enterPassword();
    await act(async () => button("导出加密备份").props.onClick());
    expect(password().props.value).toBe("");
    expect(JSON.stringify(renderer.toJSON())).not.toContain("sensitive-value");
    expect(api.downloadUserBackup).not.toHaveBeenCalled();
  });
  it("invalidates preview and confirmation when a different file is selected", async () => {
    await mount(); await selectFile(); await enterPassword();
    await act(async () => button("解密并预览").props.onClick());
    await act(async () => renderer.root.findByProps({ "aria-label": "确认仅隔离暂存" }).props.onChange({ currentTarget: { checked: true } }));
    await selectFile();
    expect(button("暂存恢复数据").props.disabled).toBe(true);
  });
  it("downloads only the encrypted export and clears the passphrase", async () => {
    await mount(); await enterPassword();
    await act(async () => button("导出加密备份").props.onClick());
    expect(api.downloadUserBackup).toHaveBeenCalledWith("encrypted", "backup.gpbackup");
    expect(password().props.value).toBe("");
  });
});




