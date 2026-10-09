# Local diagnostics, safety controls and reliability verification

## What O1 does — and does not do

This is an offline, per-installation control surface. There is no telemetry endpoint,
remote configuration/kill switch, automatic report upload, crash dump collection or
measured crash-rate dashboard. It does not alter screening/PIT semantics or claim
that host unit tests validate Android force-stop or Windows installer behavior.

Diagnostics have **no free-text event input**. The native `OperationalEvent` enum
permits only `started`, `clean_shutdown`, `previous_unclean_exit`,
`storage_check_failed`, `storage_recovery_completed`, `offline_operation_completed`,
`background_request`, `gepa_setting_changed`, and `gepa_blocked`. It accepts no question,
stock code, URL, path, provider key, request/run ID, timestamp, arbitrary payload or
error string. Events never include business/LLM output. Do not extend this schema with
caller-controlled strings. The Rust-only event function is deliberately not an IPC command.

Each process keeps the latest **64** events in memory. Per-kind counters and the
number of evicted events saturate at **1,000,000**; counters cover the session, not
just the ring. There are at most nine counters. There is no event-log disk growth.
The bounded preview is less than 8 KiB. Session state disappears at restart unless
the user explicitly exported it. Fixed-code read/write errors cannot leak file paths.
Storage/recovery/offline/request counts only reflect call sites the parent actually
instruments. A zero/unrecorded counter is not proof of zero real-world activity.

### Preview and manual export

Settings → 本地诊断与安全控制 → 预览本地诊断 displays the exact allow-listed
snapshot; the frontend rejects unexpected fields, event names, counter types and
quotas. Tick 确认诊断导出, then 导出已预览诊断 to request a local
`gp-local-diagnostics.json` download. A new preview or preference change invalidates
the prior confirmation. There is no automatic download or remote submission.
The UI cannot verify that a browser/WebView actually saved the Blob. Android WebView
Blob downloads still need device verification; manually copying the visible sanitized
preview is an honest fallback, not an upload. Inspect any export before sharing it.
The diagnostics envelope contains no `device_id`; performance artifacts below are a
separate manually authored operator workflow, never automatically included in export.

## Startup marker and orderly shutdown

After acquiring the app's single-instance guard, the parent calls
`diagnostics::init(&app_data_root)` before optional jobs. The fixed child files are:

- `<app-data>/diagnostics/lifecycle.json`: `{ "schema_version": 1, "clean_shutdown": false }` during an active session.
- `<app-data>/diagnostics/features.json`: `{ "enabled": false }` or `{ "enabled": true }` after a user preference save.

Files are read with a 4096-byte cap and a strict schema; malformed, unsupported or
oversized data is preserved and initialization fails closed. Missing preference
means false. Writes reuse the parent's synced, atomic `durability::atomic_write`.
No delete-before-replace operation is introduced. A failed flag write never publishes
an enabled in-memory flag. Diagnostics are not a replacement for database integrity
checks, migration backups, WAL handling, or storage recovery.

The previous marker yields `first_run`, `clean`, or **`unclean`**. Unclean means the
last process did not finish the final marker write. It may mean forced termination,
Android eviction/force-stop, power loss, a failed shutdown write or a crash; it is
**not a verified crash**. No verified-crash counter or inferred crash rate exists.
Marker absence cannot distinguish first install from manual deletion of the marker.

Call `mark_clean_shutdown()` only on final orderly exit after jobs stop and required
stores/checkpoints flush successfully. Never call it for a cancellable close request,
background/suspend event, panic, failed flush or force-stop. It is idempotent. A failed
marker write returns an error; do not claim clean shutdown. Multiple processes must
not share a diagnostics directory without the parent's single-instance guard.

## Local GEPA flag and safe start

GEPA is conservative-off even in a GEPA-capable build. The settings toggle persists
a boolean preference. Build feature/platform constraints still apply; the toggle
cannot enable code absent from the binary. Status, new experiment start and apply
all gate on the native preference/safe-start state; report reading and cancellation
remain accessible. Existing atomic GEPA report writes are unchanged.

Launch the executable with the exact argument `--safe-start`, or set
`GP_ASSISTANT_SAFE_START=1` (also accepts exact `true`) in the launching process
before startup. Example on Windows, using your installed executable location:

```powershell
$env:GP_ASSISTANT_SAFE_START = '1'
# Start your installed app, or pass --safe-start when launching it directly.
# To end this opt-in for the next normal launch:
Remove-Item Env:GP_ASSISTANT_SAFE_START
```

Safe start disables GEPA for this session but does not erase the saved preference.
A normal subsequent start restores the preference. It **never disables integrity
checks**, migration checks or recovery safety. This is specifically a GEPA safe-start
control, not a blanket promise to stop every background feature. Android app launcher
argument/environment plumbing is parent/device work; there is no hidden remote control.

Disabling GEPA does not cancel an already running experiment; explicitly cancel and
wait for it before toggling off. The native gate is authoritative even if a frontend
availability indicator is stale. Parent must invalidate/refetch GEPA availability
when preferences change (the existing status effect is not an event subscription).
A full restart also refreshes it. In headless debug mode `process::exit` bypasses
normal shutdown hooks and may produce an unclean marker unless parent finalizes first.

## Performance baseline schema and measurement protocol

There are **no actual device baselines in this delivery**. Do not use the unit fixtures
as measurements. Missing readings are `requires_user`, never synthesized zeros.
The evaluator compares operator-supplied UTF-8 JSON files, each at most 64 KiB:

```json
{
  "schema_version": 1,
  "device_id": "operator-assigned-anonymous-device-label",
  "platform": "windows",
  "fixture_id": "versioned-fixture-and-protocol-digest",
  "metrics": {
    "coldstartup_ms": null,
    "offlinep95_ms": null,
    "peak_memory_bytes": null,
    "background_requests": null
  }
}
```

This schema example intentionally contains **invalid placeholders**, not fabricated
readings: replace all nulls with real measured, nonnegative, finite JSON numbers.
Memory and request count must be safe integers. Unknown/missing metrics, unknown
fields, wrong types, invalid schema and invalid exemptions reject. The metadata
fields are 1–96 character opaque labels (`A-Za-z0-9._-`, first character alphanumeric).
They must all exactly match between baseline and candidate. Do not put hardware
serials, usernames, file paths or credentials in device/fixture labels. Distinct
Windows/Android devices require separate pairs; do not compare cross-device values.

Record the binary version/hash, build profile, OS/device configuration, power/thermal
state, caches, fixture digest, measurement-tool version and protocol in a separate
operator evidence note. Bind a canonical fixture + protocol digest into `fixture_id`;
changing a measurement method, OS/device configuration or fixture invalidates comparison.
Do not relax metadata matching or use an exemption to cross that boundary.

Suggested reproducible protocol (record the exact choices with the fixture):

1. Run release artifacts, no devtools/debug build. Use a fixed synthetic/offline
   dataset and identical clean app-data seed with no real credentials. Do not run
   against the user's live databases. Pause unrelated workloads and fix device power
   mode; record all process/network measurement tools used.
2. `coldstartup_ms`: median of at least ten separately launched, fully terminated
   cold-process runs, from process launch to the agreed first usable offline view.
   Define cache/reset policy and ready signal in the protocol. Include startup
   integrity checks; never disable them to improve the number. Do not use build time.
3. `offlinep95_ms`: nearest-rank p95 of at least 100 repetitions of a fixed offline
   action sequence after a specified warm-up, measured with a monotonic clock. Define
   action boundaries and retry/error treatment; rejected/failed samples are not discarded.
4. `peak_memory_bytes`: externally measured peak of the agreed process tree over the
   entire startup + offline action + background-window workload. Use consistent OS
   definitions (e.g. Windows private bytes versus Android PSS are different metrics,
   not cross-platform equivalents); specify sampling interval and include WebView
   children. Do not use an arbitrary instantaneous process reading as the peak.
5. `background_requests`: externally observe attempted outbound requests during a
   fixed idle/background window (recommended five minutes), with no deliberate user
   network actions. Record the request layer, retries, process attribution, DNS and
   socket/HTTP counting rules. Offline failures still count as attempts. In-memory
   diagnostic counters alone do not verify network silence.
6. Preserve raw samples/tool output locally. Populate the four aggregate readings
   only once derived from those samples. Capture baseline and candidate on the same
   device/protocol. Separately perform Windows close/forced-termination and Android
   background/force-stop/relaunch checks, inspecting marker semantics and user data.

Evaluate from the repository root:

```powershell
node scripts/reliability-benchmark.mjs --baseline <actual-baseline.json> --candidate <actual-candidate.json>
```

Every metric is lower-is-better. **Greater than 10%** regression fails; exactly 10%
passes. Zero→zero passes, zero→positive fails. Exit 0 means pass or explicitly exempted;
exit 1 means unexempted regression; exit 2 means invalid/missing/incompatible input
(`requires_user`). This validates supplied data, not its measurement provenance.

An intentional regression requires an explicit `--exemptions <file.json>` argument:

```json
{
  "schema_version": 1,
  "exemptions": [
    { "metric": "coldstartup_ms", "reason": "Approved temporary integrity-check cost; track investigation and expiry in REL-123." }
  ]
}
```

Each exemption is unique and metric-scoped, must apply to a real regression, and
requires a nonblank 20–1000 character reason. Unknown metrics and blanket skip flags
reject. The report retains the reason and says `exempted`, not `pass`. Automation
checks structure, not the truth of the justification; the release owner must review
tradeoffs, evidence, approver/issue and expiry. Exemptions never waive missing data,
metadata mismatches or invalid readings.

## Release verification commands and limits

```powershell
# Pure Rust diagnostics with real shared atomic writer; temporary test data only.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/reliability-native.test.ps1 -Offline
# Hand-authored unit fixtures and actual PowerShell flag behavior, NOT measurements.
node --test scripts/reliability-benchmark.test.mjs scripts/release-check.test.mjs
# UI behavior tests (from desktop/frontend).
npx vitest run src/components/settings/ReliabilityPanel.test.tsx src/components/panels/GepaLabPanel.test.tsx src/components/settings/BackupPanel.test.tsx
# Full existing release workflow plus local reliability suites.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-check.ps1
# Explicit measured performance gate added to the normal packaging workflow.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release-check.ps1 -EvaluateBaseline -BaselinePath <actual-baseline.json> -CandidatePath <actual-candidate.json>
# Optional reviewed, metric-specific exception document:
# append -ExemptionsPath <approved-exemptions.json>
```

Without `-EvaluateBaseline` the script prominently reports **Performance NOT EVALUATED
(requires_user)**; successful host checks are not performance sign-off. Specifying any
baseline/candidate/exemption path without that flag fails. Requested evaluation with
absent inputs fails even if other suites are skipped. `-SkipReliability` explicitly
skips the new suite block but not requested performance evaluation; `-SkipRust` and
`-SkipNode` log which native/frontend reliability suites were skipped. The existing
unit suite may still include reliability tests when `-SkipReliability` is used.
Existing signed Android APK and Windows NSIS build/artifact checks, GEPA build checks,
version, CSS, frontend build and desktop harness checks remain in place.

The standalone Rust harness compiles the actual diagnostics source and actual parent
atomic writer as a temporary dependency (without compiling unrelated parent tests).
It needs cached Cargo dependencies with `-Offline`; omit that switch for initial
local dependency resolution. Temporary harness manifests stay in the OS temp directory
for inspection; compiled artifacts are reused under `tmp/reliability-native-target`.
It is not a substitute for parent integration tests of registered IPC, lifecycle
hooks, GEPA engine paths or real installer/device execution.
