# O1 operations delivery (integration contract, work in progress)

Implementation is scoped to diagnostics, GEPA gates, settings panels and scripts. No Cargo, lib.rs, runtime, App.tsx or packaging edits by this worker. Parent owns those hooks.

## Exact parent hooks

1. Parent has declared `mod diagnostics;` in `desktop/src-tauri/src/lib.rs`.
2. After the single-instance check and after resolving app_data_dir, but BEFORE optional GEPA/headless/background startup, call `diagnostics::init(&app_data_path)` once. It creates the fixed `diagnostics/` child itself. Handle a fixed-code error visibly; GEPA fails closed. Do not skip existing database/schema/integrity checks in safe start or on diagnostics error. Repeated init in the same process is idempotent.
3. The source now annotates these functions with `#[tauri::command]`; the parent has directly registered `diagnostics::api_diagnostics_status`, `diagnostics::api_diagnostics_preview`, and `diagnostics::api_diagnostics_set_gepa`. Do not add duplicate wrappers. Signatures:
   ```rust
   api_diagnostics_status() -> Result<FeatureStatus, String>
   api_diagnostics_preview() -> Result<DiagnosticPreview, String>
   api_diagnostics_set_gepa(payload: GepaPreference) -> Result<FeatureStatus, String>
   ```
   The preference IPC payload is exactly `{ payload: { enabled: boolean } }`; unknown fields/types reject. **Do not register `record` as IPC.** No backend export path, upload or arbitrary-string logging API exists.
4. Only after all jobs are stopped and durable user stores/checkpoints have succeeded, call `diagnostics::mark_clean_shutdown()` from the final orderly exit path (`RunEvent::Exit`, not a cancellable exit request or every window close). Do NOT mark clean for panic, force-stop, early exit, failed flush or cancelled shutdown. Propagate a shutdown failure; do not claim a clean shutdown. OS termination/Android background callbacks are not evidence of a clean shutdown. Headless `process::exit` currently bypasses this hook and will be labelled unclean unless parent explicitly finalizes before that exit.
5. Parent instrumentation may call `let _ = diagnostics::record(diagnostics::OperationalEvent::StorageCheckFailed);` (or `StorageRecoveryCompleted`, `OfflineOperationCompleted`, `BackgroundRequest`) **once per real outcome/request** at verified sites. No caller data, elapsed time, error text, request IDs or payload is accepted. These counters are not measurements until those call sites exist, and must not be substituted for performance-baseline readings.
6. `SettingsSheet` already imports/mounts the actual named `ReliabilityPanel` and `BackupPanel`. Do not mount duplicates. Parent still owns GEPA availability refresh/invalidation after setting changes: the existing GepaLabPanel status effect only refreshes on its availability callback dependency. Remount/refresh the GEPA status consumer when settings change (or reopen the client); native start/apply gates always recheck current preference. This worker changed GepaLabPanel only for the expressly requested credential_ref eligibility fix.

## Native API details

`init(&Path)` expects app-data ROOT. Native state paths: `<app-data>/diagnostics/features.json` and `lifecycle.json`; same-directory atomic writes call parent's `durability::atomic_write`. Raw helper errors are replaced with fixed error codes. A malformed/oversized preferences or marker file is preserved and initialization fails closed, not silently reset. No tests touch actual app data.

The flag gate modifies GEPA status/start/apply only; cancellation/report access and parent's atomic report writes remain untouched. A currently running GEPA job is not auto-cancelled by toggling off; cancel explicitly before changing mode. Safe start uses `GP_ASSISTANT_SAFE_START=1` (or `true`) or exact `--safe-start`; preference persists independently. Feature compilation still controls availability.

## Verification log

Will be finalized after targeted reruns. There are no measured Windows/Android performance baseline files, no measured crash rate, and no device lifecycle/release-package sign-off from this work.

