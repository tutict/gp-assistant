# Rust governance development guards

This standalone test crate uses `syn` only as a development dependency. The application crates gain no production dependencies.

The committed contract and test inventories are frozen from `ecc119e1`. Do not regenerate them to accept an implementation change. A private test relocation is entered explicitly in `test-relocations.json`; public Rust/Tauri/C ABI contracts stay unchanged.

From a PowerShell prompt at the repository root:

```powershell
$taskRepoRoot = (Get-Location).Path
$env:GP_ARCH_FIXTURES = Join-Path $taskRepoRoot 'artifacts/rust-governance/fixtures'
cargo test --locked --manifest-path scripts/rust-architecture/Cargo.toml --test architecture
cargo test --locked --manifest-path scripts/rust-architecture/Cargo.toml --test architecture -- compare $taskRepoRoot (Join-Path $taskRepoRoot 'scripts/rust-architecture/public-contracts.json')
cargo test --locked --manifest-path scripts/rust-architecture/Cargo.toml --test architecture -- check $taskRepoRoot all
py -3.13 scripts/rust-architecture/test_verify_tests.py
py -3.13 scripts/rust-architecture/test_check_exact.py
py -3.13 scripts/rust-architecture/test_run_exact.py
```

Cargo runs the custom test executable inside this tool crate, so comparison roots and baseline paths passed through Cargo must be absolute. During staged extraction, `check` may select `core`, `research`, or `market`; final integrated CI checks `all`.

`verify_tests.py` consumes an actual Cargo execution log and requires every original test identity to have run successfully or retained its original explicit ignore. Keeping the total count while replacing a case fails the guard.

`check_exact.py` verifies both an exact listing and the actual execution of the requested case. Zero selection, another test, or an ignored case cannot satisfy that gate.

`run_exact.py` invokes locked Cargo with `--lib` for a listing and an execution, preserves each native exit before checking evidence, and calls the existing exact verifier. Raw console logs remain unchanged; CRLF console decoding follows the same universal-newline behavior as the verifier's text-file reader. This is console parsing, not source/data fingerprint normalization. The two original research case names stay unchanged.

The AST guard resolves declared logical modules separately from physical paths, including `#[path]`, grouped/renamed imports, `crate`/`self`/repeated `super`, block imports, and explicit reexports. Public methods attach to the canonical exported type. Ordinary macro argument expressions are inspected without macro expansion. Platform/feature alternatives remain visible in the syntax union; ambiguous aliases fail closed. Receiver dispatch, trait default/callback bodies, transaction/lock/generation lifetime and behavioral preservation still require complete source/state audits and meaningful application tests.

`governance-policy.json` fixes the reviewed ordered core67/native-research13/desktop11 fingerprint descriptors, including two existing desktop derived identities. It adds no production fingerprint protocol or Tauri research/market coverage. Future approved extractions require an independently reviewed development descriptor update; the guard never learns an allowed inventory from the current code. The narrow pack-delivery exception applies only to the two Supervisor-reviewed free functions and recovery targets; it exempts no storage module or Store method.

Guard self-tests write small synthetic sources to the explicitly named `GP_ARCH_FIXTURES` directory. Development test fixtures and subprocess controls do not run application suites. The reviewed workflow and guards are applied locally and verified by the recorded local entry-point gates, including both actual exact CI helper routes. Remote CI workflow execution remains unobserved. Candidate-only reviews remain historical evidence.

`abi_probe.py` loads the built Windows core DLL, verifies all fifteen C ABI exports, sends synthetic/invalid JSON, null pointers and invalid UTF-8, frees every returned string, and compares fifty deterministic results. It masks only implementation identity and its derived parameter/run identity fields; dataset facts, business outputs and the fixed built-in parameter identity are preserved.

Frozen PIT fixtures are byte-addressed and protected by `.gitattributes -text`; preserve their declared payload hashes and exact bytes. Source-text LF normalization must never be applied to dataset, report or parameter digests.

These guards do not confer calibration or release qualification. Current-source qualification must be generated anew using the normal real-input workflow; old IDs and receipts remain immutable.

