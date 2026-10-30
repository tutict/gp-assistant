import { Activity, ArrowLeft, Ellipsis, LoaderCircle, Plus, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { LlmSettings } from "../../types";
import type { LlmSettingsSetter } from "../../lib/llmCredentialStore";
import { validateLlmProvider } from "../../lib/llmSettingsDraft";
import { useLlmSettingsDrafts } from "../../hooks/useLlmSettingsDrafts";
import { LlmProviderEditor } from "../settings/llm/LlmProviderEditor";
import { LlmProviderList } from "../settings/llm/LlmProviderList";

interface LlmSettingsPanelProps {
  settings: LlmSettings | null;
  onChange: LlmSettingsSetter;
  presentation?: "inline" | "dialog";
  statusMode?: "full" | "inline";
  openRequest?: number;
  returnFocusRef?: RefObject<HTMLElement | null>;
}
type Confirmation = "discard" | "delete" | "clear" | "reload";

export function LlmSettingsPanel({ settings, onChange, presentation = "inline", statusMode = "full", openRequest = 0, returnFocusRef }: LlmSettingsPanelProps) {
  const controller = useLlmSettingsDrafts(settings, onChange);
  const [open, setOpen] = useState(false);
  const [mobileEditing, setMobileEditing] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const seenOpenRequest = useRef(openRequest);
  const toggleButtonRef = useRef<HTMLButtonElement>(null);
  const moreButtonRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const confirmationRef = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const isDialog = presentation === "dialog";
  const closePanel = useCallback(() => {
    setOpen(false); setMoreOpen(false); setConfirmation(null);
    if (isDialog) window.requestAnimationFrame(() => (returnFocusRef?.current || toggleButtonRef.current)?.focus());
  }, [isDialog, returnFocusRef]);
  const requestClose = () => {
    if (controller.busy) return;
    if (controller.unsavedCount) setConfirmation("discard"); else closePanel();
  };
  const focusControl = (selector: string) => window.requestAnimationFrame(() => {
    const control = dialogRef.current?.querySelector<HTMLElement>(selector);
    if (control?.getClientRects().length) control.focus();
  });
  const restoreConfirmationFocus = () => {
    const target = previousFocus.current;
    window.requestAnimationFrame(() => {
      if (confirmationRef.current) return;
      if (target && dialogRef.current?.contains(target) && target.isConnected && !target.closest("[inert]")) target.focus();
      else dialogRef.current?.querySelector<HTMLElement>("button:enabled")?.focus();
    });
  };
  const cancelConfirmation = () => {
    setConfirmation(null);
    restoreConfirmationFocus();
  };
  const escape = () => {
    if (controller.busy) return;
    if (confirmation) cancelConfirmation();
    else if (moreOpen) { setMoreOpen(false); window.requestAnimationFrame(() => moreButtonRef.current?.focus()); }
    else requestClose();
  };
  const escapeRef = useRef(escape);
  escapeRef.current = escape;
  useEffect(() => {
    if (!openRequest || openRequest === seenOpenRequest.current) return;
    seenOpenRequest.current = openRequest; setOpen(true);
  }, [openRequest]);
  useEffect(() => {
    if (!open || !isDialog) return;
    const previousBodyOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    dialogRef.current?.querySelector<HTMLElement>("button:not([disabled]), input:not([disabled]), select:not([disabled])")?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); escapeRef.current(); return; }
      const scope = confirmationRef.current || dialogRef.current;
      if (event.key !== "Tab" || !scope) return;
      const focusable = Array.from(scope.querySelectorAll<HTMLElement>(
        'button:enabled, input:enabled, select:enabled, textarea:enabled, summary, [href], [tabindex]:not([tabindex="-1"])',
      )).filter(element => element.getClientRects().length > 0 && !element.closest("[inert], [hidden]"));
      const first = focusable[0], last = focusable[focusable.length - 1];
      if (!first || !last) { event.preventDefault(); scope.focus(); return; }
      const outside = !scope.contains(document.activeElement) || !focusable.includes(document.activeElement as HTMLElement);
      if (event.shiftKey && (document.activeElement === first || outside)) {
        event.preventDefault(); last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || outside)) {
        event.preventDefault(); first.focus();
      }
    };
    const captureOutsideFocus = (event: KeyboardEvent) => {
      const panel = dialogRef.current;
      if (!panel || !["Tab", "Escape"].includes(event.key) || panel.contains(document.activeElement)) return;
      const top = Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]'))
        .filter(element => element.getClientRects().length > 0 && !element.closest("[inert]")).pop();
      if (top && top !== panel && !panel.contains(top)) return;
      event.stopPropagation();
      handleKeyDown(event);
    };
    document.addEventListener("keydown", captureOutsideFocus, true);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", captureOutsideFocus, true);
      document.removeEventListener("keydown", handleKeyDown); document.body.style.overflow = previousBodyOverflow;
    };
  }, [open, isDialog]);
  useEffect(() => {
    if (!confirmation || typeof document === "undefined") return;
    previousFocus.current = document.activeElement as HTMLElement | null;
    confirmationRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [confirmation]);

  const draft = controller.editingDraft;
  const id = controller.editingId;
  const active = controller.active;
  const isActive = active?.id === id;
  const testing = controller.connection?.state === "testing";
  const complete = draft && !Object.keys(validateLlmProvider(draft.provider, true)).length;
  const confirmationTitle = confirmation === "discard" ? "还有 " + controller.unsavedCount + " 个连接未保存"
    : confirmation === "delete" ? "删除此连接？" : confirmation === "reload" ? "放弃此连接的修改？" : "清空所有连接？";
  const confirm = async () => {
    if (controller.busy) return;
    if (confirmation === "discard") { controller.reset(); closePanel(); }
    else if (confirmation === "reload") { controller.reload(id); setConfirmation(null); restoreConfirmationFocus(); }
    else { await controller.remove(confirmation === "delete" ? id : undefined); setConfirmation(null); restoreConfirmationFocus(); }
  };
  const body = open && <div ref={dialogRef} className={"llm-settings-body llm-switcher" + (isDialog ? " dialog" : "")}
    role={isDialog ? "dialog" : undefined} tabIndex={isDialog ? -1 : undefined} aria-modal={isDialog || undefined} aria-label={isDialog ? "模型连接配置" : undefined}
    onKeyDown={event => {
      if (event.key !== "Escape") return;
      event.preventDefault(); event.stopPropagation(); escapeRef.current();
    }}>
    <div className="llm-config-content" inert={Boolean(confirmation)}>
      <header className="llm-switcher-summary">
        <button type="button" className="llm-sheet-back-btn" onClick={requestClose} disabled={controller.busy}
          aria-label="关闭模型配置" autoFocus={isDialog}><X size={18} aria-hidden="true" /></button>
        <div><strong>模型 API 配置</strong><span>{active?.model ? "当前使用：" + (active.name || "自定义连接") + " · " + active.model : "尚未启用模型连接"}</span></div>
        <button type="button" className="llm-add-btn" disabled={controller.busy}
          onClick={() => { controller.add(); setMobileEditing(true); setMoreOpen(false); focusControl(".llm-back-to-list"); }}><Plus size={16} aria-hidden="true" /><span>新建连接</span></button>
      </header>
      <div className={"llm-switcher-grid " + (mobileEditing ? "show-editor" : "show-list")}>
        <LlmProviderList drafts={controller.drafts} editingId={id} activeId={active?.id} connections={controller.connections}
          disabled={controller.busy} onSelect={selected => { controller.select(selected); setMobileEditing(true); setMoreOpen(false); focusControl(".llm-back-to-list"); }}
          onAdd={preset => { controller.add(preset); setMobileEditing(true); setMoreOpen(false); focusControl(".llm-back-to-list"); }} />
        <div className="llm-editor-container">
          <button type="button" className="llm-back-to-list" disabled={controller.busy}
            onClick={() => { setMobileEditing(false); setMoreOpen(false); focusControl('.llm-provider-card[aria-pressed="true"]'); }}><ArrowLeft size={16} aria-hidden="true" />连接列表</button>
          {draft && <LlmProviderEditor key={id} draft={draft} errors={controller.errors} connection={controller.connection} catalog={controller.catalog}
            disabled={controller.busy} onPatch={values => controller.patch(id, values)} onCredential={value => controller.credential(id, value)}
            onFetch={() => controller.fetchModels(id)} />}
        </div>
      </div>
      <footer className={"llm-config-footer" + (mobileEditing ? " show-editor" : " show-list")}>
        <div className="llm-save-feedback">
          {controller.feedback.text ? <p role={controller.feedback.error ? "alert" : "status"} className={controller.feedback.error ? "error" : "success"}>{controller.feedback.text}</p>
            : <p>{controller.needsTest && !controller.tested ? "连接参数已修改，请测试通过后保存生效。"
              : controller.unsavedCount ? controller.unsavedCount + " 个连接有未保存修改。" : "测试通过后可启用连接。"}</p>}
        </div>
        <div className="llm-footer-actions">
          <div className="llm-more">
            <button ref={moreButtonRef} type="button" className="llm-secondary-btn" aria-expanded={moreOpen} aria-controls="llmConnectionActions"
              disabled={controller.busy} onClick={() => setMoreOpen(current => !current)}><Ellipsis size={17} aria-hidden="true" />更多</button>
            {moreOpen && <div className="llm-more-menu" id="llmConnectionActions" role="group" aria-label="连接管理">
              <button type="button" disabled={controller.busy || !controller.dirty} onClick={() => { setMoreOpen(false); setConfirmation("reload"); }}>放弃此连接修改</button>
              <button type="button" disabled={controller.busy || isActive}
                title={isActive ? "请先启用其他连接" : undefined} onClick={() => { setMoreOpen(false); setConfirmation("delete"); }}>删除此连接</button>
              <button type="button" disabled={controller.busy} onClick={() => { setMoreOpen(false); setConfirmation("clear"); }}>清空所有连接</button>
            </div>}
          </div>
          <div className="llm-submit-actions">
            <button type="button" className={"llm-test-btn" + (testing ? " loading" : "")} disabled={controller.busy || testing || !complete}
              onClick={() => controller.test(id)}>{testing ? <LoaderCircle size={16} aria-hidden="true" /> : <Activity size={16} aria-hidden="true" />}测试连接</button>
            <button type="button" className={isActive ? "llm-primary-btn" : "llm-secondary-btn"}
              disabled={!controller.canSave || testing} onClick={() => controller.save(id)}>{controller.busy ? "保存中…" : "保存"}</button>
            {!isActive && <button type="button" className="llm-primary-btn" disabled={!controller.canActivate || testing}
              onClick={() => controller.save(id, true)}>{controller.dirty ? "保存并启用" : "启用此连接"}</button>}
          </div>
        </div>
      </footer>
    </div>
    {confirmation && <div className="llm-confirm-layer"><div ref={confirmationRef} role="alertdialog" tabIndex={-1} aria-modal="true"
      aria-labelledby="llmConfirmTitle" aria-describedby="llmConfirmDescription" className="llm-confirm-panel">
      <strong id="llmConfirmTitle">{confirmationTitle}</strong>
      <p id="llmConfirmDescription">{confirmation === "discard" ? "放弃后会撤销全部未保存的草稿修改。"
        : confirmation === "reload" ? "将重新载入此连接最新的已保存配置。"
        : confirmation === "delete" ? (draft?.provider.name || "此连接") + " 的配置和密钥将被移除。"
        : "已保存的连接、密钥和全部草稿将被移除。"}</p>
      <div className="llm-confirm-actions">
        <button type="button" className="llm-secondary-btn" disabled={controller.busy} onClick={cancelConfirmation}>{confirmation === "discard" ? "继续编辑" : "取消"}</button>
        <button type="button" className="llm-primary-btn" disabled={controller.busy} onClick={confirm}>{controller.busy ? "正在处理…"
          : confirmation === "discard" ? "放弃修改并关闭" : confirmation === "reload" ? "放弃并重新载入" : confirmation === "delete" ? "确认删除" : "确认清空"}</button>
      </div>
    </div></div>}
  </div>;
  return <div className={"llm-settings-panel" + (open ? " open" : "") + (isDialog ? " dialog" : "")}>
    <div className="llm-settings-header"><button type="button" className="llm-settings-toggle" ref={toggleButtonRef}
      onClick={() => open ? requestClose() : setOpen(true)} aria-haspopup={isDialog ? "dialog" : undefined} aria-expanded={open}>
      {statusMode === "inline" ? <span>{active?.model ? "模型：" + (active.name || active.model) : "模型：未启用"}</span>
        : <><span>模型连接</span><strong>{active?.model ? active.name || "自定义连接" : "未启用"}</strong>
          <em>{active?.model || "配置模型 API"}</em><span className="llm-state-badge">{active?.model ? "当前启用" : "待配置"}</span></>}
    </button></div>
    {isDialog && open && typeof document !== "undefined" ? createPortal(<>
      <button type="button" className="llm-settings-modal-backdrop" onClick={requestClose} tabIndex={-1} aria-hidden="true" />{body}
    </>, document.body) : body}
  </div>;
}
