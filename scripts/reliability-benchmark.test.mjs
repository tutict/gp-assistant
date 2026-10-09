import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL, fileURLToPath } from 'node:url';
const moduleUrl = new URL('./reliability-benchmark.mjs', import.meta.url);
// Hand-authored synthetic unit input. Never a measured device baseline.
const fixture = () => ({ schema_version: 1, device_id: 'unit-device', platform: 'windows', fixture_id: 'offline-fixture-v1', metrics: { coldstartup_ms: 100, offlinep95_ms: 200, peak_memory_bytes: 1000, background_requests: 10 } });
const load = () => import(moduleUrl.href);

test('10% exactly passes, greater than 10% fails for each required metric', async () => {
  const { compareBaselines } = await load();
  for (const [metric, boundary, over] of [['coldstartup_ms',110,110.001],['offlinep95_ms',220,220.01],['peak_memory_bytes',1100,1101],['background_requests',11,12]]) {
    const candidate = fixture(); candidate.metrics[metric] = boundary;
    assert.equal(compareBaselines(fixture(), candidate).status, 'pass');
    candidate.metrics[metric] = over;
    assert.equal(compareBaselines(fixture(), candidate).status, 'fail');
    assert.deepEqual(compareBaselines(fixture(), candidate).regressions.map(r => r.metric), [metric]);
  }
});
test('compares identical device, platform, and fixture identity only', async () => {
  const { compareBaselines } = await load();
  for (const key of ['device_id','platform','fixture_id']) {
    const candidate = fixture(); candidate[key] += '-other';
    assert.throws(() => compareBaselines(fixture(), candidate), /metadata/);
  }
});
test('unknown, missing, negative, string, nonfinite and fractional count metrics reject', async () => {
  const { compareBaselines } = await load();
  for (const mutate of [f=>{ f.metrics.typo_ms=1; }, f=>{ delete f.metrics.offlinep95_ms; }, f=>{ f.metrics.offlinep95_ms=-1; }, f=>{ f.metrics.offlinep95_ms='2'; }, f=>{ f.metrics.offlinep95_ms=NaN; }, f=>{ f.metrics.peak_memory_bytes=Infinity; }, f=>{ f.metrics.background_requests=1.5; }, f=>{ f.metrics.peak_memory_bytes=1.5; }, f=>{ f.extra='x'; }]) {
    const invalid = fixture(); mutate(invalid);
    assert.throws(() => compareBaselines(fixture(), invalid), /invalid|unknown|missing/);
  }
});
test('zero baseline is not ignored or divided away', async () => {
  const { compareBaselines } = await load();
  const baseline=fixture(), candidate=fixture(); baseline.metrics.background_requests=0; candidate.metrics.background_requests=0;
  assert.equal(compareBaselines(baseline,candidate).status,'pass');
  candidate.metrics.background_requests=1;
  assert.equal(compareBaselines(baseline,candidate).status,'fail');
});
test('only an explicit metric-scoped meaningful reason exempts a regression', async () => {
  const { compareBaselines } = await load();
  const candidate=fixture(); candidate.metrics.coldstartup_ms=150; candidate.metrics.offlinep95_ms=350;
  const exemption={ schema_version:1, exemptions:[{metric:'coldstartup_ms', reason:'Accepted startup tradeoff for verified integrity checks; tracked in REL-1.'}] };
  const result=compareBaselines(fixture(),candidate,exemption);
  assert.equal(result.status,'fail'); assert.equal(result.regressions[0].exempted,true); assert.equal(result.regressions[1].exempted,false);
  exemption.exemptions.push({metric:'offlinep95_ms',reason:'Accepted temporary offline index migration cost; tracked in REL-2.'});
  assert.equal(compareBaselines(fixture(),candidate,exemption).status,'exempted');
  for (const bad of [{metric:'typo',reason:'This is a sufficiently long explanation.'},{metric:'coldstartup_ms',reason:' '},{metric:'coldstartup_ms',reason:'ok'}]) {
    assert.throws(()=>compareBaselines(fixture(),candidate,{schema_version:1,exemptions:[bad]}), /exemption/);
  }
});
test('CLI requires actual input files and cannot waive absent device readings', () => {
  const result=spawnSync(process.execPath,[fileURLToPath(moduleUrl),'--baseline','no-such-baseline.json','--candidate','no-such-candidate.json'],{encoding:'utf8'});
  assert.equal(result.status,2); assert.match(result.stdout,/requires_user/);
});
test('CLI evaluates temporary JSON fixtures and fails closed on unknown options', () => {
  const root=mkdtempSync(join(tmpdir(),'gp-reliability-gate-'));
  try {
    const baseline=join(root,'baseline.json'), candidate=join(root,'candidate.json');
    writeFileSync(baseline,JSON.stringify(fixture())); const regression=fixture(); regression.metrics.offlinep95_ms=250;
    writeFileSync(candidate,JSON.stringify(regression));
    const args=[fileURLToPath(moduleUrl),'--baseline',baseline,'--candidate',candidate];
    let result=spawnSync(process.execPath,args,{encoding:'utf8'}); assert.equal(result.status,1); assert.equal(JSON.parse(result.stdout).status,'fail');
    result=spawnSync(process.execPath,[...args,'--allow-regression'],{encoding:'utf8'}); assert.equal(result.status,2);
  } finally { rmSync(root,{recursive:true,force:true}); }
});

test('fractional readings preserve the exact 10% boundary without float rounding exemptions', async () => {
  const { compareBaselines }=await load();
  const base=fixture(), candidate=fixture();
  base.metrics.coldstartup_ms=0.3; candidate.metrics.coldstartup_ms=0.33;
  assert.equal(compareBaselines(base,candidate).status,'pass');
  candidate.metrics.coldstartup_ms=0.33000000000000007;
  assert.equal(compareBaselines(base,candidate).status,'fail');
});
