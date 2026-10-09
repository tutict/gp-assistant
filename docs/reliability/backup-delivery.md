# D5 encrypted user backup / recovery delivery

## Result and boundaries

Implemented in the six assigned new files only. No commits, subagents, Cargo edits,
command registration, settings integration, or storage-owner edits were made by
this task. The parent must integrate the module and panel.

**Integration gate:** restored watchlist native files are schema v2. Register
live recovery only together with the owner's v2/`restore_pristine` migration;
the worker's v2 schema was cross-checked against the recovery implementation
after it landed: column order, defaults, CHECKs and version agree. Tests cover
the four-column v2 contract and legacy-v1 conversion.

**This is not staging-only:** verified watchlist v1/v2 and agent-ledger v2 stores can
be recovered into missing paths. An explicitly pristine, already-initialized
watchlist can also be restored transactionally without replacing its active file. Agent-ledger v2 additionally supports a
transactional, current-wins, tombstone-aware merge. Durable per-backup/per-store
receipts prevent repeated import after deletion, including interrupted attempts.

**Full all-store recovery is NOT delivered.** Research, sentiment, future client
state, and unrecognized schemas are exported as sanitized recovery datasets and
can be explicitly staged, but are not installed or merged. They require additional
owner-reviewed schema/recovery contracts. There is no existing-watchlist merge:
its operation ledger is not a complete historical per-entity deletion journal.

## Supported stores / exact paths

All paths are relative to Tauri `app_data_dir()`. Files are explicitly allowlisted;
there is no recursive app-data collection. Missing files are reported, not created
by export. A corrupt or oversized present source fails the export rather than
silently producing an incomplete backup.

| Store | Path | Encrypted representation | Live recovery |
|---|---|---|---|
| Watchlist | `watchlist/watchlist.sqlite` | Recognized v1/v2: sanitized native v2 schema with constraints/index. Unknown: data-only SQLite. | Install native v2 when DB and sidecars are absent; or restore in-place when the owner explicitly marks an initialized DB pristine and it has no rows. Ordinary empty stores stay rejected. |
| Agent ledger | `agent/agent-runs.sqlite` | Recognized v2: sanitized native schema, both indexes and deletion table. Unknown: data-only SQLite. | Install into missing path; or separately confirmed current-wins merge into recognized existing v2. |
| Research | `research/research.sqlite` | Sanitized ordinary-table SQLite dataset, not a native replacement. | Preview/stage only. No automatic citation, vector/FTS, generation, or schema recovery. |
| Sentiment | `research/sentiment.sqlite` | Sanitized ordinary-table SQLite dataset. | Preview/stage only; generation coupling with research is not reconciled. |
| Future client state | `client-state.sqlite` | Sanitized ordinary-table dataset if file exists. | Preview/stage only; no guessed schema or deletion history. |

The agent path was verified in `agent_ledger::with_app_store`, and the sentiment
path in `sentiment::with_app`. This feature is separate from research sync packs.
No settings files, credential files, keychain contents, plaintext exports, WAL
files, or SHM files are bundled. Other app-data directories are not traversed.

## Cargo dependencies (Cargo owner)

```toml
aes-gcm = "0.10.3"
argon2 = "0.5.3"
zeroize = "1.8"
```

Existing `aes-gcm = "0.10"` / `argon2 = "0.5"` entries satisfy the requirement.
**`zeroize` is a direct dependency, not merely a transitive dependency.**
Already-existing dependencies reused: `base64 = "0.23"`, `rusqlite = "0.40"`
with `bundled`, `serde` with `derive`, `serde_json`, `sha2 = "0.11"`, Tauri 2.
No extra rusqlite feature, frontend package, plugin, keychain permission, or broad
filesystem permission is needed. OS randomness is re-exported by aes-gcm's default
features; this module does not require a separate rand_core declaration.

## Parent integration

1. Declare `mod user_backup;` in the desktop Rust crate.
2. Register these five commands in the existing Tauri handler:
   - `user_backup::api_user_backup_export`
   - `user_backup::api_user_backup_preview`
   - `user_backup::api_user_backup_stage`
   - `user_backup::api_user_backup_restore_empty`
   - `user_backup::api_user_backup_merge_agent`
3. Mount `<BackupPanel />` from
   `desktop/frontend/src/components/settings/BackupPanel.tsx` in settings. It uses
   existing control classes and does not edit App or existing settings itself.
4. After live recovery, refresh/reload the affected views before editing. The
   panel instructs this but does not reach into owner-managed caches or stores.
5. Parent must add `watchlist_state.restore_pristine` in schema v2 as an integer
   boolean column: false for every existing/migrated DB; true only for a newly
   created DB; unchanged by empty startup migration; false after any user mutation
   or nonempty legacy import. Never infer this marker from row count. This makes
   recovery reachable after startup auto-creates the watchlist DB.
6. Do not log invoke payloads or return values through parent IPC tracing: export
   requests contain a passphrase, and export results contain the encrypted blob.
7. Re-run the full parent build and native end-to-end commands after registration.
   Isolated compilation does not prove those parent integration steps happened.

### Explicit command modes and payloads

All invoke arguments have the shape `{ payload: ... }`.

| Command | Payload | Effect |
|---|---|---|
| `api_user_backup_export` | `{ passphrase }` | Consistent SQLite snapshots, filtering, authenticated encryption; returns `blob_base64`, safe `filename`, store summaries, missing IDs. |
| `api_user_backup_preview` | `{ blob_base64, passphrase }` | Authenticate, validate, filter, identify current stores/conflicts/eligibility/receipts; transient scratch removed. No persistent recovery directory or live import. |
| `api_user_backup_stage` | Same import payload | Reauthenticate and validate all stores; publish isolated datasets plus `preview.json`. No live data mutation. |
| `api_user_backup_restore_empty` | Same import payload | Reauthenticate/restage; attempt supported absent-store installs or explicitly pristine watchlist transactions, guarded by once-only receipts. |
| `api_user_backup_merge_agent` | Same import payload | Reauthenticate/restage; only merge an eligible agent v2 store. All other stores are unchanged. |

There are no caller-provided filesystem paths, SQL, KDF parameters, or module
identifiers. Imported store IDs must be unique members of the fixed allowlist.
Request structs reject unknown fields. Keys and passphrases are never returned.

`BackupPreview` returns:

- `state`: `preview_only`, `partially_imported`, or `imported`.
- `imported`: whether a supported live recovery operation succeeded. In merge mode
  this means the transaction committed, not that every row was newly inserted.
- `restored_stores`: exactly the store IDs installed/merged by this call.
- `staged`, `recovery_id`, `created_at_epoch_ms`, `missing_stores`,
  `restoration_note`.
- Each store: fixed path, byte/table/row/filter/omission counts,
  `current` (`absent`, `present_valid`, `present_unreadable`), conservative
  store-level `conflict`, `restore_to_empty` (also true for explicit pristine
  watchlist recovery), `merge_available`, `restore_status`,
  and optional `receipt_id`.

`imported` means **all included stores** were applied only when
`state == "imported"`; it does not mean all five possible stores were present in
this backup. `partially_imported` explicitly leaves other stores unapplied.
`preview_only` with `staged: true` means no live recovery occurred. Preview does
not claim a per-row diff or a cross-store transaction.

## Format and bounds

Binary `.gpbackup` container, encoded as standard base64 only for IPC:

- bytes 0..8: `GPUBAK\0\0` (not a research sync pack magic).
- bytes 8..10: little-endian format version, currently 1.
- bytes 10..26: random 16-byte salt.
- bytes 26..38: random 12-byte AES-GCM nonce.
- bytes 38..42: little-endian ciphertext length including the 16-byte tag.
- remaining bytes: AES-256-GCM ciphertext. The entire 42-byte header is AAD.
- Plaintext: bounded JSON with manifest version/time and allowlisted IDs, each
  containing base64 SQLite bytes and filtering counts; unknown fields rejected.

Argon2id v0x13, 19 MiB memory, two iterations, one lane, 32-byte key. The v1 KDF
cost is fixed: imported data cannot request arbitrary resource usage. Salt/nonce
come from OS randomness. Password input is 12..1024 UTF-8 bytes, not normalized;
choose a long high-entropy passphrase. There is no forgotten-password bypass.

Limits: 64 MiB total encrypted file, 16 MiB per database (including WAL-aware page
count checks), five stores, 128 table-list entries, 128 columns per table,
256-byte table/column names, 250,000 copied rows per store, 2 MiB per cell. JSON
redaction recursion is capped. No archive decompressor or embedded file paths.
Oversize base64 is rejected before decoding/KDF; magic/version/length are checked
before KDF. Authentication succeeds **before any filesystem write** on import.
Commands run blocking work off the UI thread with a process-wide backup mutex.

## SQLite consistency and filtering

Uses the parent's current `crate::durability::snapshot_sqlite(&Connection,&Path)`,
`validate_sqlite`, and `atomic_write`. Snapshots include committed WAL pages; no
raw copying of live main files or sidecars. The helper's Windows writable-fsync
fix is included in verification. Snapshots are consistent **per SQLite store**,
not globally synchronized across all stores.

Filtering reconstructs tables from validated SQLite values instead of executing
arbitrary source CREATE SQL. This discards deleted/freelist page contents, source
triggers/views, virtual/FTS/shadow tables, and secret/settings-named tables.
Credential columns and nested JSON keys are redacted, including serialized JSON
strings and `{key/name/field: credentialName, value: ...}` records. Obvious inline
credential markers/URL userinfo are conservatively redacted. Opaque BLOB values
(including derived embeddings) are replaced with NULL. Malformed JSON-like text
is redacted rather than passed through unchecked. Secret-named key/value rows are
omitted. Counts are included in the preview.

Recognized watchlist v1/v2 and agent v2 datasets are exported with pinned, explicit
native schema definitions rather than generic rowsets. Unsupported or filtered
watchlist structures remain data-only; they cannot be silently installed. During
import, filtering and native reconstruction run again; authentication is not
permission to execute source schema SQL.

Recovery normalization: watchlist v1/v2 retains its operation ledger, marks legacy
migration complete, and advances its revision beyond the exported value (using
current epoch milliseconds as a floor). Agent runs marked `running` become
`unknown` because no worker is actually running; backup tombstones remove matching
backup runs before recovery. Unknown future tables/columns/versions are not
silently ignored for native installation/merge.

Privacy limits: structured credential filtering cannot identify every arbitrary
unlabelled secret pasted into prose or encoded in an unexpected format. User
questions/research can themselves contain sensitive text. Rust key/plaintext
buffers and request passphrases use zeroization where practical; JS strings, IPC
copies, SQLite internals, and crypto implementation working memory are not claimed
to be forensically erased. The panel never stores passphrases in browser storage,
logs them, or reflects raw backend errors; password inputs clear for success and
failure.

## Recovery safety / tombstones / receipts

### Empty-store installation

Only a recognized schema and a completely absent main DB plus WAL/SHM/journal are
eligible. Existing valid, ordinary-empty, corrupt, linked, or concurrently-created files
are never replaced; the explicit pristine-v2 transaction described below is a
separate in-place path. Native schema/constraints are built in isolated scratch, validated
and synced, then published with an atomic **no-replace hard link**. There is no
copy-overwrite or delete/rename fallback. A competing owner creating the main file
wins. Filesystems without suitable hard-link support refuse the install safely.

An ordinary empty current watchlist is authoritative deletion state, not an invitation to
refill it. V1 has no complete independent tombstone table, so merging is refused.
Unknown future tombstone tables cause native reconstruction to reject that schema.

### Explicit pristine-initialized watchlist recovery

Startup creates the watchlist DB before settings is reachable. Absence alone is
therefore insufficient. Legacy v1 backups require the original three state
columns; recognized v2 requires the fourth `restore_pristine` column. Recovered
native schemas are always v2, with that column initialized false. In-place
recovery requires a current v2 database, not an inferred v1 state.

For an existing v2 DB, recovery requires the owner's explicit `restore_pristine=1`
marker AND zero watchlist rows; missing/false markers remain authoritative even
when empty. The check is repeated under `BEGIN IMMEDIATE` before insertion. The
transaction preserves existing startup operation receipts on operation-ID
conflicts, inserts the validated backup rows/other operations, advances revision,
sets migration complete, and clears `restore_pristine` before commit. The original
open SQLite connection observes the restored rows; the main file and WAL are not
replaced. A user clear/mutation between preview and transaction clears the marker
and blocks recovery. A stale true marker with any current row also blocks it.

The same once-only receipt is claimed before this transaction. This is NOT a
merge into an authoritative empty store and is not a heuristic based on receipt
contents. Parent must maintain the marker's lifecycle as specified above.

### Agent merge

Requires the exact recognized v2 tables/columns, including
`agent_deleted_conversations`. Uses a separate read/write connection without
CREATE and one `BEGIN IMMEDIATE` transaction with full synchronous durability.

- Existing `run_id` rows are never updated/replaced.
- Current conversation tombstones exclude all corresponding incoming runs.
- Incoming tombstones exclude corresponding backup runs before import.
- An incoming tombstone is added only if the current conversation has no live
  runs and no existing tombstone. Current live conversations win such conflicts.
- New, non-conflicting runs are inserted; existing current rows are never deleted.
- A concurrent owner deletion serializes with this transaction: deletion-before
  excludes the import; deletion-after removes/tombstones the merged rows.
- No research, sentiment, watchlist, or client-state merge is attempted.

### Once-only receipts

`user-backup-recovery/receipt-<SHA256 of encrypted base64>-<store>.json` binds an
attempt to the exact authenticated backup and store, across both restore modes.
A `create_new` pending receipt is fsynced **before mutation**. Existing receipts
(including pending/corrupt ones) block repeated attempts. A process crash cannot
turn an uncertain previous attempt into permission to import again. The final
receipt is atomically written after a successful install/merge. If final receipt
writing fails, the pending receipt remains and the response reports that state.

Deleting later-restored data does not allow the same backup to resurrect it.
Receipts are local to this installation, not a global identity or cross-device
protocol; independently re-encrypted backups have different identities. Do not
remove receipts as a retry mechanism. A failed/pending attempt may require manual
inspection and owner-mediated recovery; there is deliberately no automatic unlock.

Datasets are published in `user-backup-recovery/recovery-<random>/` with
`preview.json`, `<store-id>.sqlite`, and after a live operation `result.json`.
Staged copies are **decrypted private data**, not password-protected at rest.
Normal scratch paths are cleaned; a process crash can leave `.user-backup-*.pending`
with plaintext snapshots, potentially before filtering. Windows relies on the
user app-data ACL; Unix scratch directories are mode 0700. No startup janitor or
retention/pruning operation is added. Retain receipts independently of staging
cleanup. Parent must not auto-load generic staged datasets as active databases.

## Verification performed

### Rust: 17 tests, real Tauri adapters compiled; strict Clippy clean

Repository Cargo/module registration was intentionally not edited. An isolated
verification crate lives at:

`C:/Users/tutic/AppData/Local/Temp/gp-backup-d5-harness`

It compiles an exact copy of `user_backup.rs`, including all Tauri commands,
against real Tauri 2.11.6 and the dependencies above. `durability.rs` is copied
exactly up to its `#[cfg(test)]` block so unrelated parent tests requiring other
modules are not misrepresented as part of this harness. Its helper implementations
are not mocked. No repository Cargo files are changed by harness creation.

```powershell
cargo test --offline --manifest-path "$env:TEMP/gp-backup-d5-harness/Cargo.toml" --lib
cargo clippy --offline --manifest-path "$env:TEMP/gp-backup-d5-harness/Cargo.toml" --lib -- -D warnings
```

Passing coverage: WAL inclusion; main+WAL byte preservation; secret fields/JSON/
key-value records/settings exclusion; wrong password; salt/nonce/ciphertext/tag
tampering; oversize and unknown format version; authenticated invalid SQLite;
unknown/duplicate store IDs; all five exact paths; native schema constraints;
missing-store recovery; deletion/operation preservation; existing empty/corrupt
and sidecar/racing-file refusal; future schema/tombstone refusal; current-wins
agent merge with both deletion sets; repeated import after clearing; durable
pending-receipt refusal; explicit partial recovery; initialized pristine recovery
through an already-open handle; and marker/row rechecks after concurrent mutation.

Initial tests failed before corresponding feature implementation. Final Rust
verification: 17 passed, 0 failed; Clippy with `-D warnings` exited 0.

### Frontend: 11 tests + TypeScript

From `desktop/frontend`:

```powershell
npm.cmd run test:unit -- src/lib/backup.test.ts src/components/settings/BackupPanel.test.tsx
.\node_modules\.bin\tsc.cmd --noEmit
```

11 passed, 0 failed; TypeScript exited 0. Covers dedicated command payloads, input
bounds before file reading/invoke, error-message privacy, file encoding, password
clearing, preview/confirmation gates, stale-preview invalidation, encrypted
download, missing-store recovery and separately confirmed agent merge with
partial-success wording. Windows sandbox blocked Vitest subprocess creation; the
same focused commands passed with approved child-process access.

### Rendered UI QA

Browser plugin/skill unavailable; used the already-installed Playwright Chromium
headless runtime with an isolated temporary Vite fixture and mocked Tauri IPC.
This is **panel UI proof, not native end-to-end recovery proof**.

- URL during run: `http://127.0.0.1:1596` (temporary server stopped afterward).
- Viewports: desktop 1000x1100, mobile 390x844.
- File selection -> password -> pristine-initialized preview -> explicit restore confirmation ->
  imported acknowledgement passed; password cleared at both operations.
- Correct page/heading rendered; no blank page/framework error overlay; no page
  errors; no horizontal overflow at mobile size. Screenshots visually inspected.
- Evidence: `C:/Users/tutic/AppData/Local/Temp/gp-backup-d5-ui/desktop-preview.png`,
  `mobile-preview.png`, `mobile-restored.png`; script `verify.mjs` in that directory.

## Remaining work / exact limitations

1. Parent Cargo `zeroize`, module registration, command registration and settings
   placement; full app build/native invocation and actual WebView file-download QA.
2. Research/sentiment/client-state native recovery remains unmet. Research FTS,
   embeddings, citation/metadata consistency and sentiment-generation coupling
   require explicit rebuild/reconciliation; generic staged files cannot replace
   their active schemas. Future client-state schema is not guessed.
3. No merge into an authoritative existing watchlist, even an empty one; only an
   explicit pristine initialization marker permits in-place bootstrap recovery. No all-store atomic
   import and no cross-store snapshot transaction. No per-row conflict preview.
4. This is SQLite-backed user-state backup only: browser-only data not yet migrated
   to a SQLite store, attachments/source files, runtime caches and settings are not
   recovered. Parent must surface this boundary where necessary.
5. Hard-link/ACL/crash behavior was exercised on Windows temporary files, not every
   filesystem or packaging target; Unix/macOS compilation was not run here.
6. Pending receipts intentionally prioritize non-resurrection over automatic retry.
   No receipt unlocking, recovery-directory deletion UI, crash janitor, retention,
   scheduled backup, remote storage, or forgotten-password recovery is included.
7. Large stores fail closed at 16 MiB / total 64 MiB. Larger real research datasets
   need a separately reviewed streaming/size policy, not silent truncation.
8. Unlabelled sensitive prose and crash-left plaintext scratch remain privacy
   limitations. Encryption protects the exported file, not a compromised running
   process, OS account, already-unlocked dataset, or staged recovery directory.





