# R2/R3 follow-up — current contract (2026-10-06)

This addendum supersedes the original schema-v1/retry description below. Changes in this follow-up are limited to the existing watchlist backend, store, its regression tests, and this delivery report. No App/lib/Cargo edits, commits, or subagents. The review document itself was not rewritten.

## Fixes and regression evidence

- **R2:** `open_watchlist_db` delegates to a path-based opener that distinguishes missing files from existing paths. Every existing file passes `crate::durability::validate_sqlite_header` BEFORE `Connection::open`. Empty/truncated/invalid files return errors without modification, WAL creation, or migration re-enablement. Missing files still initialize normally. The parent's `backup_before_migration` call remains before schema changes.
- **R3:** an operation with an already-assigned expectedRevision is a replay. After replaying the exact operation ID/request and receiving an acknowledgement, the store ALWAYS fresh-reads the authoritative snapshot before removing that queued operation, advancing a follow-up, updating cache/UI to acknowledged data, or publishing saved. A failed authority read retains the original operation and all follow-ups for retry. No module reload is necessary.
- **Observed RED:** both same-renderer lost-ack tests (with/without a queued follow-up) failed because there was no snapshot read. The real production path opener accepted an existing zero-byte file. Five pristine-state tests failed on the missing column. These now pass.

## Exact schema ready for the backup parent

`PRAGMA user_version = 2`. The `watchlist` and `watchlist_operations` shapes are unchanged. `watchlist_state` is exactly:

```sql
CREATE TABLE watchlist_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    revision INTEGER NOT NULL CHECK(revision>=0),
    migration_complete INTEGER NOT NULL CHECK(migration_complete IN (0,1)),
    restore_pristine INTEGER NOT NULL DEFAULT 0 CHECK(restore_pristine IN (0,1))
);
```

Creation/mutation contract:

| Event | restore_pristine |
| --- | --- |
| Genuinely missing file + newly created schema | 1 |
| Empty one-time localStorage migration on that fresh DB | Retains 1 |
| Existing v0 DB (even empty) | 0 |
| Existing v1 DB (any rows/revision/migration marker) | 0 |
| Any delta, including no-op, or clear | 0 |
| Nonempty migration that actually imports data, or legacy write/import | 0 |
| Failed transaction | Original value retained |
| Already-used/cleared DB followed by an empty migration | Remains 0 |

The v1 → v2 migration is an IMMEDIATE transaction adding the column with default 0 and setting user_version=2. It preserves revision, migration_complete, rows, and operation receipts. A disk test verifies the parent helper also keeps a readable pre-v2 SQLite backup with the old schema and committed data.

**Backup integration:** validate this v2 shape/version, permit initialized-target restoration only while the singleton restore_pristine is 1 (and the target is empty), and set it to 0 together with migration_complete=1 and the revision change in the same restore transaction. This follow-up does not implement or modify the parent's native restoration code. No App/lib/Cargo wiring is needed for these fixes.

## Verification after follow-up

From `C:/Users/tutic/IdeaProjects/gp-assistant`:

- `npm.cmd --prefix desktop/frontend run test:unit -- src/lib/watchlistStore.test.ts src/components/panels/WatchlistPanel.test.tsx` — **24 passed** (19 store + 5 panel).
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib watchlist::reliability_tests --no-default-features` — **17 passed**; existing unrelated compiler/linker warnings only.
- `rustfmt --check --edition 2021 desktop/src-tauri/src/watchlist.rs` — passed.
- Scoped `git diff --check` — passed (only repository LF/CRLF notices).

The wider frontend run at 16:48 was **not green**: 486 tests passed, 11 failed, and one additional suite could not load. Failures were in concurrently edited `tauri.local-data.test.ts`, `tauri.observe.test.ts`, and the then-missing `nativeJobs` module, not watchlist tests. TypeScript reported the same missing/in-progress native-job/local-data symbols. Those files were not edited here. Real device/power-loss/native IPC checks were not repeated for this follow-up.

---
# D2 watchlist reliability delivery

## Scope and changed files

Implemented directly, without commits or subagents. No integration or dependency files were edited by this delivery. Other concurrent parent changes were left untouched.

- `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/watchlist.rs`
- `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/lib/watchlistStore.ts`
- `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/lib/watchlistStore.test.ts` (new)
- `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/components/panels/WatchlistPanel.tsx`
- `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/components/panels/WatchlistPanel.test.tsx`
- This explicitly permitted delivery report: `C:/Users/tutic/IdeaProjects/gp-assistant/docs/reliability/watchlist-delivery.md`

## Implementation

### SQLite protocol

- New `api_watchlist_snapshot` returns `{items, revision, migrationComplete}` from one read transaction.
- New `api_watchlist_mutate` accepts `{payload: {operationId, expectedRevision, mutation}}`.
- Mutations are `{kind:"delta", upserts:[...], removes:[...]}`, `{kind:"clear"}`, or the one-shot `{kind:"migrate", items:[...]}`.
- An IMMEDIATE transaction checks the operation ledger before checking revision, applies the mutation, increments the revision, and commits its original response with its exact request payload.
- An identical committed retry returns the original response without writing again. Reusing that ID with a different payload is rejected with JSON-string error code `WATCHLIST_OPERATION_REUSED`.
- Revision mismatch is explicitly rejected with JSON-string error code `WATCHLIST_CONFLICT` and the current `revision`. Rejected/rolled-back requests do not consume an operation ID.
- Legacy list/replace/add/remove/clear Tauri signatures and array results remain compatible; every legacy write uses the same transaction/version state. Daily frontend changes no longer call whole-array replace.
- SQLite schema version 1 adds `watchlist_state` and `watchlist_operations`. Migration is transactional, preserves existing v0 rows, and rejects future versions before changing the database journal policy.
- Every connection uses WAL, synchronous FULL, and a 5000ms busy timeout. This implementation is independent of the parent's durability helper and needs no dependency or helper integration.

### Migration policy

`watchlist_state.migration_complete` is the authoritative one-time localStorage import marker, not an empty-array heuristic. A pre-existing v0 watchlist table, including an empty one, is already authoritative and gets marker=true. Only a newly created database starts with marker=false. The first migration (even with no local items) permanently completes migration; any ordinary write also completes it. Clearing the database never re-enables import.

### Frontend and UI

- Existing App-facing load/setter helpers remain compatible. The setter computes per-item deltas rather than sending snapshots.
- A single-flight queue waits for the initial native read and then for each write acknowledgement. Initial reads reconcile SQLite with queued local intent without erasing the optimistic cache.
- A versioned localStorage outbox saves exact attempted operation IDs/revisions before dispatch; unacknowledged requests survive renderer reload when storage succeeds. It preserves the pre-edit migration snapshot separately.
- Errors retain queued edits, stop automatic writes, and expose explicit retry. Transport retries preserve the exact request. Explicit conflict retry reloads SQLite and rebases only the rejected delta, preserving unrelated rows.
- An old duplicate acknowledgement is reconciled against the newer SQLite revision instead of resurrecting historical rows. Post-ack outbox-cleanup failures can also be retried without replaying the write.
- Exported external-store API: `subscribeWatchlistPersistence`, `getWatchlistPersistenceSnapshot`, `retryWatchlistPersistence`. The stable snapshot includes status (`saving`, `saved`, `error`), pendingCount, error, and storage (`sqlite` or `local`).
- The panel subscribes with useSyncExternalStore. Optimistic updates are marked unsaved before reaching the UI; native saved is shown only after acknowledgement. Browser-only mode is labeled separately.
- Per-item undo restores only the removed item and its metadata, not an obsolete entire list. Clear still requires confirmation. Existing CSS classes are reused.

## Exact minimal parent integration

In the existing `tauri::generate_handler!` list in `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/lib.rs`, keep/add exactly these entries alongside all five existing watchlist commands:

```rust
watchlist::api_watchlist_snapshot,
watchlist::api_watchlist_mutate,
```

At final readback, these two entries were already present from the parent. Do not add duplicates. **No App.tsx integration is required**: the existing createPersistentWatchlistSetter/loadPersistentWatchlist calls work unchanged; status and retry are wired inside WatchlistPanel. No Cargo or durability-module changes are required.

## Test-first evidence

Before implementation, the actual old store failed the regression tests:

1. Expected one write while the first acknowledgement was deferred, but observed two concurrent writes.
2. Expected cached `[A,B]` after the late initial read, but observed `[A]`.

Before UI implementation, saving feedback and per-item undo tests failed because the status text and undo controls were absent. Further red/green cycles caught old duplicate-ack state regression, a subscriber-enqueued write stranded at the saved notification, premature saved status during optimistic publication, stock-alias deltas, and failed post-ack outbox cleanup.

## Final verification (2026-10-06, Asia/Shanghai)

Commands and results:

| Working directory | Command | Result |
| --- | --- | --- |
| `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend` | `npm.cmd run test:unit -- src/lib/watchlistStore.test.ts src/components/panels/WatchlistPanel.test.tsx` | **20 passed** (15 store + 5 panel) |
| `C:/Users/tutic/IdeaProjects/gp-assistant` | `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib watchlist::reliability_tests --no-default-features` | **10 passed**, 0 failures; unrelated existing dead-code/linker warnings |
| `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend` | `npm.cmd run test:unit` | **469 passed, 74 files passed**, no failed suites |
| `C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend` | `npm.cmd exec -- tsc --project tsconfig.json --noEmit` | **Passed**, exit 0 |
| `C:/Users/tutic/IdeaProjects/gp-assistant` | `rustfmt --check --edition 2021 desktop/src-tauri/src/watchlist.rs` | **Passed** |
| `C:/Users/tutic/IdeaProjects/gp-assistant` | scoped `git diff --check` | **Passed**, only repository LF/CRLF notices |

Backend tests cover ordered deltas, stale conflicts, duplicate retries/payload mismatch, once-only and empty migration, existing empty/populated v0, future-schema rejection, transaction rollback/ID reuse, shared legacy revisioning, actual disk reopen, WAL/FULL/timeout settings, and concurrent writers.

Early wider checks were temporarily blocked by the parent's in-progress research/credential/backup code. Those blockers were not edited here; the final checks above supersede them.

### Browser QA

Browser plugin skill was not available, so an isolated headless Chromium check used the existing Playwright and Vite dependencies via an inline `node --input-type=module` fixture. No fixture source files were added. The temporary localhost server and browser were closed in finally blocks.

The flow was: isolated WatchlistPanel -> remove -> saving -> acknowledged saved -> per-item undo -> simulated write failure -> retained item and error -> explicit retry -> saved. Native transport was synthetic; no real user data was used. Page URL/title were checked, meaningful DOM rendered with no framework overlay, console/page errors were empty, and horizontal overflow was false at 390px width. Desktop and mobile screenshots were inspected:

- `C:/Users/tutic/AppData/Local/Temp/gp-watchlist-D2-error.png`
- `C:/Users/tutic/AppData/Local/Temp/gp-watchlist-D2-mobile.png`

## Limitations / intentional boundaries

- Real Tauri IPC/device force-stop/power-loss testing and release bundling were not run; browser transport was a fixture. Rust tests use real temporary SQLite files and connections.
- The operation ledger is intentionally not garbage-collected: arbitrary-age duplicate retries remain safe, at the cost of storage growth.
- The frontend outbox has one renderer owner. Backend revision checks protect concurrent SQLite writers, but this does not add cross-window outbox coordination or live-change broadcasting.
- If localStorage cannot accept the outbox, edits are retained in memory and native dispatch is blocked with an error. Do not claim those unjournaled edits survive process termination. A corrupt/future outbox is preserved and surfaced, not silently discarded.
- Undo history lives in the mounted panel; it is not a persisted undo journal. Clear confirmation remains, without adding a global clear-undo feature.
- Legacy array-replace callers still have their old replacement semantics; new daily edits must use the delta store. Existing v0 empty databases deliberately do not import an old cache, because doing so could resurrect an intentional clear.
