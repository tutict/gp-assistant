# Preserved worktree integration

The preserved worktree started at ecc119e1, before PR #2. Its research and market extraction overlaps the reviewed implementation already on main. The additional application, HK API and test organization is integrated onto e1bf42a2.

## Application composition

The Tauri root declares modules and delegates its existing public desktop/mobile entrypoint. The app owner contains bootstrap, command registration, shell commands, desktop-window setup, lifecycle handling and application state.

AppState owns close-handler readiness, close confirmation and startup warnings. It is managed before window creation and optional-store initialization. Reload readiness, the client-close-requested event, clean-shutdown admission, window dimensions and startup warning/error payloads retain their existing behavior. The command arguments contain Tauri-injected State; frontend payloads remain the same.

The original plugin order is retained: single-instance handling and close events precede credentials, IPC registration and the shell plugin. Both desktop and mobile setup use the same reliability and research-maintenance owners.

## HK and test ownership

Twelve current-main HK functions move into hk_market/api.rs. Existing crate-level entrypoints remain reexported; helper visibility is restricted to the original HK domain where parent tests require it.

Three existing HK test modules move to physical files while retaining their logical module names. The 167 existing root tests are grouped into six physical files included within the original tests module. Their test identities and existing ignores are preserved. Relative compile-time fixture includes are resolved to the same source files.

The reviewed core, research and market decomposition, the bounded job cancellation/timeout test, and the frozen public-contract/test baselines remain authoritative.

## Source identity and checks

The existing desktop fingerprint adds seven production owners immediately after its lib.rs input: app/mod.rs, bootstrap.rs, command_registry.rs, commands.rs, desktop_window.rs, lifecycle.rs and state.rs. The existing relative input order, source framing, domain version and derived identities are retained. The development descriptor declares the same ordered input expansion.

The composition guard preserves all 153 IPC command names, the original exact RAG gate, owned fingerprint inputs and bounded source owners. The screening size allowance accounts for the 22 metadata lines required by those seven fingerprint inputs.

Observed local verification at this stage:

- Composition guard: 6 passed.
- Full desktop default suite: 824 passed, 0 failed, 8 original ignores.
- Frozen native inventory: all 827 original identities covered; 832 current cases accounted for.
- Rust architecture controls: 37 passed; frozen public contracts, dependency boundaries and ordered fingerprints passed.
- Python guard controls: 20 passed.

The original 45-file source snapshot and migration/body-association evidence are preserved locally under artifacts/pending-integration. This integration does not restore calibration or release qualification; those require the normal current-source real-input process.
