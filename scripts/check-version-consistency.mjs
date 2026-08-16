import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function readJson(path) {
  return JSON.parse(readFileSync(resolve(repoRoot, path), "utf8"));
}

function cargoPackageVersion(path) {
  const source = readFileSync(resolve(repoRoot, path), "utf8");
  const packageHeader = "[package]";
  const packageStart = source.indexOf(packageHeader);
  assert.notEqual(packageStart, -1, `${path} must declare [package]`);
  const bodyStart = packageStart + packageHeader.length;
  const nextSection = source.indexOf("\n[", bodyStart);
  const packageSection = source.slice(bodyStart, nextSection === -1 ? source.length : nextSection);
  const version = packageSection.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  assert.ok(version, `${path} must declare [package].version`);
  return version;
}

function cargoLockPackageVersion(path, packageName) {
  const source = readFileSync(resolve(repoRoot, path), "utf8");
  const packageBlocks = source.split(/\r?\n\[\[package\]\]\r?\n/);
  const matchingBlocks = packageBlocks.filter((block) => {
    return block.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1] === packageName;
  });
  assert.equal(matchingBlocks.length, 1, `${path} must contain one ${packageName} package`);
  const version = matchingBlocks[0].match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  assert.ok(version, `${path} package ${packageName} must declare version`);
  return version;
}

const desktopPackage = readJson("desktop/package.json");
const desktopPackageLock = readJson("desktop/package-lock.json");
const frontendPackage = readJson("desktop/frontend/package.json");
const frontendPackageLock = readJson("desktop/frontend/package-lock.json");
const tauriConfig = readJson("desktop/src-tauri/tauri.conf.json");
const androidConfig = readJson("desktop/src-tauri/tauri.android.conf.json");
const versions = new Map([
  ["desktop/package.json", desktopPackage.version],
  ["desktop/package-lock.json", desktopPackageLock.version],
  ["desktop/package-lock.json root package", desktopPackageLock.packages?.[""]?.version],
  ["desktop/frontend/package.json", frontendPackage.version],
  ["desktop/frontend/package-lock.json", frontendPackageLock.version],
  ["desktop/frontend/package-lock.json root package", frontendPackageLock.packages?.[""]?.version],
  ["desktop/src-tauri/tauri.conf.json", tauriConfig.version],
  ["desktop/src-tauri/Cargo.toml", cargoPackageVersion("desktop/src-tauri/Cargo.toml")],
  [
    "desktop/src-tauri/Cargo.lock stock-optimizer-desktop",
    cargoLockPackageVersion("desktop/src-tauri/Cargo.lock", "stock-optimizer-desktop"),
  ],
  ["native/gp-core/Cargo.toml", cargoPackageVersion("native/gp-core/Cargo.toml")],
  [
    "native/gp-core/Cargo.lock stock-optimizer-core",
    cargoLockPackageVersion("native/gp-core/Cargo.lock", "stock-optimizer-core"),
  ],
]);
const expectedVersion = desktopPackage.version;

for (const [path, version] of versions) {
  assert.equal(version, expectedVersion, `${path} version must match ${expectedVersion}`);
}

const semver = /^(\d+)\.(\d+)\.(\d+)$/.exec(expectedVersion);
assert.ok(semver, `Release version must be numeric semver: ${expectedVersion}`);
const expectedVersionCode = Number(semver[1]) * 10_000 + Number(semver[2]) * 100 + Number(semver[3]);
assert.equal(
  androidConfig.bundle?.android?.versionCode,
  expectedVersionCode,
  `Android versionCode must be ${expectedVersionCode} for ${expectedVersion}`,
);

console.log(`Version metadata is consistent at ${expectedVersion} (Android ${expectedVersionCode}).`);
