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
assert.match(gepaSource, /cfg!\(\s*all\(\s*feature\s*=\s*"gepa-lab"\s*,\s*any\(\s*target_os\s*=\s*"windows"\s*,\s*target_os\s*=\s*"android"\s*\)\s*\)\s*\)/s);
assert.match(androidSource, /\[switch\] \$SensitiveArguments/);
assert.match(androidSource, /\[arguments redacted\]/);
assert.match(androidSource, /-SensitiveArguments -Arguments/);

console.log("Release package build contract passed.");

// Behavioral additions run the real PowerShell gate with packaging deliberately skipped.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
const releasePath=fileURLToPath(new URL('./release-check.ps1',import.meta.url));
const skipped=['-SkipAndroidPreflight','-SkipRust','-SkipNode','-SkipPrepare','-SkipPackageBuild','-SkipReliability'];
function release(args) {
  return spawnSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',releasePath,...skipped,...args],{encoding:'utf8',timeout:30000});
}
test('release gate rejects requested-but-absent measured baselines',()=>{
  const result=release(['-EvaluateBaseline']);
  assert.notEqual(result.status,0); assert.match(result.stdout+result.stderr,/requires.user|requires.*BaselinePath/i);
});
test('release gate rejects silently unused baseline/exemption arguments',()=>{
  const result=release(['-BaselinePath','absent.json']);
  assert.notEqual(result.status,0); assert.match(result.stdout+result.stderr,/EvaluateBaseline/);
});
test('release gate reports absent measurements as unverified instead of inventing a pass',()=>{
  const result=release([]);
  assert.equal(result.status,0,result.stdout+result.stderr);
  assert.match(result.stdout,/NOT EVALUATED.*requires.user/i);
  assert.match(result.stdout,/SKIPPED.*reliability/i);
});
test('release baseline option executes threshold validation against temporary inputs',()=>{
  const temporary=mkdtempSync(join(tmpdir(),'gp-release-gate-'));
  try {
    const baseline={schema_version:1,device_id:'fixture-device',platform:'windows',fixture_id:'unit-only',metrics:{coldstartup_ms:100,offlinep95_ms:100,peak_memory_bytes:1000,background_requests:0}};
    const base=join(temporary,'baseline.json'), candidate=join(temporary,'candidate.json');
    writeFileSync(base,JSON.stringify(baseline)); writeFileSync(candidate,JSON.stringify({...baseline,metrics:{...baseline.metrics,coldstartup_ms:111}}));
    let result=release(['-EvaluateBaseline','-BaselinePath',base,'-CandidatePath',candidate]);
    assert.notEqual(result.status,0); assert.match(result.stdout,/"status": "fail"/);
    writeFileSync(candidate,JSON.stringify({...baseline,metrics:{...baseline.metrics,coldstartup_ms:110}}));
    result=release(['-EvaluateBaseline','-BaselinePath',base,'-CandidatePath',candidate]);
    assert.equal(result.status,0,result.stdout+result.stderr); assert.match(result.stdout,/"status": "pass"/);
    assert.match(result.stdout,/SKIPPED.*reliability/i);
  } finally { rmSync(temporary,{recursive:true,force:true}); }
});
