# D4 — OS-backed LLM credentials delivery

Date: 2026-10-06. Implemented directly in the shared checkout; no commits or subagents.

## Scope and security behavior

- `desktop/src-tauri/src/credentials.rs`: native vault abstraction, fake-backend tests, Windows backend, Android bridge, and exactly three public commands: `api_credential_put`, `api_credential_status`, `api_credential_delete`. Successful commands return `{ credential_ref, has_key }`, never a secret. There is deliberately no JS read/export command.
- Windows uses `CredWriteW` / `CredReadW` / `CredDeleteW`, generic credentials, `CRED_PERSIST_LOCAL_MACHINE` (the signed-in user's credential store, not a globally readable file). Namespace: `gp-assistant.llm.`. No plaintext fallback, environment-variable fallback, or arbitrary target lookup. The persistent limit is 2560 UTF-8 bytes, matching Windows' credential-blob limit; session requests retain the prior 8192-byte limit.
- Android uses the platform `AndroidKeyStore` AES-256 key, `AES/GCM/NoPadding`, fresh randomized 96-bit IVs and 128-bit authentication tags. The credential reference is authenticated as AAD. The non-exportable key alias is `com.gpassistant.llm.aes.v1`. Encrypted versioned files live in `Context.noBackupFilesDir/llm-credentials`, written with `AtomicFile`, read-back verified, with directory fsync. Corrupt ciphertext, missing keys and Keystore failures fail closed; reads never regenerate keys.
- `desktop/src-tauri/android/credentials/` is the canonical, tracked Kotlin/XML source. The build script copies it into generated Android sources/resources on every normal build and `-InitOnly`, adds narrow reflection keep rules, and reapplies `allowBackup=false`, legacy full-backup exclusions, and Android 12+ cloud/device-transfer exclusions. No new Android permissions are requested. Automatic OS backup is disabled app-wide, including legacy WebView data; D5's explicit encrypted user backup is separate.
- Important Tauri 2 detail: an unhandled Rust plugin call can fall through to a Kotlin command. This plugin's Rust `invoke_handler` explicitly rejects **all** JavaScript plugin calls and returns `true`. Only native `run_mobile_plugin` can invoke Kotlin `read`. Do not remove this guard or add an allow-read permission.
- Native `llm.rs` catalog/test and `rig_runtime.rs` provider construction resolve `credential_ref` without mutating or returning the input payload. A reference lookup failure never falls back to a supplied inline key. OpenAI Chat, OpenAI Responses, Anthropic Messages, full/base endpoints and header formats remain unchanged. Stored-provider runtime errors omit upstream details; key redaction occurs before truncation; `ProviderConfig` Debug excludes secret contents.

## Frontend contracts and migration

`contracts.ts` augments the existing LLM interfaces with `credential_ref` and `has_key` (without editing the shared types file). `llmProviderAuth` is used for catalog/test requests and `buildLlmConfig`. Remembered requests carry only a reference. Nonremembered keys are held in a module-memory session map, not settings or localStorage; request construction can use those session keys. The panel keeps unsaved input in component memory and commits it only on Save. It never fills/reveals a stored OS key and offers explicit Clear key, provider deletion and Clear all.

`createLlmCredentialStore` and `useLlmCredentials` replace the old synchronous sanitizer-on-read flow. Operations are serialized, including initialization and React StrictMode replay.

Migration follows the user's corrected requirement:

1. Read and retain the existing flat or provider-list legacy settings in memory without rewriting storage.
2. Capture keys into session memory so the configured model remains usable.
3. For remembered keys, write a new native slot; native put performs a full read-back comparison. Then independently check returned reference and native status.
4. **Only after all required secure writes verify**, persist the secret-free reference settings. Only then remove old working native references during replacement/clear. A localStorage quota failure does not delete the old working credential.
5. On migration failure, leave the preexisting localStorage bytes **byte-for-byte unchanged**. Expose a persistent migration warning and keep a usable session copy. No settings edit may overwrite the pending record. Explicit retry repeats migration; only verified success replaces the old plaintext.
6. New-key remember failures (not legacy migration) keep the key in session memory only, persist no plaintext, and show an error. Missing saved references on restart are retained with an explicit unavailable-key warning instead of silently stripping configuration.

**Transitional risk:** failed legacy migration intentionally leaves previously stored plaintext in place to avoid losing working configuration after restart. The warning says not to back up/share app data until migration succeeds. This is not a newly introduced plaintext fallback. Overwriting a localStorage entry is not a forensic secure erase of historical WebView DB pages, old backups, memory, swap, or crash dumps.

Successful persistence contains only provider metadata/reference/status. The synchronous sanitizer refuses remembered plaintext instead of silently dropping it: never attach it to an initializer that catches the exception and substitutes empty settings.

## Exact parent integration

### `desktop/src-tauri/src/lib.rs`

Parent-owned (already observed in the shared checkout):

```rust
mod credentials; // exactly once; no #[path] copy under llm

let builder = tauri::Builder::default()
    .plugin(credentials::init())
    .invoke_handler(tauri::generate_handler![
        // existing commands ...
        credentials::api_credential_put,
        credentials::api_credential_status,
        credentials::api_credential_delete,
    ]);
```

Keep the module top-level because both native request paths use `crate::credentials`. No edits to `tauri.ts` are required: the credential adapter invokes these native commands directly and never uses an HTTP fallback.

### `desktop/frontend/src/App.tsx`

Delete the old LLM `useLocalStorage` declaration, mirrored `useState`, old `setLlmSettings` callback, sanitizer import, and obsolete updater alias (if unused). Replace them with:

```tsx
import { useLlmCredentials } from "./hooks/useLlmCredentials";

const {
  llmSettings, setLlmSettings, credentialError, credentialsReady,
  credentialMigrationPending, retryCredentialMigration,
} = useLlmCredentials();
```

Keep existing settings consumers and pass the setter through to the panel. It returns `Promise<void>` and rejects on a failed save/delete after updating the hook's persistent error. The panel awaits/catches it. Other fire-and-forget callers must catch rejection (the hook's error remains visible).

Always display `credentialError` with an accessible alert/PanelFeedback. While `credentialMigrationPending`, offer a button calling `retryCredentialMigration()`; do not hide the warning merely because the panel closes. Parent's current `PanelFeedback` integration and retry button were observed during delivery.

**Remaining integration check:** gate settings editing/model submissions until `credentialsReady` (without conditionally skipping React hooks). The store serializes early writes, but a request built from initial null settings before initialization completes could otherwise run without the user's configured model. Keep deterministic/local-only work available as appropriate.

**Other out-of-scope guard:** `GepaLabPanel.tsx`'s model-ready test must use `(llm.api_key || llm.credential_ref || llm.base_url)` so a stored reference with the default endpoint is eligible. This was reported to parent; do not change provider protocols to work around this guard.

### Shared Cargo dependencies

This task owns the dependency edits and lockfile:

- Windows-only `windows-sys = 0.61.2`, features `Win32_Foundation`, `Win32_Security_Credentials`.
- At parent's request for D5: `aes-gcm = 0.10` (locked 0.10.3), `argon2 = 0.5` (0.5.3), `rand_core = 0.6` (0.6.4) with `getrandom`.
- At parent's request for lifecycle integration: `tauri-plugin-single-instance = "2"` under `cfg(not(target_os = "android"))`, locked 2.4.5. Parent owns its lifecycle registration/order. Register single-instance protection before startup reconciliation; do not allow a second process to mutate live agent rows.
- Android crypto is the OS Java API, not Rust AES. AndroidX runner 1.5.0/JUnit extension 1.1.4 are instrumentation-only Gradle dependencies applied reproducibly by the script.

## Verification and reproduction

All command paths below are relative to repository root unless a working directory is specified. Tests use synthetic secrets and fake native backends; no actual Windows Credential Manager entry or user key was read, written, or deleted.

### Frontend

Working directory `desktop/frontend`:

```powershell
npm.cmd run test:unit -- src/lib/contracts.test.ts src/lib/llmCredentialStore.test.ts src/hooks/useLlmCredentials.test.tsx src/components/panels/LlmSettingsPanel.test.tsx
npm.cmd exec -- tsc -b --pretty false
npm.cmd run build:app
```

Final run: **51 targeted frontend tests passed**; `npm.cmd run build:app` passed (TypeScript and Vite production build).

Coverage: successful flat/provider-list migration; write/status failure; exact legacy-byte retention and retry; no overwriting pending legacy data; new-key failure; nonremembered sessions; stored-key request formats; native clear/delete failure; serialized save/clear; StrictMode; stored-key UI; replacement quota failure. Initial assertions were observed failing before implementation/fixes.

### Native host

```powershell
cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib credential -- --test-threads=1
cargo check --manifest-path desktop/src-tauri/Cargo.toml --lib --locked
```

The credential-filtered host run passed 11 tests (six vault tests, two added redaction/debug tests, three existing matching tests). Existing unrelated dead-code/linker warnings remain; no claim of a warning-free repository.

### Android generation and Kotlin compile (no device)

```powershell
./scripts/build-android.ps1 -CredentialGenerationTest
# Normal reproducible generation, when preparing the Android build:
./scripts/build-android.ps1 -InitOnly
```

The generation test uses a scratch directory, verifies idempotence, namespace-safe manifest edits, exclusion XML and exact source copy, and asserts no extra permissions. It does not call a device or load secrets.

Working directory `desktop/src-tauri/gen/android`, SDK/JDK environment configured as in the build script:

```powershell
./gradlew.bat :app:compileArm64DebugKotlin :app:compileArm64DebugAndroidTestKotlin :app:processArm64DebugMainManifest -x rustBuildArm64Debug --console=plain
```

Passed: plugin Kotlin compile, instrumentation test-source compile and merged manifest/resource processing. This is **not** a packaged APK/device test. The Rust build task is intentionally excluded here because Rust is checked separately.

### Rust Android cross-check

```powershell
$env:ANDROID_HOME = 'C:/tmp/android-sdk'
$env:NDK_HOME = 'C:/tmp/android-sdk/ndk/29.0.14206865'
$env:CC_aarch64_linux_android = "$env:NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/aarch64-linux-android24-clang.cmd"
$env:AR_aarch64_linux_android = "$env:NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-ar.exe"
cargo check --manifest-path desktop/src-tauri/Cargo.toml --target aarch64-linux-android --lib --locked
```

Passed. In the restricted shell, Tauri needed approval to write its registry-side generated Android plugin cache; it was rerun with approval, not bypassed.

## Real-device/release acceptance still required

No installation, device invocation, process kill, actual Credential Manager operation, or user-secret read was performed here. On an isolated test profile/device with synthetic credentials:

1. Verify Rust-to-Kotlin put/status/resolve/delete through the installed app; try JS `plugin:llm-credentials|read` and confirm rejection. Verify the public commands never return the key.
2. Run `CredentialVaultTest` via AndroidJUnitRunner. It uses randomized test alias/directory, covers ciphertext round-trip, tampering and missing-key failure, and never targets the production alias.
3. Verify restart/force-stop while writing; Keystore unavailable/invalidated, app-data clear/reinstall and missing/corrupt ciphertext must produce explicit errors, never plaintext fallback.
4. Inspect signed release/shrunk plugin registration and merged backup rules; test Android 12+ cloud backup and OEM device transfer. `noBackupFilesDir` is the primary exclusion; verify behavior on supported OEMs rather than promising every vendor obeys policy.
5. Test Windows Credential Manager write/readback/delete/restart under a disposable OS account, including a denied/locked credential-store scenario.
6. Test all three provider formats with a synthetic local test server; confirm only native requests use the stored key. Verify migration retry UX and readiness gating after parent integration.

Boundaries: Keystore hardware backing is device-dependent; this does not protect against a rooted device, arbitrary code running as the same Windows user, a compromised application process, or an intentionally configured exfiltrating provider endpoint. There is no transaction spanning OS credentials and WebView metadata: writes verify before publishing references, and old working keys are not deleted on metadata-write failure, but a crash/disconnect between stores can leave an unreferenced OS-protected slot. OS slot garbage collection is not implemented; there is no enumeration/export IPC. Normal clear/delete operations verify removal of the configured slot.

## Final verification note

The final host credential-filtered test rerun passed 11/11 and the generation test/diff whitespace checks passed. Android Kotlin/instrumentation sources and the Rust Android cross-check had passed earlier in this delivery. The optional final Rust Android repetition was not executed: automatic approval review timed out twice. Do not treat that repetition as a new success or as a product failure. No real-device acceptance has been claimed.
