import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./release-check.ps1", import.meta.url), "utf8");
const androidSource = readFileSync(new URL("./build-android.ps1", import.meta.url), "utf8");

assert.match(source, /if \(-not \$SkipPackageBuild\)/);
assert.match(source, /if \(-not \$AllowUnsignedAndroid\)[\s\S]*?\$androidArgs \+= "-Signed"/);
assert.match(source, /Build Android release package/);
assert.match(source, /Name -match "signed"/);
assert.match(source, /Build Windows NSIS installer/);
assert.match(source, /target\/release\/bundle\/nsis/);

assert.match(androidSource, /\[switch\] \$SensitiveArguments/);
assert.match(androidSource, /\[arguments redacted\]/);
assert.match(androidSource, /-SensitiveArguments -Arguments/);

console.log("Release package build contract passed.");
