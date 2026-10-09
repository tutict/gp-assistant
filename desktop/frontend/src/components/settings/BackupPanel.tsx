import { useId, useRef, useState } from "react";
import {
  downloadUserBackup,
  exportUserBackup,
  previewUserBackup,
  readBackupFile,
  stageUserBackup,
  restoreEmptyUserBackup,
  mergeAgentUserBackup,
  type BackupPreview,
} from "../../lib/backup";

/** Parent embeds this standalone panel inside settings; no route or global state changes. */
export function BackupPanel() {
  const headingId = useId();
  const helpId = useId();
  const [passphrase, setPassphrase] = useState("");
  const [blob, setBlob] = useState<string | null>(null);
  const [filename, setFilename] = useState("");
  const [preview, setPreview] = useState<BackupPreview | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [restoreConfirmed, setRestoreConfirmed] = useState(false);
  const [mergeConfirmed, setMergeConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);
  const passwordBytes = new TextEncoder().encode(passphrase).length;
  const hasPassword = passwordBytes >= 12 && passwordBytes <= 1024 && !!passphrase.trim();

  async function run(operation: () => Promise<void>) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setMessage(""); setFailed(false);
    // Clear the visible value immediately, including failure paths. JS/IPC copies cannot be zeroized.
    setPassphrase("");
    try { await operation(); }
    catch {
      setFailed(true);
      setMessage("备份操作失败。请检查口令、文件格式、大小及磁盘空间。当前数据未被恢复操作替换。");
    } finally { setPassphrase(""); setBusy(false); inFlight.current = false; }
  }

  async function selectFile(input: HTMLInputElement) {
    if (inFlight.current) return;
    const file = input.files?.[0];
    input.value = "";
    setBlob(null); setFilename(""); setPreview(null); setConfirmed(false); setRestoreConfirmed(false); setMergeConfirmed(false);
    if (!file) return;
    await run(async () => { setBlob(await readBackupFile(file)); setFilename(file.name); });
  }

  return (
    <section className="settings-item-copy" style={{ overflowWrap: "anywhere" }} aria-labelledby={headingId} aria-busy={busy}>
      <h3 id={headingId}>加密用户备份</h3>
      <p id={helpId}>
        手动备份自选股、代理记录、研究及情绪 SQLite 数据；独立于研究同步包。
        排除设置文件和凭据字段。请保管口令，遗失后无法解密。
      </p>
      <p>
        支持解密预览、隔离暂存，以及可恢复缺失的自选股 v1/v2 / 代理 v2 库，或带有“尚未使用”标记的全新自选股库。普通空库、已使用库和损坏库不回填；仅代理记录支持保留当前值的合并。
        暂存文件为经过过滤的数据表，不是可直接替换的数据库；当前数据与删除记录始终保留；研究、情绪和客户端状态目前只能暂存。
        暂存目录含解密后的私人数据，请勿共享。
      </p>
      <label>
        备份口令（至少 12 个 UTF-8 字节；每次操作后清空）
        <input
          type="password"
          aria-label="备份口令"
          aria-describedby={helpId}
          value={passphrase}
          disabled={busy}
          autoComplete="off"
          spellCheck={false}
          maxLength={1024}
          onChange={(event) => setPassphrase(event.currentTarget.value)}
        />
      </label>
      <button type="button" className="save-btn" aria-label="导出加密备份" disabled={busy || !hasPassword}
        onClick={() => run(async () => {
          const result = await exportUserBackup(passphrase);
          downloadUserBackup(result.blob_base64, result.filename);
          setMessage(`已请求下载加密备份（${result.stores.length} 个数据库）。请确认文件已保存。`);
        })}>
        导出加密备份
      </button>
      <label>
        选择加密备份（.gpbackup，最大 64 MiB）
        <input type="file" aria-label="选择加密备份" accept=".gpbackup" disabled={busy}
          onChange={(event) => selectFile(event.currentTarget)} />
      </label>
      {filename ? <p>已选择：{filename}</p> : null}
      <button type="button" className="clear-btn" aria-label="解密并预览" disabled={busy || !blob || !hasPassword}
        onClick={() => run(async () => {
          setPreview(null); setConfirmed(false); setRestoreConfirmed(false); setMergeConfirmed(false);
          const result = await previewUserBackup(blob!, passphrase);
          setPreview(result);
          setMessage("预览完成。未恢复任何数据；暂存前请再次输入口令并确认。");
        })}>
        解密并预览
      </button>
      {preview ? (
        <div>
          <h4>{preview.imported ? "恢复结果" : "恢复预览 · 仅数据集"}</h4>
          <ul>
            {preview.stores.map((store) => (
              <li key={store.id}>
                <strong>{store.relative_path}</strong> — {store.tables} 表 / {store.rows} 行 / {Math.ceil(store.bytes / 1024)} KiB。
                {store.restore_status === "eligible_pristine_watchlist_v2" ? "全新初始化库（尚未使用），可事务恢复；不替换文件。" :
                  store.current === "present_valid" ? "当前数据库存在，保留当前数据；不执行合并。" :
                  store.current === "present_unreadable" ? "当前数据库无法验证，保留原文件；不执行替换。" : "当前数据库缺失；只有标记为受支持的库可显式恢复。"}
                {` 过滤项：${store.filtered_fields}；省略表：${store.omitted_tables}。`}
                {store.restore_status === "merged_current_wins" ? " 本次已安全合并。" : store.restore_status.startsWith("imported") ? " 本次已恢复。" : store.restore_to_empty ? " 可恢复（已验证空白状态与结构）。" : store.merge_available ? " 可安全合并代理记录。" : " 不支持自动恢复此库。"}
                {store.receipt_id ? ` 恢复回执：${store.receipt_id}` : null}
              </li>
            ))}
          </ul>
          {preview.missing_stores.length ? <p>备份中未包含：{preview.missing_stores.join("、")}</p> : null}
          <p>预览不计算逐行冲突。代理合并保留当前记录，跳过已删除会话；不执行跨库事务。</p>
        </div>
      ) : null}
      <label>
        <input type="checkbox" aria-label="确认仅隔离暂存" checked={confirmed}
          disabled={busy || !preview || preview.staged}
          onChange={(event) => setConfirmed(event.currentTarget.checked)} />
        我理解：只将解密数据暂存到隔离目录，不替换当前数据库，也不恢复已删除记录。
      </label>
      <button type="button" className="save-btn" aria-label="暂存恢复数据"
        disabled={busy || !preview || preview.staged || !blob || !hasPassword || !confirmed}
        onClick={() => run(async () => {
          const result = await stageUserBackup(blob!, passphrase);
          setPreview(result); setConfirmed(false); setRestoreConfirmed(false); setMergeConfirmed(false);
          if (result.state !== "preview_only" || result.imported || !result.staged || !result.recovery_id) {
            throw new Error("Unexpected recovery state");
          }
          setMessage(`已隔离暂存，尚未恢复到应用。恢复目录：user-backup-recovery/${result.recovery_id}。需要存储模块提供安全合并。`);
        })}>
        暂存恢复数据
      </button>
      <label>
        <input type="checkbox" aria-label="确认恢复缺失库" checked={restoreConfirmed}
          disabled={busy || !preview?.stores.some((store) => store.restore_to_empty) || preview.imported}
          onChange={(event) => setRestoreConfirmed(event.currentTarget.checked)} />
        我确认：只恢复缺失或明确标记尚未使用的受支持库；普通空库不回填，保留用户删除状态。
      </label>
      <button type="button" className="save-btn" aria-label="恢复缺失或全新库"
        disabled={busy || !blob || !hasPassword || !restoreConfirmed || !preview?.stores.some((store) => store.restore_to_empty) || preview.imported}
        onClick={() => run(async () => {
          const result = await restoreEmptyUserBackup(blob!, passphrase);
          setPreview(result); setConfirmed(false); setRestoreConfirmed(false); setMergeConfirmed(false);
          if (result.imported && result.restored_stores.length) {
            setMessage(`已恢复：${result.restored_stores.join("、")}。仅列出的库已安装；其他库未恢复。请重新加载恢复的页面后再编辑。`);
          } else {
            setMessage("未恢复任何库：当前库或日志文件已存在，或结构不受支持。隔离数据已保留，未覆盖当前数据。");
          }
        })}>
        恢复缺失或全新库
      </button>
      <label>
        <input type="checkbox" aria-label="确认代理记录合并" checked={mergeConfirmed}
          disabled={busy || !preview?.stores.some((store) => store.merge_available)}
          onChange={(event) => setMergeConfirmed(event.currentTarget.checked)} />
        我确认：仅合并代理 v2 记录；当前值和删除标记优先，不覆盖当前会话。
      </label>
      <button type="button" className="save-btn" aria-label="安全合并代理记录"
        disabled={busy || !blob || !hasPassword || !mergeConfirmed || !preview?.stores.some((store) => store.merge_available)}
        onClick={() => run(async () => {
          const result = await mergeAgentUserBackup(blob!, passphrase);
          setPreview(result); setConfirmed(false); setRestoreConfirmed(false); setMergeConfirmed(false);
          setMessage(result.imported ? `已合并：${result.restored_stores.join("、")}。保留当前值和删除标记，其他库未恢复。请重新加载代理记录。` :
            "未合并任何记录。结构不支持、已有恢复回执或当前数据发生变化；请保留回执并检查，勿重复强制恢复。");
        })}>
        安全合并代理记录
      </button>
      <p>每份备份的每个库只允许一次恢复尝试；回执会阻止重复导入和删除后复活。中断或失败回执需人工检查。</p>
      {preview?.stores.some((store) => store.restore_status.includes("receipt") && store.restore_status !== "imported") ? (
        <p role="status">部分库存在恢复回执或待确认状态。请保留备份与回执，不要删除回执来强制重复导入。</p>
      ) : null}
      {preview?.stores.some((store) => store.restore_status === "imported_durability_unconfirmed") ? (
        <p role="alert">自选股库已安装，但磁盘持久化确认失败。请保留备份并检查磁盘，不要重复覆盖恢复。</p>
      ) : null}
      {busy ? <p role="status">正在处理加密备份，请稍候…</p> : null}
      {message ? <p role={failed ? "alert" : "status"}>{message}</p> : null}
    </section>
  );
}






