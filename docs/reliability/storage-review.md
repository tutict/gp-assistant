# Storage reliability review — 2026-10-06

**Scope:** current working-tree D1 durability/cache/prompt writes and permit lifetime; D2 watchlist backend/store/panel; D3 research import/rollback/recovery. Read implementation, watchlist-delivery and research-delivery contracts. Included the parent's new migration-snapshot callers in watchlist/agent ledger and sentiment's version guard. No source edits, subagents, commits, service writes or real user-data access; this report is the only repository file written.

**Result: 1 P1 + 3 P2 open findings.** These are remaining contract/correctness gaps, not four claims of newly introduced regressions. R1 was addressed during this review; R4/R5 retain pre-existing behavior. Credentials, user-backup integration and the completed unavailable-citation UI label are excluded.

## Spec compliance — findings by severity

### R2 · P1 · Existing zero-byte watchlist DB is silently recreated and re-importable
- **Ref:** [watchlist.rs:137–143](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/watchlist.rs#L137).
- **Failure:** unlike research/sentiment, open_watchlist_db does not validate an existing file before Connection::open. SQLite accepts zero bytes; backup_before_migration skips it because there are no tables; initialization creates revision 0 with migration_complete=false. The frontend then imports stale localStorage as if this were a new installation, potentially restoring previously removed stocks instead of surfacing damaged storage. The helper's zero-header test does not cover this caller.
- **Smallest fix:** distinguish absent from existing before opening; reject an existing invalid/empty header without initializing it. Keep valid empty v0 tables authoritative. Apply the same existing-file check to AgentRunStore::open, which also currently accepts zero bytes.
- **Test:** existing zero-byte file plus stale local cache must produce an error, preserve the file and dispatch no migration; missing-file installation must still work. **Reproduced with initializer SQL extracted from this source:** empty file became a database with state (revision=0, migration_complete=0); the new snapshot helper's table predicate returned false.

### R3 · P2 · Same-renderer retry accepts an obsolete duplicate acknowledgement as saved
- **Ref:** [watchlistStore.ts:183–191](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/lib/watchlistStore.ts#L183).
- **Failure:** start at revision 0; add A commits at revision 1 but acknowledgement is lost; another supported writer clears at revision 2; click retry without reloading. hydrated remains true and revision remains 0, so duplicate response revision 1 does not satisfy remote.revision < revision. The queue clears and publishes saved with A in UI/localStorage, although SQLite is empty. This is not a demand for live broadcasting: it is reconciliation of an explicitly retried uncertain operation. The reproduction did **not** change the newer SQLite rows.
- **Smallest fix:** on transport uncertainty, require a fresh snapshot during retry while retaining the exact operation ID/request; reconcile the duplicate result against current authority before publishing saved.
- **Test:** extend the existing old-duplicate test without resetModules/reload, and include a queued follow-up delta. **Reproduced using the actual TypeScript store stripped of types with an in-memory IPC/storage fixture:** DB=[], UI/cache=[A], status=saved; calls were snapshot, mutate, mutate (no reconciliation read).

### R4 · P2 · Prompt mutations still overwrite unreadable state with defaults
- **Refs:** [prompt_upgrade.rs:364–374](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/prompt_upgrade.rs#L364), mutation at [248–258](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/prompt_upgrade.rs#L248).
- **Failure:** parse failures and non-NotFound read errors become OverlayStore::default(). Status therefore reports successful builtin state; a subsequent revert or accepted activation atomically overwrites the original with that default-derived store, discarding other profiles and recovery evidence. Atomic publication does not make this fail-open read/modify/write safe. This is a retained D1 gap, not a regression caused by atomic_write.
- **Smallest fix:** return Result from load_store; default only on NotFound and propagate other errors through status/mutation callers. Any read-only builtin fallback must remain visibly degraded and must not authorize writing defaults.
- **Test:** malformed JSON and injected transient read failure; status/mutations must report failure and leave original bytes unchanged. Static call-path evidence; no new Rust test executed.

### R5 · P2 · Research modifies future-schema files before rejecting them
- **Ref:** [research.rs:2972–2980](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/research.rs#L2972).
- **Failure:** initialize_schema calls configure_connection before checking user_version. An otherwise recognizable version-99 DB in DELETE mode is persistently switched to WAL before rejection. The new watchlist/ledger/sentiment guards avoid this; research still violates future-schema non-mutation. No row loss is claimed.
- **Smallest fix:** check supported version before any writable configuration; also enforce it in research recovery validation before any manifest/main mutation.
- **Test:** future-version main with the expected native tables: hash main and preserve sidecars/journal policy across rejected open/import/rollback. **Extracted-schema/helper SQL reproduction:** version 99 rejected only after DELETE→WAL and original file bytes changed.

## Code standards assessment / reviewed non-findings

No repository-wide AGENTS/CONTRIBUTING/CODING_STANDARDS file was found. Reviewed PRODUCT/DESIGN state/error guidance, React external-store use, error propagation, transaction/lock boundaries and allocation behavior; no stylistic churn proposed. R4/R5 also identify fail-closed error-handling and validation-before-mutation defects; R3 contradicts truthful saved-state feedback.

- Blocking closures now own CPU/I/O permits through actual task exit; cancellation no longer releases those permits prematurely. No additional lifetime bug found in that change.
- Checkpoint inspects busy and frame counts before research quiesce deletes sidecars. New migration snapshots use consistent VACUUM INTO and do not intentionally reactivate an old backup.
- Watchlist duplicate lookup precedes revision validation in one IMMEDIATE transaction; valid empty-v0 authority and one-shot migration are implemented. Panel undo changes one item, not a historical full list.
- Research replaces old user tables with current state, preserves unavailable citation payloads, and keeps committed-generation main authoritative. No additional high-confidence deleted-history resurrection or unavailable-identity bug found in those paths.

## Addressed during review

**R1 — former whole-DB activation/recovery buffering: no longer open.** Readback confirmed [durability.rs:51–80](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/durability.rs#L51) streams and SHA-256 verifies with one 64 KiB buffer, syncs the temporary file, atomically replaces the destination and syncs its parent. Both [research_recovery.rs:327](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/research_recovery.rs#L327) and [431](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/research_recovery.rs#L431) now use atomic_copy. This resolves the identified full-DB buffers at those sites; it is not a claim of an app-wide Android memory bound. Parent is rerunning fault tests. Suggested follow-up: large synthetic-file peak-memory check and interruption during copying, not only after installation.

## Verification boundary

This review ran one in-memory actual-store JS reproduction, two isolated temporary SQLite SQL reproductions, and scoped git diff --check (passed; LF/CRLF notices only). SQL reproductions exercise extracted SQL, **not** complete Rust/Tauri entrypoints. No test sources were added. Parent-reported D1=8, D2=10, D3=48 Rust passes are recorded as supplied evidence, not rerun or treated as proof against the gaps above. Android force-stop/power-loss/memory validation remains outstanding. Line references describe the reviewed working tree; concurrent parent edits can shift them.
