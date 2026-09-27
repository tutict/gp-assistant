import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./release-check.ps1", import.meta.url), "utf8");
const androidSource = readFileSync(new URL("./build-android.ps1", import.meta.url), "utf8");
const desktopPackage = JSON.parse(readFileSync(new URL("../desktop/package.json", import.meta.url), "utf8"));
const gepaSource = readFileSync(new URL("../desktop/src-tauri/src/gepa_lab.rs", import.meta.url), "utf8");

assert.match(source, /if \(-not \$SkipPackageBuild\)/);
assert.match(source, /if \(-not \$AllowUnsignedAndroid\)[\s\S]*?\$androidArgs \+= "-Signed"/);
assert.match(source, /Build Android release package/);
assert.match(source, /Name -match "signed"/);
assert.match(source, /Build Windows NSIS installer/);
assert.match(source, /target\/release\/bundle\/nsis/);

assert.match(desktopPackage.scripts["dev:gepa"], /--features gepa-lab/);
assert.doesNotMatch(desktopPackage.scripts["dev"], /--features gepa-lab/);
assert.match(desktopPackage.scripts["build:windows"], /--features gepa-lab/);
assert.match(desktopPackage.scripts["build:android"], /--features gepa-lab/);
assert.match(androidSource, /"--features", "gepa-lab"/);
assert.match(gepaSource, /any\(target_os = "windows", target_os = "android"\)/);
assert.match(gepaSource, /cfg!\(all\(feature = "gepa-lab", any\(target_os = "windows", target_os = "android"\)\)\)/);
assert.match(androidSource, /\[switch\] \$SensitiveArguments/);
assert.match(androidSource, /\[arguments redacted\]/);
assert.match(androidSource, /-SensitiveArguments -Arguments/);

console.log("Release package build contract passed.");
