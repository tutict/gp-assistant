# L2 native jobs — parent integration delivery

## Files and ownership

Only `desktop/src-tauri/src/jobs.rs`, `desktop/frontend/src/lib/nativeJobs.ts`, its adjacent test, and this document are authored by this delivery. No registration, Cargo, runtime, market, screening/PIT, research, route or App changes are made. No commits. This is an **opt-in orchestration wrapper**, not a replay queue or a distributed engine.

## Parent registration and startup (required)

1. Parent has registered `mod jobs;` in the native crate root; do not add a duplicate.
2. Parent has registered `jobs::api_job_run`, `jobs::api_job_status`, and `jobs::api_job_cancel` in `tauri::generate_handler!`; do not add duplicate hooks.
3. In the existing single-threaded setup, after the app-data path is available and single-instance enforcement is established, call `jobs::init(app.handle())?`. Convert the returned string to `std::io::Error::other` if the setup return type requires it. Run initialization **before accepting job commands**. Initialization failure must be visible; never silently fall back to untracked execution.
4. No new dependencies are required. Existing `futures`, `tokio`, `serde`, `serde_json`, `sha2`, `rusqlite`, Tauri and shared durability helpers are used. `init` is idempotent for the same managed app state. Do not open another `Jobs` manager against the same DB in a live process. The DB is NOT a multi-process lease system.

Native command signatures:

```rust
api_job_run(app: AppHandle, job_id: String, kind: String, payload: Value) -> Result<Value, String>
api_job_status(app: AppHandle, job_id: String) -> Result<JobStatus, String>
api_job_cancel(app: AppHandle, job_id: String) -> Result<JobStatus, String>
```

Tauri JS arguments use camelCase: `{ jobId, kind, payload }` / `{ jobId }`. `api_job_run` returns the **unwrapped original result** to its original invocation, only after lifecycle persistence succeeds. No event listener or result polling is necessary. The internal owned task survives an IPC receiver disappearing; explicit native cancellation or the native deadline stops it. There is no background retry or startup replay.

## Exact allowlist, payloads and budgets

All payloads must be JSON objects, at most **256 KiB serialized**, maximum nesting depth 32. Kind names are case-sensitive. IDs: 1–80 ASCII letters/digits/`_`/`-`; use random UUIDs, never business data or secrets. Results returned through the wrapper are capped at **1 MiB serialized**. No result is persisted.

| Kind / frontend source command | Native dispatch | Payload | Deadline / concurrency |
|---|---|---|---|
| `market_refresh` / `api_market_refresh` | Existing `market::api_market_refresh` | Original refresh payload plus normalized defaults: `batch_count=4` (1–8), `max_candidates=2000` (1–6000), `max_failed_batches=3` (1–3), `batch_start=0` (0–100000). Explicit out-of-range values are rejected, not clamped. Other existing refresh fields pass through. | 90 seconds / 1 |
| `backtest` / `api_backtest` | Existing `screening::api_backtest` | Original `{ payload }` object, unchanged. Example `{strategy_mode:"adaptive_swing_v1", as_of_date:"20250101"}`; native validation, history prerequisites, PIT, adaptive release checks remain authoritative. | 180 seconds / 2 |
| `research_pack_import` / `api_research_pack_import` | Existing `research::import_app_pack(&app,&payload)` inside `runtime::run_io_bound` | Exactly one nonempty string field: `{path:"C:/.../research.sqlite"}` **or** `{bytes_base64:"..."}`. Base64 remains within the 256 KiB IPC job budget; use path mode for larger packs. The native portable-pack reader retains its existing 64 MiB file/content cap and recovery protocol. | 120 seconds / 1 |
| `research_rebuild_index` / `api_research_rebuild_index` | Existing `research::api_research_rebuild_index` | `{}`; extra fields rejected. Rebuilds local FTS, not embeddings. | 120 seconds / 1 |

Deadlines begin at executor admission entry and cover queuing + native async execution. Admission/start/terminal durability barriers are awaited, not abandoned on deadline, to prevent late DB writes orphaning execution. SQLite busy timeout is 250 ms; metadata-lane acquisition timeout is 2 seconds. OS filesystem stalls, synchronous work inside existing async commands, and already-running blocking operations **cannot be forcibly preempted**, so these are cooperative budgets, not wall-clock hard-kill guarantees.

Same-resource dedupe is conservative: all market refreshes target one `market` resource (even different page requests); imports and FTS rebuilds share one `research` resource. A second distinct ID gets `job_resource_busy`, not a silently joined result for a different payload. Backtests share their own two-slot lane. There are at most 16 queued/running/quarantined in-memory entries. There is no automatic batch loop, automatic retry, or paid-AI dispatch.

### Intentional import difference: parent must acknowledge before mapping

The pack-import **command** schedules detached embedding work after its synchronous import. This wrapper uses the same existing import primitive but **does not schedule embeddings**. That primitive already rebuilds FTS and preserves the existing import/recovery semantics. The helper exposes the mapping explicitly; the parent must opt in knowingly and offer a separate explicit index/embedding action if needed. Do not claim it is a fully transparent replacement of the old command's background embedding side effect.

## Frontend wiring

Use `nativeJobInvocation(command,args,jobId)` for mapping inspection; it returns `null` for every unsupported command. Use `invokeNativeJob(invoke, command, args, options)` to actually invoke with AbortSignal handling:

```ts
const result = await invokeNativeJob(invoke, "api_backtest", { payload }, {
  signal,
  onCancelError: (_error, jobId) => showCancellationUnconfirmed(jobId),
});
```

- Omitted `jobId` creates a fresh `crypto.randomUUID()`. Every **user retry** uses a fresh ID.
- An explicit ID is only for retransmission of the same logical invocation; native fingerprints reject different payloads. To retain the ID for status inspection, allocate it first with `newNativeJobId()` and supply it.
- Abort sends `api_job_cancel` once and rejects immediately with `AbortError`; native delivery errors go to `onCancelError` (or a warning with ID only). Late results/errors are consumed, not applied to UI. Listeners are removed at settlement.
- Already-aborted fresh requests dispatch nothing. Already-aborted explicit retransmissions send cancel for the potentially existing invocation without sending another run.
- **Do not apply the old route abort/agent-cancel handler as well.** Only the allowlisted branch uses this helper. Unsupported commands are rejected by `invokeNativeJob`; the parent keeps original routing and cancellation behavior for them. Agent, news synthesis and research query commands are not mapped.
- Parent-owned multi-invocation refresh loops must check the AbortSignal before scheduling each new wrapper call. This module cancels one invocation's native async batches; it cannot stop an outer frontend loop that ignores its signal.
- Wrapper deadlines are native; avoid a shorter legacy JS timeout that simply abandons the request without aborting its controller. A status call or retransmit never restarts execution.

## Persistence and duplicate rules

`app_data/jobs.sqlite`, schema `user_version=1`, uses the shared durability configurator (WAL + FULL), 250 ms busy timeout, WAL autocheckpoint every 64 pages, 4 MiB retained journal target, and 16384-page DB ceiling (~64 MiB at the new DB's normal 4 KiB page size). WAL targets do not constitute a strict live-WAL file cap under externally pinned readers. Single app owner/no external long-running readers is required.

Only bounded opaque ID, allowlisted kind, SHA-256 fingerprint, lifecycle state, fixed reason codes, timestamps and booleans are stored. No full payload, API key, URL, native error text or result cache is stored. Fingerprints canonicalize object key ordering recursively and refresh defaults, preserve array order and JSON scalar representation, and include kind plus format version. This is exact canonical-JSON identity, not domain-level equivalence: for example omitted backtest defaults versus explicit defaults may conflict. Conservative rejection is preferable to running a different request under the same ID. Fingerprints are not encrypted secrets and do not promise resistance to guessing low-entropy input.

States: `queued`, `running`, `completed`, `failed`, `cancelled`, `interrupted`. A fresh process marks pending rows `interrupted` without replay, clears old-worker liveness uncertainty, and preserves historical side-effect uncertainty. Completed/failed/cancelled/interrupted rows are never replayed. Completed duplicate returns `job_already_completed` (no cached result); failed returns `job_already_failed`. A live duplicate returns `job_in_progress`; different fingerprint returns `job_id_conflict` first.

A cancel that races before admission records a durable cancelled tombstone; the first run binds its fingerprint but cannot execute. Cancelling a terminal ID does not change its terminal state. Records are capped at **10000**; IDs are not silently evicted, because eviction would allow a duplicate to re-execute. At capacity, new jobs and unknown-ID cancellation fail with `job_store_full`, while existing-ID reads, cancellation and finalization remain available. Parent may eventually add explicit retention/reset UX, but deleting the DB invalidates historical dedupe guarantees and must not be automatic.

If result serialization exceeds 1 MiB, the record is still `completed` with reason `result_too_large`, and original invocation gets `job_result_too_large`. Retrying that ID never executes again. If terminal persistence fails after a native side effect, the invocation returns `job_store_unavailable`, not success; the pending record and in-memory reservation fail closed, and startup later interrupts it. Inspect the underlying resource before retrying with a new ID.

## Cancellation truthfulness and concrete adapter gaps

`JobStatus` fields: `job_id`, `kind`, `state`, `reason`, `created_at_ms`, `updated_at_ms`, `cancel_requested`, `blocking_may_continue`, `side_effects_may_have_occurred`.

A watch channel is signaled immediately, independently of the metadata store lane. The executor selects cancellation/deadline against native work; the losing native future is **dropped before** terminal persistence. Consequently it cannot resume later async batches/retries. No extra native retries are added. Metadata has its own one-worker bounded lane; its owned permit lives in the blocking closure, exactly as the fixed runtime helpers retain theirs.

`blocking_may_continue=true` means **may**, not that a worker was proved running. It is set conservatively when entering running, so a blocked terminal store write cannot falsely report no outstanding work; a confirmed normal completion clears it. A cancellation race after marking `running` may set it before the first native poll. Underlying blocking CPU/IO work (including atomic commits and existing Windows network fallback processes) can finish after the async waiter is dropped. Native failures are also conservatively marked uncertain because an existing command may itself have timed out a nested blocking waiter. No rollback or all-or-nothing multi-step job transaction is promised. `side_effects_may_have_occurred` is conservative from running onwards and survives restarts.

There is no existing per-operation blocking completion hook. Therefore started cancellation/timeout/panic/native failure **retains the resource reservation and per-kind permit until process restart**. That prevents overlapping writes and premature capacity release. Queued cancellation/timeout releases its reservation immediately and reports no worker uncertainty. This is intentionally fail-closed; early release requires a parent/runtime adapter that proves completion of every nested worker. A false flag is never used to imply hard cancellation. Resource limits only govern opt-in wrapper calls; direct old commands/background tasks can bypass them until the parent routes/gates those entry points.

Unsupported without further adapters:

- `api_research_refresh`: may synthesize paid AI through news RAG and schedules detached embeddings. Not safe for this no-paid-replay allowlist.
- URL import: existing import performs synchronous parse/SQLite writes inside an async future and its command schedules detached embeddings. Needs phase-separated bounded async fetch and blocking parse/store plus completion reporting, rather than an unsafe wrapper rewrite here.
- PDF import command / embedding rebuild: command schedules detached embeddings; not mapped. The bounded PDF primitive could be supported later with an explicit no-background-work contract and payload/worker budgeting.
- Generic RAG pack builds/transfers, agent operations, screening/observe/research query, arbitrary command names: not dispatched.
- Market refresh and backtest retain existing synchronous sections/nested network fallback workers. Deadline/cancel is cooperative at async yield points; preserving their screening/PIT behavior takes precedence over rewriting those out-of-scope modules.
- Parent backup/reset integration must not hot-restore `jobs.sqlite` while this manager holds its connection. No job DB backup/restore command is added here.

## Verification / remaining integration

Tests are colocated in the new module/helper. The native unit harness includes the real jobs module, real runtime and shared durability implementation; it stubs only the four application dispatch targets (no network/paid requests). This validates lifecycle orchestration, not real native command integration. With module/command registration now supplied by the parent, run `cargo test --lib jobs::tests` and the runtime permit regression, plus `npm run test:unit -- src/lib/nativeJobs.test.ts` in the frontend directory.

Executed: isolated native harness **13 passing tests** (11 new job tests + 2 existing runtime permit tests); targeted strict TypeScript check for both new frontend files passed; `rustfmt --check` passed. The full crate/registered command integration remains parent-owned. Vitest initially hit sandbox `spawn EPERM`; escalation could not be reviewed due to an approval-service rate limit, not a test assertion failure. No app startup, network refresh, pack import or backtest is claimed validated by the unit harness.
