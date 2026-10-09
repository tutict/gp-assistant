# D3 research delivery / rollback safety

## Scope and integration

Implementation files (workspace `C:/Users/tutic/IdeaProjects/gp-assistant`):

- `desktop/src-tauri/src/research.rs`
- `desktop/src-tauri/src/research_recovery.rs`
- This delivery note: `docs/reliability/research-delivery.md`

`research.rs` registers `#[path = "research_recovery.rs"] mod recovery;` as a private child module. **Do not add a `mod research_recovery` declaration to `lib.rs`.** No new dependency or Cargo declaration is required by D3. No commits or subagents were used.

Required parent-owned APIs in `crate::durability`:

- `atomic_write(&Path, &[u8]) -> Result<(), String>`
- `configure_user_connection(&Connection) -> Result<(), String>`
- `checkpoint(&Connection) -> Result<(), String>` (must check busy/total/completed)
- `validate_sqlite(&Path) -> Result<(), String>` (read-only integrity check)
- `snapshot_sqlite(&Connection, &Path) -> Result<(), String>` (consistent VACUUM INTO, validate, sync, atomic publish)

Windows sync handles must be write-capable: `OpenOptions::new().read(true).write(true).open(...)`, not `File::open(...).sync_all()`. The expanded tests exposed this in the first shared snapshot helper implementation; parent subsequently corrected it. Research's own checkpoint finalization also uses a write-capable handle.

The existing `with_exclusive_app_database` / `with_shared_app_database` guards and in-flight-answer generation invalidation remain the runtime coordination mechanism. No new runtime lock is introduced. Native query commands and CPU/I/O async dispatch remain unchanged. Pending recovery is checked under the **same shared guard** as the subsequent operation; recovery releases that guard and acquires the existing exclusive guard, then retries. Full startup integrity validation is not run on every query.

## User state authority

Import builds a new document store from the existing v1 JSON / v2 SQLite reader. Rollback creates a **new staging snapshot of old documents**, never reactivates the old whole database.

Before current user state is copied, staging deletes, in foreign-key-safe order:

1. Unavailable citation snapshots.
2. Active answer citations.
3. Answers.
4. Threads.

It then inserts the complete current authoritative threads and answers, including answers with null thread IDs. This is replacement, not `INSERT OR IGNORE`: deleted threads do not return, new threads survive, and recent answers survive. Current unread flags overwrite matching message/document IDs. A historical document absent from the current message set defaults to unread, not the stale rollback read flag. Cited counts are recomputed. Current legacy-migration markers are carried forward. FTS is rebuilt before activation.

## Historical citations

Chunk ID alone is not identity. An active citation survives only if document/chunk IDs, full document and chunk content/hashes, title, source tier/name, URL, publication time and page identity agree with the current historical evidence.

If evidence is absent or changed, the original citation payload is persisted in additive `research_unavailable_citations`, with a foreign key only to its answer. It retains the original label, IDs, excerpt, source metadata and scores, and is returned with `unavailable: true`. It is never joined to a replacement chunk, and remains unavailable on later imports/rollbacks rather than silently rebinding by ID. Citation ordering is retained. Deleting a thread cascades these snapshots. Attempting to save an unavailable snapshot as a new active citation fails transactionally.

The native schema version remains 2; the snapshot table is an additive local-history extension. Exported v2 packages still exclude history and vectors. Older history that was already lost or rebound before this change cannot be reconstructed.

**Parent/UI follow-up:** add optional `unavailable?: boolean` to the frontend citation type and visibly label these as historical/unavailable evidence. The existing evidence inspector consumes the returned snapshot rather than looking up a replacement chunk, but its generic “original can be traced” wording should not claim active availability for these citations. UI files are outside D3's write scope.

## Durable journal and file layout

- `research.sqlite`: current authoritative database.
- `research-delivery.json`: versioned durable phase manifest.
- `research.sqlite.g<N>.previous`: sealed pre-attempt snapshot.
- `research.sqlite.g<N>.staged`: sealed candidate database.
- `research.sqlite.g<N>.uncommitted`: preserved uncommitted main when recovery reverts an installed candidate.
- Legacy `research.sqlite.rollback`: accepted only as an existing old document source; never directly reactivated.

Generation is a persisted monotonic attempt counter, not a timestamp. The next attempt exceeds both the manifest counter and all generation-named artifacts. Failed/aborted attempts consume their generation. The active DB separately stores `delivery_generation` in `research_metadata`, so recovery can distinguish old/current without expecting a committed main's bytes to remain unchanged after normal user writes.

Manifest fields include version, attempted/previous generation, phase, SHA-256 of old/staged snapshots, and the last committed rollback filename/hash. Rollback filenames are constrained to the legacy name or generated local filenames; arbitrary manifest paths are rejected.

### Commit sequence

1. Validate/recover existing authority. Only a genuinely new directory with no database artifacts may create an empty baseline.
2. Durably reserve a generation with phase `preparing`.
3. Checkpoint current main, checking busy and incomplete frame counts, close, sync; **only after success** remove live sidecars.
4. Take a SQLite-consistent immutable old snapshot using the parent's snapshot helper.
5. Build staging, replace user state, write its generation, checkpoint, close, sync and validate.
6. Persist `prepared`, including both snapshot hashes.
7. Revalidate both snapshots, checkpoint/quiesce main, atomically replace main with the candidate. There is no missing-main rename gap.
8. Persist `activated`, validate installed integrity/generation, then persist `committed` and advance the rollback pointer.

No error-path snapshot cleanup is performed. The manifest is retained after commit/abort as the generation high-water mark. Normal user writes after commit do not need to update the manifest.

### Recovery policy

| State | Action |
| --- | --- |
| Valid old-generation main, unfinished delivery | Keep old main, persist `aborted`; never promote staging |
| Valid installed new main but no durable `committed` phase | Validate/checksum old snapshot, preserve installed main separately, restore old and persist `aborted` |
| Valid committed-generation main | Keep it, including all newer threads/answers/unread updates; do not compare its changing bytes to the staged hash |
| Corrupt/zero-byte/unrecognized main | Fail closed; preserve main, sidecars, manifest and snapshots; never initialize/replace it |
| Missing main, prepared manifest and valid hashed old snapshot, no orphan sidecars | Restore old; never promote uncommitted staging |
| Missing committed main, orphan sidecars, invalid manifest, mismatched generation or required snapshot hash | Fail closed and preserve artifacts for explicit recovery |
| Missing main with only legacy rollback/replaced/importing artifacts and no manifest | Fail closed; old heuristic promotion is intentionally removed |

Snapshot and main validation includes a nonempty SQLite header, parent read-only integrity check, expected native research tables and foreign-key validation. A zero-byte file is explicitly rejected even though SQLite's `quick_check` alone would accept it as empty.

## Verification

Run from the workspace root:

```powershell
cargo test --locked --manifest-path desktop/src-tauri/Cargo.toml --lib research:: -- --test-threads=1
```

`research::` includes both the existing research tests and private recovery helper tests. Every database/manifest/package mutation in these tests uses `std::env::temp_dir()` paths. No real `user_data` reads/writes or app-data destructive tests are used. Existing retrieval fixtures are read-only repository test data.

Coverage added:

- Stale user tables are cleared; fresh threads/answers/read state win.
- End-to-end import then rollback after creating a thread, deleting a thread, adding an answer to an existing thread and changing unread flags.
- Reused chunk IDs with different content cannot rebind history; unavailable snapshots survive repeated delivery.
- Fault injection after generation reservation, old snapshot, staging, prepared manifest, main replacement before phase update, activated manifest and committed manifest; repeated recovery; monotonic retries.
- A committed main with later user writes wins even if a retired snapshot is damaged.
- A pinned SQLite reader causes a busy checkpoint; WAL bytes are unchanged and committed WAL data survives recovery.
- Corrupt main/old snapshot, valid-but-tampered snapshot, invalid manifest, missing main and orphan staging fail safely.
- Legacy rollback rebuilding and both v1/v2 package delivery paths.

Latest serial Windows host run: **48 passed, 0 failed, 0 ignored** (9.43 seconds test execution). `git diff --check` passed for the tracked research change. Existing unrelated dead-code/linker warnings remain. Final review covered generation transitions, current-state replacement, corruption behavior, lock ordering and unchanged async dispatch. Earlier blocked runs encountered concurrent parent edits (an unfinished prompt-upgrade function, then duplicate credential command macros); these were not edited by D3. The initial integration Cargo run resolved the parent's newly added dependencies and automatically refreshed the shared lockfile; subsequent runs use `--locked`. D3 did not edit dependency declarations.

## Explicit limits / remaining integration work

- Tests inject failures at durable protocol boundaries; they are **not** real OS force-kill, power-cut, ENOSPC, disk-controller or Android device tests. Windows host tests do not establish Android fsync/force-stop behavior.
- The protocol relies on the parent's atomic replacement/fsync contract and the existing in-process exclusive guard. Multi-process writers/external SQLite editors are not newly supported; parent single-instance integration remains important.
- Snapshots are intentionally retained. Bounded storage retention/GC is not implemented: a future policy must preserve current authority, the manifest, the referenced rollback and any unfinished recovery evidence. This favors recoverability over disk reclamation.
- `atomic_write` accepts bytes, so activation/recovery materializes one whole DB image in memory (the helper may also allocate verification bytes). A streaming durable file-replace API is a future optimization, particularly for Android.
- Corrupt-main and ambiguous legacy recovery are intentionally manual/fail-closed, not an automatic data-loss-prone fallback. UI should surface the error and offer explicit, verified recovery.
- Parent backup/restore must coordinate under the same exclusive guard. Do not restore only `research.sqlite` underneath a conflicting delivery manifest: restore a coherent journal/artifact set or explicitly establish a newly validated delivery baseline. D3 does not implement the separate user backup/restore workflow.
- Portable export publication, runtime job architecture, other native modules, frontend rendering and packaging/release validation are outside this change.
