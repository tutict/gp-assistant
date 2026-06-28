# Tauri Migration Audit

Last checked: 2026-06-28.

The migration is complete. The Python/FastAPI service layer, dependency file, Python tests, and legacy launchers have been removed from the repository. The maintained runtime is Tauri + Rust for both Windows desktop and Android.

## Current Runtime

| Area | Maintained path |
| --- | --- |
| Frontend source | `desktop/frontend/` |
| Generated frontend assets | `desktop/mobile-dist/` |
| Tauri shell and commands | `desktop/src-tauri/` |
| Native core library | `native/gp-core/` |
| Android build scripts | `scripts/build-android.ps1`, `scripts/prepare-tauri-android-assets.ps1` |
| Cache maintenance | `scripts/maintain-data-tauri.ps1` |

## Replaced Surfaces

| Old surface | Replacement |
| --- | --- |
| standalone HTTP app | Tauri command bridge in `desktop/frontend/src/lib/tauri.ts` |
| screen/sector/graph/trend/backtest routes | Rust commands registered in `desktop/src-tauri/src/lib.rs` and core logic in `native/gp-core` |
| data maintenance routes | Tauri data source bar and Rust cache commands |
| RAG pack routes | Rust lightweight RAG pack commands in `desktop/src-tauri/src/rag_pack.rs` |
| upstream RAG transfer | inline descriptor JSON and mobile import commands |
| Python test suite | Rust and frontend build checks |

## Notes

The app may still expose route-shaped names in frontend helper functions for compatibility with existing UI code. Those calls are handled inside the Tauri bridge and do not require an external server.