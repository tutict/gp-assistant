[CmdletBinding()]
param([switch] $Offline)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
# Compile the real pure-Rust modules without editing the parent's module registry/Cargo manifest.
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('gp-reliability-tests-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
$sourceRoot = ($root -replace '\\', '/') + '/desktop/src-tauri/src'
@"
[package]
name = "gp-reliability-tests"
version = "0.0.0"
edition = "2021"
[lib]
path = "lib.rs"
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
gp-test-durability = { path = "durability-support" }
# Command macros compile the actual API annotations; runtime IPC is integration-tested by parent.
tauri = { package = "tauri-macros", version = "2" }
[features]
gepa-lab = []
"@ | Set-Content -Encoding utf8 (Join-Path $temporary 'Cargo.toml')
@"
#![allow(dead_code)]
mod durability { pub(crate) use gp_test_durability::atomic_write; }
#[path = "$sourceRoot/diagnostics.rs"]
mod diagnostics;
"@ | Set-Content -Encoding utf8 (Join-Path $temporary 'lib.rs')
$support = Join-Path $temporary 'durability-support'
New-Item -ItemType Directory -Path $support | Out-Null
@"
[package]
name = "gp-test-durability"
version = "0.0.0"
edition = "2021"
[lib]
path = "lib.rs"
[dependencies]
serde_json = "1"
sha2 = "0.11"
rusqlite = { version = "0.40", features = ["bundled"] }
"@ | Set-Content -Encoding utf8 (Join-Path $support 'Cargo.toml')
@"
#![allow(dead_code)]
#[path = "$sourceRoot/durability.rs"]
mod implementation;
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> { implementation::atomic_write(path, bytes) }
"@ | Set-Content -Encoding utf8 (Join-Path $support 'lib.rs')
$argsList = @('test', '--manifest-path', (Join-Path $temporary 'Cargo.toml'), '--target-dir', (Join-Path $root 'tmp/reliability-native-target'))
if ($Offline) { $argsList += '--offline' }
$argsList += @('diagnostics::tests', '--', '--test-threads=1')
& cargo @argsList
if ($LASTEXITCODE -ne 0) { throw "Native reliability fixtures failed ($LASTEXITCODE). Temporary harness retained: $temporary" }
Write-Host "Native reliability fixtures passed. Temporary harness: $temporary"


